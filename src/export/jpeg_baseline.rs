use std::time::{Duration, Instant};

use j2k_jpeg::{EncodedJpeg, JpegBackend, JpegSamples, JpegSubsampling};
use wsi_rs::TileLayout;
use wsi_rs::{Compression, EncodedTilePhotometricInterpretation, RawCompressedTile, Slide};

use crate::error::Error;
use crate::tile::PixelProfile;

use super::frame_region::{FrameLocation, OutputFrameRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JpegBaselineFrameGeometry {
    pub(crate) frame_columns: u32,
    pub(crate) frame_rows: u32,
    pub(crate) tiles_across: u64,
    pub(crate) tiles_down: u64,
}

pub(crate) type JpegBaselineFrameLocation = FrameLocation;

pub(super) type JpegBaselineFallbackFrame = OutputFrameRect;

pub(super) enum JpegBaselinePlannedFrame {
    Passthrough {
        data: Vec<u8>,
        profile: PixelProfile,
        uncompressed_bytes: u64,
    },
    Retile {
        data: Vec<u8>,
        profile: PixelProfile,
        uncompressed_bytes: u64,
        retile_duration: Duration,
    },
    Blank {
        data: Vec<u8>,
        profile: PixelProfile,
        uncompressed_bytes: u64,
        encode_duration: Duration,
    },
    Fallback(JpegBaselineFallbackFrame),
}

pub(super) struct JpegBaselineMetalEncodedRun {
    pub(super) frames: Vec<Option<(EncodedJpeg, PixelProfile)>>,
    pub(super) input_decode_duration: Duration,
    pub(super) encode_duration: Duration,
    pub(super) input_decode_batches: u64,
    pub(super) encode_batches: u64,
}

pub(super) fn jpeg_baseline_frame_geometry(
    level: &wsi_rs::Level,
    fallback_tile_size: u32,
) -> Result<JpegBaselineFrameGeometry, Error> {
    if fallback_tile_size == 0 {
        return Err(Error::InvalidOptions {
            reason: "tile_size must be greater than zero".into(),
        });
    }
    let (matrix_columns, matrix_rows) = level.dimensions;
    let (frame_columns, frame_rows, tiles_across, tiles_down) = match level.tile_layout {
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } => {
            if virtual_tile_width == 0 || virtual_tile_height == 0 {
                return Err(Error::Unsupported {
                    reason:
                        "JPEG Baseline WholeLevel export requires nonzero virtual tile geometry"
                            .into(),
                });
            }
            if native_jpeg_frame_geometry_is_viewer_friendly(
                virtual_tile_width,
                virtual_tile_height,
                fallback_tile_size,
            ) {
                (
                    virtual_tile_width,
                    virtual_tile_height,
                    matrix_columns.div_ceil(u64::from(virtual_tile_width)),
                    matrix_rows.div_ceil(u64::from(virtual_tile_height)),
                )
            } else {
                (
                    fallback_tile_size,
                    fallback_tile_size,
                    matrix_columns.div_ceil(u64::from(fallback_tile_size)),
                    matrix_rows.div_ceil(u64::from(fallback_tile_size)),
                )
            }
        }
        TileLayout::Regular {
            tile_width,
            tile_height,
            tiles_across,
            tiles_down,
        } if tile_width == fallback_tile_size && tile_height == fallback_tile_size => (
            fallback_tile_size,
            fallback_tile_size,
            tiles_across,
            tiles_down,
        ),
        TileLayout::Regular { .. } | TileLayout::Irregular { .. } => (
            fallback_tile_size,
            fallback_tile_size,
            matrix_columns.div_ceil(u64::from(fallback_tile_size)),
            matrix_rows.div_ceil(u64::from(fallback_tile_size)),
        ),
    };
    if frame_columns == 0 || frame_rows == 0 {
        return Err(Error::Unsupported {
            reason: "JPEG Baseline frame geometry must be nonzero".into(),
        });
    }
    if frame_columns > u16::MAX as u32 || frame_rows > u16::MAX as u32 {
        return Err(Error::Unsupported {
            reason: format!(
                "DICOM Rows/Columns require u16 frame geometry, got {frame_columns}x{frame_rows}"
            ),
        });
    }
    Ok(JpegBaselineFrameGeometry {
        frame_columns,
        frame_rows,
        tiles_across,
        tiles_down,
    })
}

