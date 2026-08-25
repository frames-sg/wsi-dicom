use std::time::Duration;

use wsi_rs::Slide;

use super::super::j2k_policy::{
    LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES, LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_DENOMINATOR,
    LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_NUMERATOR,
};
use super::super::jpeg_baseline::JpegBaselineFrameLocation;
use super::super::lossless_j2k_direct_routes::encode_cpu_input_tile;
use super::super::lossless_j2k_plan::LosslessJ2kPlannedFrame;
use super::super::route_cache::AutoLosslessJ2kRouteDecision;
use super::{
    try_encode_metal_input_tile_run, MetalEncodedTileRun, MetalInputTileReader,
    MetalInputTileRunRequest,
};
use crate::encode::{self, DicomJ2kEncoder, EncodedDicomJ2kFrame};
use crate::error::Error;
use crate::tile::PixelProfile;

pub(in crate::export) struct RoutedLosslessJ2kTile {
    pub(in crate::export) encoded: Result<EncodedDicomJ2kFrame, Error>,
    pub(in crate::export) profile: PixelProfile,
    pub(in crate::export) used_gpu_input: bool,
}

pub(in crate::export) struct CpuEncodedTileRun {
    pub(in crate::export) tiles: Vec<(Result<EncodedDicomJ2kFrame, Error>, PixelProfile)>,
    pub(in crate::export) input_decode_duration: Duration,
    pub(in crate::export) compose_duration: Duration,
}

pub(in crate::export) struct AutoMetalInputProbeRun {
    pub(in crate::export) tiles: Vec<Option<RoutedLosslessJ2kTile>>,
    pub(in crate::export) input_decode_duration: Duration,
    pub(in crate::export) compose_duration: Duration,
    pub(in crate::export) gpu_input_decode_batches: u64,
    pub(in crate::export) gpu_compose_batches: u64,
    pub(in crate::export) gpu_encode_batches: u64,
    pub(in crate::export) gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats,
    pub(in crate::export) probe_cpu_duration: Duration,
    pub(in crate::export) probe_gpu_duration: Duration,
    pub(in crate::export) probe_gpu_batches: u64,
    pub(in crate::export) route: AutoLosslessJ2kRouteDecision,
}

pub(in crate::export) struct AutoMetalInputProbeRequest<'a> {
    pub(in crate::export) level: &'a wsi_rs::Level,
    pub(in crate::export) location: JpegBaselineFrameLocation,
    pub(in crate::export) row: u64,
    pub(in crate::export) planned: &'a [LosslessJ2kPlannedFrame],
    pub(in crate::export) route_scope_frames: usize,
    pub(in crate::export) matrix_columns: u64,
    pub(in crate::export) matrix_rows: u64,
    pub(in crate::export) tile_size: u32,
}

