use std::time::{Duration, Instant};

use wsi_rs::Slide;

use super::super::cpu_batch::frame_batch_len;
use super::super::j2k_policy::{
    LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES, LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_DENOMINATOR,
    LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_NUMERATOR,
};
use super::super::jpeg_baseline::JpegBaselineFrameLocation;
use super::super::lossless_j2k_cpu::{
    encode_prepared_lossless_j2k_cpu_batch, lossless_j2k_samples_from_prepared_region,
    prepare_cpu_input_batch, LosslessJ2kCpuBatchSettings,
};
use super::super::lossless_j2k_plan::LosslessJ2kPlannedFrame;
use super::super::route_cache::AutoLosslessJ2kRouteDecision;
use super::{try_encode_metal_input_tile_run, MetalInputTileReader, MetalInputTileRunRequest};
use crate::encode::{self, DicomJ2kEncoder, EncodedDicomJ2kFrame};
use crate::error::Error;
use crate::tile::PixelProfile;

pub(in crate::export) struct RoutedLosslessJ2kTile {
    pub(in crate::export) encoded: Result<EncodedDicomJ2kFrame, Error>,
    pub(in crate::export) profile: PixelProfile,
    pub(in crate::export) used_gpu_input: bool,
}

pub(in crate::export) struct CpuEncodedTileRun {
    pub(in crate::export) wall_duration: Duration,
    pub(in crate::export) gpu_encode_batches: u64,
    pub(in crate::export) gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats,
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

    let previous_limit = metal_input.frame_limit;
    let probe_end = row
        .checked_mul(matrix_columns.div_ceil(u64::from(tile_size)))
        .and_then(|start| start.checked_add(first.col))
        .and_then(|start| start.checked_add(planned.len() as u64))
        .ok_or_else(|| Error::Unsupported {
            reason: "auto route probe frame limit overflow".into(),
        })?;
    metal_input.frame_limit = Some(previous_limit.map_or(probe_end, |limit| limit.min(probe_end)));
    let gpu_started = Instant::now();
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
    );
    metal_input.frame_limit = previous_limit;
    let resident_gpu_duration = gpu_started.elapsed();
    let metal_run = metal_run?;
    let cpu_probe_encoder = j2k_encoder.cpu_only_peer();
    let mut partial_probe_encoder = (route_scope_frames
        >= LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES)
        .then(|| j2k_encoder.require_device_peer());
    let (cpu_run, partial_gpu_run) = encode_shared_cpu_input_probe_runs(
        slide,
        &cpu_probe_encoder,
        partial_probe_encoder.as_mut(),
        CpuInputPlannedTileRunRequest {
            level,
            max_prepared_frame_bytes: metal_input.max_prepared_frame_bytes,
            location,
            planned,
            tile_size,
        },
    )?;

    let resident_gpu_complete = metal_run.tiles.iter().all(Option::is_some);
    let partial_gpu_complete = partial_gpu_run.as_ref().is_some_and(|partial_gpu_run| {
        partial_gpu_run
            .tiles
            .iter()
            .all(|(encoded, _)| matches!(encoded, Ok(encoded) if encoded.used_device_encode))
    });
    let cpu_complete = cpu_run.tiles.iter().all(|(encoded, _)| encoded.is_ok());
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
                gpu_encode_batches: partial_gpu_run.gpu_encode_batches,
                gpu_encode_stats: partial_gpu_run.gpu_encode_stats,
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
    level: &'a wsi_rs::Level,
    max_prepared_frame_bytes: u64,
    location: JpegBaselineFrameLocation,
    planned: &'a [LosslessJ2kPlannedFrame],
    tile_size: u32,
}