pub(crate) fn jpeg_baseline_route_frame_geometry(
    slide: &Slide,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    fallback_tile_size: u32,
) -> Result<JpegBaselineFrameGeometry, Error> {
    if let Some(geometry) = jpeg_baseline_native_regular_passthrough_geometry(
        slide,
        level,
        location,
        fallback_tile_size,
    )? {
        return Ok(geometry);
    }
    jpeg_baseline_frame_geometry(level, fallback_tile_size)
}

fn jpeg_baseline_native_regular_passthrough_geometry(
    slide: &Slide,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    fallback_tile_size: u32,
) -> Result<Option<JpegBaselineFrameGeometry>, Error> {
    let TileLayout::Regular {
        tile_width,
        tile_height,
        tiles_across,
        tiles_down,
    } = level.tile_layout
    else {
        return Ok(None);
    };
    if tile_width == 0 || tile_height == 0 {
        return Err(Error::Unsupported {
            reason: "JPEG Baseline Regular export requires nonzero tile geometry".into(),
        });
    }
    if !native_jpeg_frame_geometry_is_viewer_friendly(tile_width, tile_height, fallback_tile_size) {
        return Ok(None);
    }

    let geometry = JpegBaselineFrameGeometry {
        frame_columns: tile_width,
        frame_rows: tile_height,
        tiles_across,
        tiles_down,
    };
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    for (col, row) in native_regular_probe_tile_coords(tiles_across, tiles_down) {
        let raw = match slide.read_raw_compressed_tile(&location.tile_request(col, row)) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        if !raw_jpeg_matches_frame_geometry(&raw, tile_width, tile_height) {
            continue;
        }
        let Ok(profile) = pixel_profile_from_raw_jpeg_tile(&raw) else {
            continue;
        };
        if raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
            return Ok(Some(geometry));
        }
    }

    Ok(None)
}

fn native_regular_probe_tile_coords(tiles_across: u64, tiles_down: u64) -> Vec<(i64, i64)> {
    fn push_unique(coords: &mut Vec<(i64, i64)>, col: u64, row: u64) {
        let (Ok(col), Ok(row)) = (i64::try_from(col), i64::try_from(row)) else {
            return;
        };
        if !coords.contains(&(col, row)) {
            coords.push((col, row));
        }
    }

    if tiles_across == 0 || tiles_down == 0 {
        return Vec::new();
    }

    let mut coords = Vec::new();
    let last_col = tiles_across - 1;
    let last_row = tiles_down - 1;
    let mid_col = tiles_across / 2;
    let mid_row = tiles_down / 2;
    for (col, row) in [
        (0, 0),
        (mid_col, mid_row),
        (last_col, last_row),
        (0, mid_row),
        (mid_col, 0),
        (last_col, mid_row),
        (mid_col, last_row),
        (last_col, 0),
        (0, last_row),
    ] {
        push_unique(&mut coords, col, row);
    }

    let grid_steps = 8_u64;
    for row_step in 0..grid_steps {
        let row = last_row.saturating_mul(row_step) / (grid_steps - 1);
        for col_step in 0..grid_steps {
            let col = last_col.saturating_mul(col_step) / (grid_steps - 1);
            push_unique(&mut coords, col, row);
        }
    }

    for idx in 0..tiles_across.saturating_mul(tiles_down).min(4096) {
        push_unique(&mut coords, idx % tiles_across, idx / tiles_across);
    }

    coords
}

fn native_jpeg_frame_geometry_is_viewer_friendly(
    frame_columns: u32,
    frame_rows: u32,
    fallback_tile_size: u32,
) -> bool {
    if frame_columns == 0 || frame_rows == 0 || fallback_tile_size == 0 {
        return false;
    }
    frame_columns.max(frame_rows) <= fallback_tile_size
        && frame_columns.min(frame_rows) >= fallback_tile_size.div_ceil(2)
}

