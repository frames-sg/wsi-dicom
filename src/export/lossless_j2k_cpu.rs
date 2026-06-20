use std::time::{Duration, Instant};

use j2k::{J2kLosslessSamples, ReversibleTransform};
use rayon::prelude::*;
use wsi_rs::{PlaneSelection, Slide, TileLayout, TileOutputPreference, TilePixels, TileRequest};

use crate::encode::{self, EncodedDicomJ2kFrame};
use crate::error::Error;
use crate::options::{CodecValidation, TransferSyntax};
use crate::tile::{prepare_tile_samples_with_limit, PixelProfile};

use super::{
    frame_region::{OutputFrameRect, PreparedCpuRegion},
    read_and_prepare_region, JpegBaselineFrameLocation, LosslessJ2kPlannedFrame,
};

const CPU_INPUT_BATCH_PARALLEL_MEMORY_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(super) struct LosslessJ2kCpuBatchSettings {
    pub(super) transfer_syntax: TransferSyntax,
    pub(super) codec_validation: CodecValidation,
    pub(super) j2k_decomposition_levels: Option<u8>,
    pub(super) reversible_transform: ReversibleTransform,
    pub(super) max_prepared_frame_bytes: u64,
}

pub(super) type LosslessJ2kCpuBatchFrame = OutputFrameRect;

#[derive(Clone, Copy, Debug)]
struct SourceTileBatchLocation {
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
}

