//! Source-aware default selection for export options.

use statumen::Slide;

use crate::error::WsiDicomError;
use crate::export::{
    jpeg_baseline_route_frame_geometry, pixel_profile_from_raw_jpeg_tile, plan_lossless_j2k_row,
    raw_jpeg_profile_can_passthrough, raw_rgb_passthrough_has_no_geometry_fallback,
    read_raw_jpeg_passthrough_tile, JpegBaselineFrameLocation,
};
use crate::options::{DicomExportOptions, TransferSyntax};
use crate::request::DefaultTransferSyntaxRequest;
use crate::routing::{j2k_route_tile_size, level_is_synthetic_downsample};
use crate::tile::optical_path_groups;

/// Pick the default transfer syntax for a source by preserving native compressed
/// frames when an eligible passthrough route is visible.
pub fn default_transfer_syntax_for_source(
    request: DefaultTransferSyntaxRequest,
) -> Result<TransferSyntax, WsiDicomError> {
    if request.tile_size == 0 {
        return Err(WsiDicomError::InvalidOptions {
            reason: "tile_size must be greater than zero".into(),
        });
    }
    if request.max_levels == Some(0) {
        return Err(WsiDicomError::InvalidOptions {
            reason: "max_levels must be greater than zero when provided".into(),
        });
    }
    let max_levels = request
        .max_levels
        .map(usize::try_from)
        .transpose()
        .map_err(|_| WsiDicomError::Unsupported {
            reason: "max_levels exceeds platform addressable memory".into(),
        })?;
    let slide = Slide::open(&request.source_path).map_err(|source| WsiDicomError::SourceOpen {
        path: request.source_path.clone(),
        message: source.to_string(),
    })?;
    let mut j2k_passthrough_available = false;

    for (scene_idx, scene) in slide.dataset().scenes.iter().enumerate() {
        for (series_idx, series) in scene.series.iter().enumerate() {
            let level_limit = max_levels
                .unwrap_or(series.levels.len())
                .min(series.levels.len());
            for (level_idx, level) in series.levels.iter().take(level_limit).enumerate() {
                let level_idx =
                    u32::try_from(level_idx).map_err(|_| WsiDicomError::Unsupported {
                        reason: "default transfer syntax level index exceeds u32".into(),
                    })?;
                if request
                    .level_filter
                    .is_some_and(|requested_level| requested_level != level_idx)
                {
                    continue;
                }
                if level_is_synthetic_downsample(&slide, scene_idx, series_idx, level_idx)? {
                    continue;
                }
                for z in 0..series.axes.z {
                    for t in 0..series.axes.t {
                        for c in optical_path_groups(series.axes.c) {
                            let location = JpegBaselineFrameLocation {
                                scene_idx,
                                series_idx,
                                level_idx,
                                z,
                                c,
                                t,
                            };
                            if jpeg_baseline_passthrough_available_for_default(
                                &slide,
                                level,
                                location,
                                request.tile_size,
                            )? {
                                return Ok(TransferSyntax::JpegBaseline8Bit);
                            }
                            if !j2k_passthrough_available
                                && j2k_passthrough_available_for_default(
                                    &slide,
                                    level,
                                    location,
                                    request.tile_size,
                                )?
                            {
                                j2k_passthrough_available = true;
                            }
                        }
                    }
                }
            }
        }
    }

    if j2k_passthrough_available {
        Ok(TransferSyntax::Jpeg2000)
    } else {
        Ok(TransferSyntax::Htj2kLosslessRpcl)
    }
}

fn jpeg_baseline_passthrough_available_for_default(
    slide: &Slide,
    level: &statumen::Level,
    location: JpegBaselineFrameLocation,
    fallback_tile_size: u32,
) -> Result<bool, WsiDicomError> {
    let geometry =
        match jpeg_baseline_route_frame_geometry(slide, level, location, fallback_tile_size) {
            Ok(geometry) => geometry,
            Err(err @ WsiDicomError::InvalidOptions { .. }) => return Err(err),
            Err(_) => return Ok(false),
        };
    let Some(raw) = read_raw_jpeg_passthrough_tile(slide, location, geometry, 0)? else {
        return Ok(false);
    };
    let Ok(profile) = pixel_profile_from_raw_jpeg_tile(&raw) else {
        return Ok(false);
    };
    Ok(raw_jpeg_profile_can_passthrough(
        profile,
        raw_rgb_passthrough_has_no_geometry_fallback(level, geometry),
    ))
}

fn j2k_passthrough_available_for_default(
    slide: &Slide,
    level: &statumen::Level,
    location: JpegBaselineFrameLocation,
    fallback_tile_size: u32,
) -> Result<bool, WsiDicomError> {
    let options = DicomExportOptions {
        tile_size: fallback_tile_size,
        transfer_syntax: TransferSyntax::Jpeg2000,
        ..DicomExportOptions::default()
    };
    let tile_size = j2k_route_tile_size(&options, level)?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    if matrix_columns == 0 || matrix_rows == 0 {
        return Ok(false);
    }
    let planned = plan_lossless_j2k_row(
        slide,
        location.scene_idx,
        location.series_idx,
        location.level_idx,
        location.z,
        location.c,
        location.t,
        0,
        0,
        1,
        matrix_columns,
        matrix_rows,
        tile_size,
        TransferSyntax::Jpeg2000,
        true,
    )?;
    Ok(planned.iter().any(|frame| frame.has_passthrough()))
}