pub(crate) fn pixel_profile_from_raw_jpeg_tile(
    raw: &RawCompressedTile,
) -> Result<PixelProfile, Error> {
    if raw.compression != Compression::Jpeg {
        return Err(Error::Unsupported {
            reason: format!(
                "JPEG passthrough requires JPEG compression, got {:?}",
                raw.compression
            ),
        });
    }
    if raw.bits_allocated != 8 {
        return Err(Error::UnsupportedPixelData {
            reason: format!(
                "JPEG passthrough requires 8-bit samples, got {}",
                raw.bits_allocated
            ),
        });
    }
    let photometric_interpretation = match raw.photometric_interpretation {
        EncodedTilePhotometricInterpretation::Monochrome2 => "MONOCHROME2",
        EncodedTilePhotometricInterpretation::Rgb => "RGB",
        EncodedTilePhotometricInterpretation::YbrFull422 => "YBR_FULL_422",
    };
    let components =
        u8::try_from(raw.samples_per_pixel).map_err(|_| Error::UnsupportedPixelData {
            reason: format!(
                "JPEG passthrough component count exceeds u8: {}",
                raw.samples_per_pixel
            ),
        })?;
    Ok(PixelProfile {
        components,
        bits_allocated: raw.bits_allocated,
        photometric_interpretation,
    })
}

pub(crate) fn raw_jpeg_profile_can_passthrough(
    profile: PixelProfile,
    allow_raw_rgb_passthrough: bool,
) -> bool {
    profile.photometric_interpretation != "RGB" || allow_raw_rgb_passthrough
}

pub(crate) fn raw_jpeg_matches_frame_geometry(
    raw: &RawCompressedTile,
    frame_columns: u32,
    frame_rows: u32,
) -> bool {
    raw.compression == Compression::Jpeg && raw.width == frame_columns && raw.height == frame_rows
}

const STATUMEN_EMPTY_TIFF_TILE_REASON: &str = "empty TIFF tiles";

pub(super) fn raw_compressed_error_is_empty_tile(err: &wsi_rs::WsiError) -> bool {
    matches!(
        err,
        wsi_rs::WsiError::Unsupported { reason }
            if reason.contains(STATUMEN_EMPTY_TIFF_TILE_REASON)
    )
}

pub(super) fn blank_jpeg_baseline_frame(
    frame_columns: u32,
    frame_rows: u32,
    jpeg_quality: u8,
    cache: &mut Option<(Vec<u8>, Duration)>,
) -> Result<JpegBaselinePlannedFrame, Error> {
    let profile = PixelProfile {
        components: 3,
        bits_allocated: 8,
        photometric_interpretation: "YBR_FULL_422",
    };
    let uncompressed_bytes =
        jpeg_baseline_fallback_uncompressed_bytes(frame_columns, frame_rows, profile)?;
    let (data, encode_duration) = if let Some((data, _)) = cache {
        (data.clone(), Duration::ZERO)
    } else {
        let pixels_len = usize::try_from(uncompressed_bytes).map_err(|_| Error::Unsupported {
            reason: "blank JPEG Baseline frame byte count exceeds addressable memory".into(),
        })?;
        let pixels = vec![255u8; pixels_len];
        let encode_started = Instant::now();
        let encoded = encode_jpeg_baseline_cpu_fragment(
            JpegSamples::Rgb8 {
                data: &pixels,
                width: frame_columns,
                height: frame_rows,
            },
            jpeg_quality,
            JpegSubsampling::Ybr422,
            jpeg_baseline_cpu_restart_interval(frame_columns, frame_rows, JpegSubsampling::Ybr422),
        )?;
        let duration = encode_started.elapsed();
        let data = encoded.data;
        *cache = Some((data.clone(), duration));
        (data, duration)
    };

    Ok(JpegBaselinePlannedFrame::Blank {
        data,
        profile,
        uncompressed_bytes,
        encode_duration,
    })
}