fn encode_shared_cpu_input_probe_runs(
    slide: &Slide,
    cpu_encoder: &DicomJ2kEncoder,
    mut gpu_encoder: Option<&mut DicomJ2kEncoder>,
    request: CpuInputPlannedTileRunRequest<'_>,
) -> Result<(CpuEncodedTileRun, Option<CpuEncodedTileRun>), Error> {
    let CpuInputPlannedTileRunRequest {
        level,
        max_prepared_frame_bytes,
        location,
        planned,
        tile_size,
    } = request;
    let frames: Vec<_> = planned.iter().map(LosslessJ2kPlannedFrame::rect).collect();
    let mut cpu_run = CpuEncodedTileRun {
        tiles: Vec::with_capacity(frames.len()),
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
        wall_duration: Duration::ZERO,
        gpu_encode_batches: 0,
        gpu_encode_stats: Default::default(),
    };
    let mut gpu_run = gpu_encoder.as_ref().map(|_| CpuEncodedTileRun {
        tiles: Vec::with_capacity(frames.len()),
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
        wall_duration: Duration::ZERO,
        gpu_encode_batches: 0,
        gpu_encode_stats: Default::default(),
    });
    let Some((transfer_syntax, codec_validation, j2k_decomposition_levels, reversible_transform)) =
        cpu_encoder.cpu_batch_settings()
    else {
        return Err(Error::Encode {
            message: "auto route CPU probe is missing CPU encode settings".into(),
        });
    };
    let cpu_settings = LosslessJ2kCpuBatchSettings {
        transfer_syntax,
        codec_validation,
        j2k_decomposition_levels,
        reversible_transform,
        max_prepared_frame_bytes,
    };

    for batch in frames.chunks(frame_batch_len(tile_size, tile_size)) {
        let prepare_started = Instant::now();
        let prepared = prepare_cpu_input_batch(
            slide,
            level,
            location,
            batch,
            tile_size,
            max_prepared_frame_bytes,
        )?;
        let prepare_wall_duration = prepare_started.elapsed();
        let input_decode_duration = prepared.iter().fold(Duration::ZERO, |duration, tile| {
            duration.saturating_add(tile.input_decode_duration)
        });
        let compose_duration = prepared.iter().fold(Duration::ZERO, |duration, tile| {
            duration.saturating_add(tile.compose_duration)
        });

        let cpu_encode_started = Instant::now();
        let outcomes = encode_prepared_lossless_j2k_cpu_batch(cpu_settings, &prepared, tile_size)?;
        cpu_run.wall_duration = cpu_run
            .wall_duration
            .saturating_add(prepare_wall_duration)
            .saturating_add(cpu_encode_started.elapsed());
        cpu_run.input_decode_duration = cpu_run
            .input_decode_duration
            .saturating_add(input_decode_duration);
        cpu_run.compose_duration = cpu_run.compose_duration.saturating_add(compose_duration);
        for outcome in outcomes {
            cpu_run.tiles.push((outcome.encoded, outcome.profile));
        }

        if prepared
            .iter()
            .any(|tile| !cpu_input_device_encode_profile_allowed(tile.profile))
        {
            gpu_encoder = None;
            gpu_run = None;
        }
        if let (Some(encoder), Some(run)) = (gpu_encoder.as_deref_mut(), gpu_run.as_mut()) {
            let samples = prepared
                .iter()
                .map(|tile| lossless_j2k_samples_from_prepared_region(tile, tile_size))
                .collect::<Result<Vec<_>, _>>()?;
            let gpu_encode_started = Instant::now();
            let encoded = match encoder.encode_host_samples_batch(&samples, tile_size, tile_size) {
                Ok(encoded) => {
                    run.gpu_encode_stats.add_assign(encoded.gpu_encode_stats);
                    run.gpu_encode_batches += 1;
                    encoded
                        .frames
                        .into_iter()
                        .map(|frame| {
                            frame.ok_or_else(|| Error::Encode {
                                message: "device probe produced no encoded frame".into(),
                            })
                        })
                        .collect::<Vec<_>>()
                }
                Err(err) => prepared
                    .iter()
                    .map(|_| {
                        Err(Error::Encode {
                            message: err.to_string(),
                        })
                    })
                    .collect(),
            };
            run.wall_duration = run
                .wall_duration
                .saturating_add(prepare_wall_duration)
                .saturating_add(gpu_encode_started.elapsed());
            run.input_decode_duration = run
                .input_decode_duration
                .saturating_add(input_decode_duration);
            run.compose_duration = run.compose_duration.saturating_add(compose_duration);
            for (tile, encoded) in prepared.into_iter().zip(encoded) {
                run.tiles.push((encoded, tile.profile));
            }
        }
    }
    Ok((cpu_run, gpu_run))
}

fn cpu_encoded_tile_run_total_duration(run: &CpuEncodedTileRun) -> Duration {
    run.wall_duration
}