pub(super) struct LosslessJ2kCpuBatchOutcome {
    pub(super) encoded: Result<EncodedDicomJ2kFrame, Error>,
    pub(super) profile: PixelProfile,
    pub(super) input_decode_duration: Duration,
    pub(super) compose_duration: Duration,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn encode_cpu_input_lossless_j2k_tile_batch(
    slide: &Slide,
    level: &wsi_rs::Level,
    settings: LosslessJ2kCpuBatchSettings,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    frames: &[LosslessJ2kCpuBatchFrame],
    tile_size: u32,
) -> Result<Vec<LosslessJ2kCpuBatchOutcome>, Error> {
    let prepared = if let Some(requests) = native_lossless_j2k_cpu_tile_requests(
        level,
        SourceTileBatchLocation {
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
        },
        frames,
        tile_size,
    ) {
        prepare_native_cpu_input_lossless_j2k_tile_batch(
            slide,
            &requests,
            tile_size,
            settings.max_prepared_frame_bytes,
        )?
    } else {
        prepare_region_cpu_input_lossless_j2k_tile_batch(
            slide,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            frames,
            tile_size,
            settings.max_prepared_frame_bytes,
        )?
    };
    encode_prepared_lossless_j2k_cpu_batch(settings, prepared, tile_size)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn encode_cpu_input_lossless_j2k_planned_batch(
    slide: &Slide,
    level: &wsi_rs::Level,
    settings: LosslessJ2kCpuBatchSettings,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    planned: &[LosslessJ2kPlannedFrame],
    indices: &[usize],
    tile_size: u32,
) -> Result<Vec<(usize, LosslessJ2kCpuBatchOutcome)>, Error> {
    let frames = indices
        .iter()
        .map(|&idx| {
            let planned = &planned[idx];
            planned.rect()
        })
        .collect::<Vec<_>>();
    let outcomes = encode_cpu_input_lossless_j2k_tile_batch(
        slide, level, settings, scene_idx, series_idx, level_idx, z, c, t, &frames, tile_size,
    )?;
    Ok(indices.iter().copied().zip(outcomes).collect())
}

fn native_lossless_j2k_cpu_tile_requests(
    level: &wsi_rs::Level,
    location: SourceTileBatchLocation,
    frames: &[LosslessJ2kCpuBatchFrame],
    tile_size: u32,
) -> Option<Vec<TileRequest>> {
    if frames.is_empty() {
        return Some(Vec::new());
    }
    let (tile_width, tile_height, tiles_across, tiles_down) = match &level.tile_layout {
        TileLayout::Regular {
            tile_width,
            tile_height,
            tiles_across,
            tiles_down,
        } => (*tile_width, *tile_height, *tiles_across, *tiles_down),
        TileLayout::WholeLevel { .. } | TileLayout::Irregular { .. } => return None,
    };
    if tile_width == 0 || tile_height == 0 || tile_width != tile_size || tile_height != tile_size {
        return None;
    }
    let tile_size_u64 = u64::from(tile_size);
    let plane = PlaneSelection {
        z: location.z,
        c: location.c,
        t: location.t,
    };
    frames
        .iter()
        .map(|frame| {
            if frame.width != tile_size
                || frame.height != tile_size
                || frame.x % tile_size_u64 != 0
                || frame.y % tile_size_u64 != 0
            {
                return None;
            }
            let col = frame.x / tile_size_u64;
            let row = frame.y / tile_size_u64;
            if col >= tiles_across || row >= tiles_down {
                return None;
            }
            Some(TileRequest {
                scene: location.scene_idx,
                series: location.series_idx,
                level: location.level_idx,
                plane,
                col: i64::try_from(col).ok()?,
                row: i64::try_from(row).ok()?,
            })
        })
        .collect()
}

fn prepare_native_cpu_input_lossless_j2k_tile_batch(
    slide: &Slide,
    requests: &[TileRequest],
    tile_size: u32,
    max_prepared_frame_bytes: u64,
) -> Result<Vec<PreparedCpuRegion>, Error> {
    let input_decode_started = Instant::now();
    let tiles = slide
        .read_tiles(requests, TileOutputPreference::cpu())
        .map_err(|source| Error::SlideRead {
            message: source.to_string(),
        })?;
    let input_decode_duration = input_decode_started.elapsed();
    if tiles.len() != requests.len() {
        return Err(Error::SlideRead {
            message: format!(
                "wsi-rs read_tiles returned {} tile(s), expected {}",
                tiles.len(),
                requests.len()
            ),
        });
    }

    tiles
        .into_iter()
        .enumerate()
        .map(|(idx, tile)| {
            let TilePixels::Cpu(tile) = tile else {
                return Err(Error::SlideRead {
                    message: "CPU tile batch returned device-resident tile".into(),
                });
            };
            let compose_started = Instant::now();
            let max_prepared_frame_bytes =
                usize::try_from(max_prepared_frame_bytes).map_err(|_| {
                    Error::UnsupportedPixelData {
                        reason: "max_prepared_frame_bytes exceeds platform addressable memory"
                            .into(),
                    }
                })?;
            let prepared = prepare_tile_samples_with_limit(
                &tile,
                tile_size,
                tile_size,
                max_prepared_frame_bytes,
            )?;
            Ok(PreparedCpuRegion {
                bytes: prepared.bytes,
                profile: prepared.profile,
                input_decode_duration: if idx == 0 {
                    input_decode_duration
                } else {
                    Duration::ZERO
                },
                compose_duration: compose_started.elapsed(),
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn prepare_region_cpu_input_lossless_j2k_tile_batch(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    frames: &[LosslessJ2kCpuBatchFrame],
    tile_size: u32,
    max_prepared_frame_bytes: u64,
) -> Result<Vec<PreparedCpuRegion>, Error> {
    frames
        .par_iter()
        .map(|frame| {
            prepare_cpu_input_lossless_j2k_tile(
                slide,
                scene_idx,
                series_idx,
                level_idx,
                z,
                c,
                t,
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                tile_size,
                max_prepared_frame_bytes,
            )
        })
        .collect()
}

fn encode_prepared_lossless_j2k_cpu_batch(
    settings: LosslessJ2kCpuBatchSettings,
    prepared: Vec<PreparedCpuRegion>,
    tile_size: u32,
) -> Result<Vec<LosslessJ2kCpuBatchOutcome>, Error> {
    let max_bytes_per_pixel = prepared
        .iter()
        .map(|tile| u64::from(tile.profile.components) * u64::from(tile.profile.bits_allocated / 8))
        .max()
        .unwrap_or(1);
    let workers = cpu_input_batch_worker_count(
        prepared.len(),
        tile_size,
        max_bytes_per_pixel,
        rayon::current_num_threads(),
    );
    if workers <= 1 {
        return prepared
            .into_iter()
            .map(|tile| encode_prepared_lossless_j2k_cpu_tile(settings, tile, tile_size))
            .collect();
    }
    prepared
        .into_par_iter()
        .map(|tile| encode_prepared_lossless_j2k_cpu_tile(settings, tile, tile_size))
        .collect()
}

fn encode_prepared_lossless_j2k_cpu_tile(
    settings: LosslessJ2kCpuBatchSettings,
    tile: PreparedCpuRegion,
    tile_size: u32,
) -> Result<LosslessJ2kCpuBatchOutcome, Error> {
    let samples = lossless_j2k_samples_from_prepared_region(&tile, tile_size)?;
    Ok(LosslessJ2kCpuBatchOutcome {
        encoded: encode::encode_lossless_cpu(
            samples,
            settings.transfer_syntax,
            settings.codec_validation,
            settings.j2k_decomposition_levels,
            settings.reversible_transform,
        ),
        profile: tile.profile,
        input_decode_duration: tile.input_decode_duration,
        compose_duration: tile.compose_duration,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_cpu_input_lossless_j2k_tile(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    x: u64,
    y: u64,
    width: u32,
    height: u32,
    tile_size: u32,
    max_prepared_frame_bytes: u64,
) -> Result<PreparedCpuRegion, Error> {
    read_and_prepare_region(
        slide,
        JpegBaselineFrameLocation {
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
        },
        x,
        y,
        width,
        height,
        tile_size,
        tile_size,
        max_prepared_frame_bytes,
    )
}

pub(super) fn lossless_j2k_samples_from_prepared_region<'a>(
    prepared: &'a PreparedCpuRegion,
    tile_size: u32,
) -> Result<J2kLosslessSamples<'a>, Error> {
    J2kLosslessSamples::new(
        &prepared.bytes,
        tile_size,
        tile_size,
        prepared.profile.components,
        prepared.profile.bits_allocated as u8,
        false,
    )
    .map_err(|source| Error::Encode {
        message: source.to_string(),
    })
}

fn cpu_input_batch_worker_count(
    frame_count: usize,
    tile_size: u32,
    bytes_per_pixel: u64,
    rayon_threads: usize,
) -> usize {
    if frame_count <= 1 || rayon_threads <= 1 {
        return 1;
    }
    let tile_bytes = u64::from(tile_size)
        .saturating_mul(u64::from(tile_size))
        .saturating_mul(bytes_per_pixel.max(1));
    let memory_limited_workers = CPU_INPUT_BATCH_PARALLEL_MEMORY_BYTES
        .checked_div(tile_bytes)
        .unwrap_or(1)
        .max(1);
    let thread_limited_workers = rayon_threads.saturating_sub(1).max(1);
    frame_count
        .min(thread_limited_workers)
        .min(usize::try_from(memory_limited_workers).unwrap_or(usize::MAX))
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cpu_tile_batch_requests_require_exact_source_tile_geometry() {
        let level = wsi_rs::Level {
            dimensions: (2048, 1024),
            downsample: 1.0,
            tile_layout: TileLayout::Regular {
                tile_width: 512,
                tile_height: 512,
                tiles_across: 4,
                tiles_down: 2,
            },
        };
        let frames = [
            LosslessJ2kCpuBatchFrame {
                x: 0,
                y: 0,
                width: 512,
                height: 512,
            },
            LosslessJ2kCpuBatchFrame {
                x: 512,
                y: 512,
                width: 512,
                height: 512,
            },
        ];
        let location = SourceTileBatchLocation {
            scene_idx: 1,
            series_idx: 2,
            level_idx: 3,
            z: 4,
            c: 5,
            t: 6,
        };

        let requests = native_lossless_j2k_cpu_tile_requests(&level, location, &frames, 512)
            .expect("exact regular source tiles should use wsi_rs batch reads");

        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].scene, 1);
        assert_eq!(requests[0].series, 2);
        assert_eq!(requests[0].level, 3);
        assert_eq!(requests[0].plane, PlaneSelection { z: 4, c: 5, t: 6 });
        assert_eq!((requests[0].col, requests[0].row), (0, 0));
        assert_eq!((requests[1].col, requests[1].row), (1, 1));

        let edge = [LosslessJ2kCpuBatchFrame {
            x: 1536,
            y: 0,
            width: 128,
            height: 512,
        }];
        assert!(native_lossless_j2k_cpu_tile_requests(&level, location, &edge, 512).is_none());
    }

    #[test]
    fn cpu_input_batch_workers_are_bounded_by_host_and_estimated_memory() {
        assert_eq!(cpu_input_batch_worker_count(1, 512, 3, 8), 1);
        assert_eq!(cpu_input_batch_worker_count(256, 512, 3, 1), 1);
        assert_eq!(cpu_input_batch_worker_count(256, 512, 3, 8), 7);
        assert_eq!(cpu_input_batch_worker_count(1024, 4096, 3, 16), 2);
    }

    #[test]
    fn prepared_cpu_region_builds_lossless_j2k_samples_from_profile() {
        let prepared = PreparedCpuRegion {
            bytes: vec![0; 2 * 2 * 3],
            profile: PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "RGB",
            },
            input_decode_duration: Duration::ZERO,
            compose_duration: Duration::ZERO,
        };

        let samples = lossless_j2k_samples_from_prepared_region(&prepared, 2).unwrap();

        assert_eq!(samples.data.len(), 12);
        assert_eq!(samples.width, 2);
        assert_eq!(samples.height, 2);
        assert_eq!(samples.components, 3);
        assert_eq!(samples.bit_depth, 8);
        assert!(!samples.signed);
    }
}