pub(crate) fn raw_rgb_passthrough_has_no_geometry_fallback(
    level: &wsi_rs::Level,
    geometry: JpegBaselineFrameGeometry,
) -> bool {
    let full_frame_grid = level
        .dimensions
        .0
        .is_multiple_of(u64::from(geometry.frame_columns))
        && level
            .dimensions
            .1
            .is_multiple_of(u64::from(geometry.frame_rows));
    match level.tile_layout {
        TileLayout::Regular {
            tile_width,
            tile_height,
            ..
        } => {
            tile_width == geometry.frame_columns
                && tile_height == geometry.frame_rows
                && full_frame_grid
        }
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } => {
            virtual_tile_width == geometry.frame_columns
                && virtual_tile_height == geometry.frame_rows
        }
        TileLayout::Irregular { .. } => false,
    }
}

pub(super) fn uncompressed_frame_bytes(raw: &RawCompressedTile) -> Result<u64, Error> {
    checked_uncompressed_byte_count(
        u64::from(raw.width),
        u64::from(raw.height),
        u64::from(raw.samples_per_pixel),
        raw.bits_allocated,
    )
    .ok_or_else(|| Error::Unsupported {
        reason: "JPEG passthrough uncompressed frame byte count overflow".into(),
    })
}

pub(super) fn jpeg_baseline_fallback_uncompressed_bytes(
    frame_columns: u32,
    frame_rows: u32,
    profile: PixelProfile,
) -> Result<u64, Error> {
    checked_uncompressed_byte_count(
        u64::from(frame_columns),
        u64::from(frame_rows),
        u64::from(profile.components),
        profile.bits_allocated,
    )
    .ok_or_else(|| Error::Unsupported {
        reason: "JPEG Baseline uncompressed frame byte count overflow".into(),
    })
}

fn checked_uncompressed_byte_count(
    width: u64,
    height: u64,
    samples_per_pixel: u64,
    bits_allocated: u16,
) -> Option<u64> {
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(samples_per_pixel))
        .and_then(|samples| samples.checked_mul(u64::from(bits_allocated / 8)))
}

pub(super) fn encode_jpeg_baseline_cpu_fragment(
    samples: JpegSamples<'_>,
    jpeg_quality: u8,
    subsampling: JpegSubsampling,
    restart_interval: Option<u16>,
) -> Result<EncodedJpeg, Error> {
    j2k_jpeg::encode_jpeg_baseline(
        samples,
        j2k_jpeg::JpegEncodeOptions {
            quality: jpeg_quality,
            subsampling,
            restart_interval,
            backend: JpegBackend::Cpu,
        },
    )
    .map_err(|source| Error::Encode {
        message: source.to_string(),
    })
}

pub(super) fn jpeg_baseline_cpu_restart_interval(
    frame_columns: u32,
    frame_rows: u32,
    subsampling: JpegSubsampling,
) -> Option<u16> {
    let (mcu_width, mcu_height) = match subsampling {
        JpegSubsampling::Gray | JpegSubsampling::Ybr444 => (8, 8),
        JpegSubsampling::Ybr422 => (16, 8),
        JpegSubsampling::Ybr420 => (16, 16),
    };
    let mcu_count = frame_columns
        .div_ceil(mcu_width)
        .saturating_mul(frame_rows.div_ceil(mcu_height));
    (mcu_count > 64).then_some(64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tile_detection_is_limited_to_wsi_rs_unsupported_reason() {
        assert!(raw_compressed_error_is_empty_tile(
            &wsi_rs::WsiError::Unsupported {
                reason: "JPEG passthrough does not support empty TIFF tiles".into(),
            }
        ));
        assert!(!raw_compressed_error_is_empty_tile(
            &wsi_rs::WsiError::TileRead {
                col: 0,
                row: 0,
                level: 0,
                reason: "empty TIFF tiles".into(),
            }
        ));
    }
}