pub(in crate::export) fn cpu_input_device_encode_profile_allowed(profile: PixelProfile) -> bool {
    matches!(profile.components, 1 | 3) && matches!(profile.bits_allocated, 8 | 16)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinate::InstanceCoordinate;
    use crate::export::frame_region::FrameRectGrid;
    use crate::export::lossless_j2k_plan::{plan_lossless_j2k_frames, LosslessJ2kPlanRequest};
    use crate::options::{CodecValidation, EncodeBackendPreference, TransferSyntax};

    fn decode_rgb8_frame(frame: EncodedDicomJ2kFrame, tile_size: u32) -> Vec<u8> {
        let codestream = frame.into_codestream().expect("materialize codestream");
        let mut decoder = j2k::J2kDecoder::new(&codestream).expect("parse encoded frame");
        let stride = tile_size as usize * 3;
        let mut decoded = vec![0; stride * tile_size as usize];
        decoder
            .decode_into(&mut decoded, stride, j2k::PixelFormat::Rgb8)
            .expect("decode encoded frame");
        decoded
    }

    fn expected_rgb8_tiles(pixels: &[u8], width: usize, tile_size: usize) -> Vec<Vec<u8>> {
        (0..width / tile_size)
            .map(|tile_col| {
                let mut tile = Vec::with_capacity(tile_size * tile_size * 3);
                for row in 0..tile_size {
                    let start = (row * width + tile_col * tile_size) * 3;
                    tile.extend_from_slice(&pixels[start..start + tile_size * 3]);
                }
                tile
            })
            .collect()
    }

    #[test]
    fn shared_cpu_input_probe_prepares_once_for_cpu_and_device_candidates() {
        if j2k_metal_support::system_default_device().is_err() {
            return;
        }

        const TILE_SIZE: u32 = 16;
        const WIDTH: u32 = TILE_SIZE * 2;
        const HEIGHT: u32 = TILE_SIZE;
        let temp = tempfile::tempdir().expect("create temp dir");
        let source_path = temp.path().join("source.dcm");
        let source_pixels =
            crate::synthetic_source::deterministic_rgb_pixels(WIDTH, HEIGHT).expect("pixels");
        crate::synthetic_source::write_rgb_source_dicom(
            &source_path,
            "1.2.826.0.1.3680043.10.999.801",
            "1.2.826.0.1.3680043.10.999.800",
            WIDTH,
            HEIGHT,
            source_pixels.clone(),
        )
        .expect("write source DICOM");
        let slide = Slide::open(&source_path).expect("open source DICOM");
        let level = &slide.dataset().scenes[0].series[0].levels[0];
        let location = InstanceCoordinate::first_series_level(0);
        let planned = plan_lossless_j2k_frames(
            &slide,
            LosslessJ2kPlanRequest {
                location,
                start_row: 0,
                row_count: 1,
                start_col: 0,
                tile_count: 2,
                grid: FrameRectGrid {
                    matrix_columns: u64::from(WIDTH),
                    matrix_rows: u64::from(HEIGHT),
                    frame_columns: TILE_SIZE,
                    frame_rows: TILE_SIZE,
                },
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                allow_passthrough_probe: false,
            },
        )
        .expect("plan probe frames");
        let request = CpuInputPlannedTileRunRequest {
            level,
            max_prepared_frame_bytes: 64 * 1024 * 1024,
            location,
            planned: &planned,
            tile_size: TILE_SIZE,
        };
        let cpu_encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::CpuOnly,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::Disabled,
        );
        let mut device_encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::Disabled,
        );

        let (cpu_run, device_run) = encode_shared_cpu_input_probe_runs(
            &slide,
            &cpu_encoder,
            Some(&mut device_encoder),
            request,
        )
        .expect("encode shared probe candidates");
        let device_run = device_run.expect("device candidate");
        assert_eq!(cpu_run.tiles.len(), 2);
        assert_eq!(device_run.tiles.len(), 2);
        assert_eq!(
            cpu_run.input_decode_duration,
            device_run.input_decode_duration
        );
        assert_eq!(cpu_run.compose_duration, device_run.compose_duration);
        assert_eq!(device_run.gpu_encode_batches, 1);

        let expected = expected_rgb8_tiles(&source_pixels, WIDTH as usize, TILE_SIZE as usize);
        let cpu_pixels: Vec<_> = cpu_run
            .tiles
            .into_iter()
            .map(|(frame, profile)| {
                assert_eq!(profile.components, 3);
                let frame = frame.expect("CPU probe frame");
                assert!(!frame.used_device_encode);
                decode_rgb8_frame(frame, TILE_SIZE)
            })
            .collect();
        let device_pixels: Vec<_> = device_run
            .tiles
            .into_iter()
            .map(|(frame, profile)| {
                assert_eq!(profile.components, 3);
                let frame = frame.expect("device probe frame");
                assert!(frame.used_device_encode);
                decode_rgb8_frame(frame, TILE_SIZE)
            })
            .collect();
        assert_eq!(cpu_pixels, expected);
        assert_eq!(device_pixels, expected);

        let (cpu_only_run, device_run) =
            encode_shared_cpu_input_probe_runs(&slide, &cpu_encoder, None, request)
                .expect("encode CPU-only probe candidate");
        assert!(device_run.is_none());
        assert_eq!(cpu_only_run.tiles.len(), 2);
        let cpu_only_pixels: Vec<_> = cpu_only_run
            .tiles
            .into_iter()
            .map(|(frame, _)| decode_rgb8_frame(frame.expect("CPU-only probe frame"), TILE_SIZE))
            .collect();
        assert_eq!(cpu_only_pixels, expected);
    }
}