pub(in crate::export) fn probe_auto_metal_input_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    request: AutoMetalInputProbeRequest<'_>,
) -> Result<AutoMetalInputProbeRun, Error> {
    let AutoMetalInputProbeRequest {
        level,
        location,
        row,
        planned,
        route_scope_frames,
        matrix_columns,
        matrix_rows,
        tile_size,
    } = request;
    let first = planned.first().ok_or_else(|| Error::Unsupported {
        reason: "auto Metal input route probe requires at least one tile".into(),
    })?;

    let metal_run = try_encode_metal_input_tile_run(
        slide,
        metal_input,
        j2k_encoder,
        MetalInputTileRunRequest {
            level,
            location,
            row,
            start_col: first.col,
            tile_count: planned.len(),
            matrix_columns,
            matrix_rows,
            tile_size,
        },
    )?;
    let mut cpu_probe_encoder = j2k_encoder.cpu_only_peer();
    let cpu_run = encode_cpu_input_planned_tile_run(
        slide,
        &mut cpu_probe_encoder,
        CpuInputPlannedTileRunRequest {
            location,
            planned,
            tile_size,
        },
    )?;
    let partial_gpu_run =
        if cpu_input_device_encode_auto_probe_allowed(&cpu_run, route_scope_frames) {
            let mut partial_probe_encoder = j2k_encoder.require_device_peer();
            Some(encode_cpu_input_planned_tile_run(
                slide,
                &mut partial_probe_encoder,
                CpuInputPlannedTileRunRequest {
                    location,
                    planned,
                    tile_size,
                },
            )?)
        } else {
            None
        };

    let resident_gpu_complete = metal_run.tiles.iter().all(Option::is_some);
    let partial_gpu_complete = partial_gpu_run.as_ref().is_some_and(|partial_gpu_run| {
        partial_gpu_run
            .tiles
            .iter()
            .all(|(encoded, _)| matches!(encoded, Ok(encoded) if encoded.used_device_encode))
    });
    let cpu_complete = cpu_run.tiles.iter().all(|(encoded, _)| encoded.is_ok());
    let resident_gpu_duration = metal_encoded_tile_run_total_duration(&metal_run);
    let partial_gpu_duration = partial_gpu_run
        .as_ref()
        .map(cpu_encoded_tile_run_total_duration)
        .unwrap_or(Duration::ZERO);
    let cpu_duration = cpu_encoded_tile_run_total_duration(&cpu_run);
    let route = select_auto_lossless_j2k_probe_route(
        AutoLosslessJ2kRouteCandidate {
            complete: cpu_complete,
            duration: cpu_duration,
        },
        AutoLosslessJ2kRouteCandidate {
            complete: partial_gpu_complete,
            duration: partial_gpu_duration,
        },
        AutoLosslessJ2kRouteCandidate {
            complete: resident_gpu_complete,
            duration: resident_gpu_duration,
        },
    );
    metal_input.record_auto_route_probe_decision(route);
    if route == AutoLosslessJ2kRouteDecision::CpuOnly {
        j2k_encoder.force_cpu_only_for_auto();
    }

    let probe_gpu_batches = metal_run
        .input_decode_batches
        .saturating_add(metal_run.compose_batches)
        .saturating_add(metal_run.encode_batches);
    let metal_input_decode_duration = metal_run.input_decode_duration;
    let metal_compose_duration = metal_run.compose_duration;
    let metal_input_decode_batches = metal_run.input_decode_batches;
    let metal_compose_batches = metal_run.compose_batches;
    let metal_encode_batches = metal_run.encode_batches;
    let metal_gpu_encode_stats = metal_run.gpu_encode_stats;
    let cpu_input_decode_duration = cpu_run.input_decode_duration;
    let cpu_compose_duration = cpu_run.compose_duration;
    match route {
        AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode => Ok(AutoMetalInputProbeRun {
            tiles: metal_run
                .tiles
                .into_iter()
                .map(|entry| {
                    entry.map(|(encoded, profile)| RoutedLosslessJ2kTile {
                        encoded: Ok(encoded),
                        profile,
                        used_gpu_input: true,
                    })
                })
                .collect(),
            input_decode_duration: metal_input_decode_duration,
            compose_duration: metal_compose_duration,
            gpu_input_decode_batches: metal_input_decode_batches,
            gpu_compose_batches: metal_compose_batches,
            gpu_encode_batches: metal_encode_batches,
            gpu_encode_stats: metal_gpu_encode_stats,
            probe_cpu_duration: cpu_duration,
            probe_gpu_duration: resident_gpu_duration,
            probe_gpu_batches,
            route,
        }),
        AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode => {
            let partial_gpu_run = partial_gpu_run.ok_or_else(|| Error::Unsupported {
                reason: "auto route selected CPU-input device encode without a completed probe"
                    .into(),
            })?;
            Ok(AutoMetalInputProbeRun {
                tiles: partial_gpu_run
                    .tiles
                    .into_iter()
                    .map(|(encoded, profile)| {
                        Some(RoutedLosslessJ2kTile {
                            encoded,
                            profile,
                            used_gpu_input: false,
                        })
                    })
                    .collect(),
                input_decode_duration: partial_gpu_run.input_decode_duration,
                compose_duration: partial_gpu_run.compose_duration,
                gpu_input_decode_batches: 0,
                gpu_compose_batches: 0,
                gpu_encode_batches: 0,
                gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats::default(),
                probe_cpu_duration: cpu_duration,
                probe_gpu_duration: resident_gpu_duration,
                probe_gpu_batches,
                route,
            })
        }
        AutoLosslessJ2kRouteDecision::CpuOnly | AutoLosslessJ2kRouteDecision::Undecided => {
            Ok(AutoMetalInputProbeRun {
                tiles: cpu_run
                    .tiles
                    .into_iter()
                    .map(|(encoded, profile)| {
                        Some(RoutedLosslessJ2kTile {
                            encoded,
                            profile,
                            used_gpu_input: false,
                        })
                    })
                    .collect(),
                input_decode_duration: cpu_input_decode_duration,
                compose_duration: cpu_compose_duration,
                gpu_input_decode_batches: 0,
                gpu_compose_batches: 0,
                gpu_encode_batches: 0,
                gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats::default(),
                probe_cpu_duration: cpu_duration,
                probe_gpu_duration: resident_gpu_duration,
                probe_gpu_batches,
                route,
            })
        }
    }
}

