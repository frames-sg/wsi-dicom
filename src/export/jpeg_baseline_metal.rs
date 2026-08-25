use std::time::{Duration, Instant};

use wsi_rs::{DeviceTile, PlaneSelection, TilePixels, TileRequest};

use super::jpeg_baseline::{
    encode_jpeg_baseline_metal_device_tile_batch, JpegBaselineFallbackFrame,
    JpegBaselineMetalEncodedRun,
};
use super::jpeg_baseline_pipeline::{
    JpegBaselineCpuEncodeSettings, JpegBaselineFallbackBatchRequest,
};
use super::metal_input::MetalInputTileReader;
use super::metal_route::output_frame_maps_to_wsi_rs_tile;
use crate::error::Error;
use crate::options::EncodeBackendPreference;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn jpeg_baseline_auto_allows_metal_batch(
    preference: EncodeBackendPreference,
    frame_columns: u32,
    frame_rows: u32,
    frame_count: usize,
    source_device_decode: bool,
) -> bool {
    match preference {
        EncodeBackendPreference::CpuOnly => false,
        EncodeBackendPreference::PreferDevice | EncodeBackendPreference::RequireDevice => {
            frame_count > 0
        }
        EncodeBackendPreference::Auto => {
            source_device_decode && frame_columns == frame_rows && frame_count >= 4
        }
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn try_encode_jpeg_baseline_metal_input_tile_run(
    request: JpegBaselineFallbackBatchRequest<'_>,
    metal_input: &mut MetalInputTileReader,
) -> Result<JpegBaselineMetalEncodedRun, Error> {
    let JpegBaselineFallbackBatchRequest {
        slide,
        level,
        location,
        row,
        frames,
        settings,
        ..
    } = request;
    let JpegBaselineCpuEncodeSettings {
        frame_columns,
        frame_rows,
        jpeg_quality,
        max_prepared_frame_bytes,
    } = settings;
    objc2::rc::autoreleasepool(|_| {
        if !jpeg_baseline_auto_allows_metal_batch(
            metal_input.preference,
            frame_columns,
            frame_rows,
            frames.len(),
            metal_input.source_device_decode,
        ) {
            return Ok(empty_jpeg_baseline_metal_run(frames.len()));
        }
        if !output_frame_maps_to_wsi_rs_tile(level, frame_columns, frame_rows) {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason:
                        "requested JPEG Baseline Metal fallback requires the DICOM frame grid to align with wsi-rs source tiles"
                            .into(),
                });
            }
            return Ok(empty_jpeg_baseline_metal_run(frames.len()));
        }

        let row_i64 = i64::try_from(row).map_err(|_| Error::Unsupported {
            reason: "JPEG Baseline Metal tile row exceeds i64".into(),
        })?;
        let mut requests = Vec::new();
        requests
            .try_reserve_exact(frames.len())
            .map_err(|_| Error::Unsupported {
                reason: "JPEG Baseline Metal request batch exceeds available memory".into(),
            })?;
        for frame in frames {
            requests.push(
                TileRequest::new(
                    location.scene_idx,
                    location.series_idx,
                    location.level_idx,
                    i64::try_from(frame.x / u64::from(frame_columns)).map_err(|_| {
                        Error::Unsupported {
                            reason: "JPEG Baseline Metal tile column exceeds i64".into(),
                        }
                    })?,
                    row_i64,
                )
                .with_plane(PlaneSelection::new(location.z, location.c, location.t)),
            );
        }
        let output = match metal_input.source_tile_output_preference() {
            Ok(output) => output,
            Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(err);
            }
            Err(_) => return Ok(empty_jpeg_baseline_metal_run(frames.len())),
        };

        let input_decode_started = Instant::now();
        let pixels = match slide.read_tiles(&requests, output) {
            Ok(pixels) if pixels.len() == frames.len() => pixels,
            Ok(pixels) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(Error::SlideRead {
                    message: format!(
                        "JPEG Baseline Metal input decode returned {} tile(s), expected {}",
                        pixels.len(),
                        frames.len()
                    ),
                });
            }
            Ok(_) => {
                return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                    frames.len(),
                    input_decode_started.elapsed(),
                ));
            }
            Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(Error::SlideRead {
                    message: format!("JPEG Baseline Metal input decode failed: {err}"),
                });
            }
            Err(_) => {
                return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                    frames.len(),
                    input_decode_started.elapsed(),
                ));
            }
        };
        let input_decode_duration = input_decode_started.elapsed();

        let tile_entries =
            jpeg_baseline_metal_tile_entries(pixels, frames, metal_input.preference)?;
        let batch_tiles: Vec<_> = tile_entries
            .iter()
            .filter_map(|entry| entry.as_ref().cloned())
            .collect();
        if batch_tiles.is_empty() {
            return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                frames.len(),
                input_decode_duration,
            ));
        }
        let bytes_per_pixel =
            u64::try_from(batch_tiles[0].format.bytes_per_pixel()).map_err(|_| {
                Error::UnsupportedPixelData {
                    reason: "JPEG Baseline Metal bytes-per-pixel exceeds u64".into(),
                }
            })?;
        let prepared_frame_bytes = u64::from(frame_columns)
            .checked_mul(u64::from(frame_rows))
            .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
            .ok_or_else(|| Error::UnsupportedPixelData {
                reason: "JPEG Baseline Metal prepared frame byte length overflow".into(),
            })?;
        if prepared_frame_bytes > max_prepared_frame_bytes {
            return Err(Error::UnsupportedPixelData {
                reason: format!(
                    "prepared tile buffer requires {prepared_frame_bytes} bytes, exceeding configured limit {max_prepared_frame_bytes}"
                ),
            });
        }

        let encode_started = Instant::now();
        let encoded = match encode_jpeg_baseline_metal_device_tile_batch(
            &batch_tiles,
            frame_columns,
            frame_rows,
            jpeg_quality,
            metal_input.jpeg_encode_session()?,
        ) {
            Ok(encoded) => encoded,
            Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(err);
            }
            Err(_) => return Ok(empty_jpeg_baseline_metal_run(frames.len())),
        };
        if encoded.len() != batch_tiles.len() {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Encode {
                    message: format!(
                        "JPEG Baseline Metal encode returned {} frame(s), expected {}",
                        encoded.len(),
                        batch_tiles.len()
                    ),
                });
            }
            return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                frames.len(),
                input_decode_duration,
            ));
        }
        let encode_duration = encode_started.elapsed();
        let mut encoded = encoded.into_iter();
        let mut output_frames = Vec::new();
        output_frames
            .try_reserve_exact(frames.len())
            .map_err(|_| Error::Unsupported {
                reason: "JPEG Baseline Metal output batch exceeds available memory".into(),
            })?;
        for entry in tile_entries {
            if entry.is_some() {
                output_frames.push(Some(encoded.next().ok_or_else(|| {
                    Error::Encode {
                        message:
                            "JPEG Baseline Metal encoded frame count did not match input tile count"
                                .into(),
                    }
                })?));
            } else {
                output_frames.push(None);
            }
        }
        Ok(JpegBaselineMetalEncodedRun {
            frames: output_frames,
            input_decode_duration,
            encode_duration,
            input_decode_batches: 1,
            encode_batches: 1,
        })
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn jpeg_baseline_metal_tile_entries(
    pixels: Vec<TilePixels>,
    frames: &[JpegBaselineFallbackFrame],
    preference: EncodeBackendPreference,
) -> Result<Vec<Option<wsi_rs::output::metal::MetalDeviceTile>>, Error> {
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(frames.len())
        .map_err(|_| Error::Unsupported {
            reason: "JPEG Baseline Metal tile batch exceeds available memory".into(),
        })?;
    for (pixels, frame) in pixels.into_iter().zip(frames.iter()) {
        let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
            if preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason:
                        "requested JPEG Baseline Metal input decode returned CPU pixels; set WSI_RS_JPEG_DEVICE_DECODE=1 or WSI_RS_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                            .into(),
                });
            }
            entries.push(None);
            continue;
        };
        if tile.width != frame.width || tile.height != frame.height {
            if preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason: format!(
                        "JPEG Baseline Metal input geometry changed: expected {}x{}, got {}x{}",
                        frame.width, frame.height, tile.width, tile.height
                    ),
                });
            }
            entries.push(None);
            continue;
        }
        entries.push(Some(tile));
    }
    Ok(entries)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn empty_jpeg_baseline_metal_run(tile_count: usize) -> JpegBaselineMetalEncodedRun {
    empty_jpeg_baseline_metal_run_with_input_duration(tile_count, Duration::ZERO)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn empty_jpeg_baseline_metal_run_with_input_duration(
    tile_count: usize,
    input_decode_duration: Duration,
) -> JpegBaselineMetalEncodedRun {
    JpegBaselineMetalEncodedRun {
        frames: (0..tile_count).map(|_| None).collect(),
        input_decode_duration,
        encode_duration: Duration::ZERO,
        input_decode_batches: u64::from(input_decode_duration > Duration::ZERO),
        encode_batches: 0,
    }
}