#[derive(Clone, Copy)]
struct CpuInputPlannedTileRunRequest<'a> {
    location: JpegBaselineFrameLocation,
    planned: &'a [LosslessJ2kPlannedFrame],
    tile_size: u32,
}

fn encode_cpu_input_planned_tile_run(
    slide: &Slide,
    j2k_encoder: &mut DicomJ2kEncoder,
    request: CpuInputPlannedTileRunRequest<'_>,
) -> Result<CpuEncodedTileRun, Error> {
    let CpuInputPlannedTileRunRequest {
        location,
        planned,
        tile_size,
    } = request;
    let mut tiles = Vec::new();
    tiles
        .try_reserve_exact(planned.len())
        .map_err(|_| Error::Unsupported {
            reason: "CPU fallback tile batch exceeds available memory".into(),
        })?;
    let mut input_decode_duration = Duration::ZERO;
    let mut compose_duration = Duration::ZERO;
    for planned_frame in planned {
        let (encoded, profile, frame_input_decode_duration, frame_compose_duration) =
            encode_cpu_input_tile(
                slide,
                j2k_encoder,
                location,
                planned_frame.rect(),
                tile_size,
            )?;
        input_decode_duration = input_decode_duration.saturating_add(frame_input_decode_duration);
        compose_duration = compose_duration.saturating_add(frame_compose_duration);
        tiles.push((encoded, profile));
    }
    Ok(CpuEncodedTileRun {
        tiles,
        input_decode_duration,
        compose_duration,
    })
}

fn cpu_encoded_tile_run_total_duration(run: &CpuEncodedTileRun) -> Duration {
    run.tiles.iter().fold(
        run.input_decode_duration
            .saturating_add(run.compose_duration),
        |duration, (encoded, _)| match encoded {
            Ok(encoded) => duration
                .saturating_add(encoded.encode_duration)
                .saturating_add(encoded.validation_duration),
            Err(_) => duration,
        },
    )
}

pub(in crate::export) fn cpu_input_device_encode_auto_allowed(run: &CpuEncodedTileRun) -> bool {
    run.tiles.iter().all(|(_, profile)| {
        matches!(profile.components, 1 | 3) && matches!(profile.bits_allocated, 8 | 16)
    })
}

pub(in crate::export) fn cpu_input_device_encode_auto_probe_allowed(
    run: &CpuEncodedTileRun,
    frame_count: usize,
) -> bool {
    frame_count >= LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES
        && cpu_input_device_encode_auto_allowed(run)
}

fn metal_encoded_tile_run_total_duration(run: &MetalEncodedTileRun) -> Duration {
    run.tiles.iter().fold(
        run.input_decode_duration
            .saturating_add(run.compose_duration),
        |duration, encoded| match encoded {
            Some((encoded, _)) => duration
                .saturating_add(encoded.encode_duration)
                .saturating_add(encoded.validation_duration),
            None => duration,
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::export) struct AutoLosslessJ2kRouteCandidate {
    pub(in crate::export) complete: bool,
    pub(in crate::export) duration: Duration,
}

pub(in crate::export) fn select_auto_lossless_j2k_probe_route(
    cpu_only: AutoLosslessJ2kRouteCandidate,
    cpu_input_device_encode: AutoLosslessJ2kRouteCandidate,
    gpu_input_device_encode: AutoLosslessJ2kRouteCandidate,
) -> AutoLosslessJ2kRouteDecision {
    if !cpu_only.complete {
        return [
            (
                AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode,
                cpu_input_device_encode,
            ),
            (
                AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode,
                gpu_input_device_encode,
            ),
        ]
        .into_iter()
        .filter(|(_, candidate)| candidate.complete)
        .min_by_key(|(_, candidate)| candidate.duration)
        .map(|(route, _)| route)
        .unwrap_or(AutoLosslessJ2kRouteDecision::CpuOnly);
    }

    let mut selected = (AutoLosslessJ2kRouteDecision::CpuOnly, cpu_only.duration);
    for (route, candidate) in [
        (
            AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode,
            cpu_input_device_encode,
        ),
        (
            AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode,
            gpu_input_device_encode,
        ),
    ] {
        if candidate.complete
            && route_beats_cpu_baseline(candidate.duration, cpu_only.duration)
            && candidate.duration < selected.1
        {
            selected = (route, candidate.duration);
        }
    }
    selected.0
}

fn route_beats_cpu_baseline(route_duration: Duration, cpu_duration: Duration) -> bool {
    route_duration
        .as_nanos()
        .saturating_mul(LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_DENOMINATOR)
        < cpu_duration
            .as_nanos()
            .saturating_mul(LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_NUMERATOR)
}
