use std::collections::HashMap;
use std::time::Duration;

use j2k_core::CompressedTransferSyntax;
use rayon::prelude::*;
use wsi_rs::{Compression, Slide, TileLayout};

use super::frame_region::{FrameRectGrid, FrameRectOverflowReasons, OutputFrameRect};
use super::j2k_policy::{reject_lossy_j2k_lossless_fallback, RawJ2kInspection};
use super::jpeg_retile::{read_raw_jpeg_retile_display_tile, RawJpegRetileProbe};
use super::route_plan::{
    FrameRouteDecision, FrameRouteSource, RouteExecutionContext, RoutePlanner,
};
use super::{j2k_direct_htj2k, jpeg_direct_htj2k};
use crate::coordinate::InstanceCoordinate;
use crate::error::Error;
use crate::lossy::{
    LossyCompressionByteCounts, HTJ2K_METHOD, JPEG_2000_METHOD, JPEG_BASELINE_METHOD,
};
use crate::options::TransferSyntax;
use crate::report::JpegRetileRejectionReason;
use crate::tile::PixelProfile;

pub(crate) struct LosslessJ2kPlannedFrame {
    pub(super) row: u64,
    pub(super) col: u64,
    pub(super) x: u64,
    pub(super) y: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) source_j2k_syntax: Option<CompressedTransferSyntax>,
    pub(super) source_j2k: Option<j2k_direct_htj2k::Frame>,
    pub(super) source_jpeg: Option<jpeg_direct_htj2k::Frame>,
    pub(super) source_jpeg_retiled: bool,
    pub(super) source_jpeg_retile_duration: Duration,
    pub(super) source_jpeg_retile_rejection: Option<JpegRetileRejectionReason>,
    pub(super) source_jpeg_direct_rejected: bool,
    pub(super) source_lossy_compression: Option<LossyCompressionByteCounts>,
    pub(super) passthrough: Option<J2kPassthroughFrame>,
}

impl LosslessJ2kPlannedFrame {
    pub(super) fn rect(&self) -> OutputFrameRect {
        OutputFrameRect::new(self.x, self.y, self.width, self.height)
    }

    pub(crate) fn has_passthrough(&self) -> bool {
        self.passthrough.is_some()
    }

    pub(crate) fn has_j2k_source(&self) -> bool {
        self.source_j2k_syntax.is_some()
    }

    pub(super) fn route_decision(&self, context: RouteExecutionContext) -> FrameRouteDecision {
        RoutePlanner::new(context).decide(FrameRouteSource::J2k {
            passthrough: self.passthrough.is_some(),
            direct_j2k: self.source_j2k.is_some(),
            direct_jpeg: self.source_jpeg.is_some(),
        })
    }
}

#[derive(Clone)]
pub(super) struct J2kPassthroughFrame {
    pub(super) codestream: Vec<u8>,
    pub(super) profile: PixelProfile,
    #[cfg(test)]
    pub(super) transfer_syntax: CompressedTransferSyntax,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LosslessJ2kPlanRequest {
    pub(crate) location: InstanceCoordinate,
    pub(crate) start_row: u64,
    pub(crate) row_count: u64,
    pub(crate) start_col: u64,
    pub(crate) tile_count: u64,
    pub(crate) grid: FrameRectGrid,
    pub(crate) transfer_syntax: TransferSyntax,
    pub(crate) allow_passthrough_probe: bool,
}

#[derive(Clone, Copy)]
pub(super) enum WholeLevelJpegPreflight {
    Profile {
        virtual_tile_width: u32,
        virtual_tile_height: u32,
    },
}

#[derive(Debug, Clone, Copy, Default)]
struct WholeLevelJpegFramePreflight {
    profile_unsupported: bool,
}

impl WholeLevelJpegPreflight {
    pub(super) fn profile(level: &wsi_rs::Level, transfer_syntax: TransferSyntax) -> Option<Self> {
        if !jpeg_direct_htj2k::transfer_syntax(transfer_syntax) {
            return None;
        }
        match level.tile_layout {
            TileLayout::WholeLevel {
                virtual_tile_width,
                virtual_tile_height,
                ..
            } if virtual_tile_width != 0 && virtual_tile_height != 0 => Some(Self::Profile {
                virtual_tile_width,
                virtual_tile_height,
            }),
            _ => None,
        }
    }
}

pub(crate) fn plan_lossless_j2k_frames(
    slide: &Slide,
    request: LosslessJ2kPlanRequest,
) -> Result<Vec<LosslessJ2kPlannedFrame>, Error> {
    plan_lossless_j2k_frames_with_preflight(slide, request, None)
}

pub(super) fn plan_lossless_j2k_frames_with_preflight(
    slide: &Slide,
    request: LosslessJ2kPlanRequest,
    preflight: Option<WholeLevelJpegPreflight>,
) -> Result<Vec<LosslessJ2kPlannedFrame>, Error> {
    let rows = usize::try_from(request.row_count).map_err(|_| Error::Unsupported {
        reason: "J2K row planning row count exceeds platform addressable memory".into(),
    })?;
    let tiles = usize::try_from(request.tile_count).map_err(|_| Error::Unsupported {
        reason: "J2K row planning tile count exceeds platform addressable memory".into(),
    })?;
    let frame_capacity = rows.checked_mul(tiles).ok_or_else(|| Error::Unsupported {
        reason: "J2K frame plan count overflow".into(),
    })?;
    let mut positions = Vec::new();
    positions
        .try_reserve_exact(frame_capacity)
        .map_err(|_| Error::Unsupported {
            reason: "J2K frame plan exceeds available memory".into(),
        })?;
    for offset in 0..request.row_count {
        let row = request
            .start_row
            .checked_add(offset)
            .ok_or_else(|| Error::Unsupported {
                reason: "J2K row planning tile row overflow".into(),
            })?;
        for tile_offset in 0..tiles {
            let col = request
                .start_col
                .checked_add(u64::try_from(tile_offset).map_err(|_| Error::Unsupported {
                    reason: "J2K row planning tile offset exceeds u64".into(),
                })?)
                .ok_or_else(|| Error::Unsupported {
                    reason: "J2K row planning tile column overflow".into(),
                })?;
            positions.push((row, col));
        }
    }
    let jpeg_preflight = match preflight {
        Some(WholeLevelJpegPreflight::Profile {
            virtual_tile_width,
            virtual_tile_height,
        }) => whole_level_profile_preflight(
            slide,
            request,
            &positions,
            virtual_tile_width,
            virtual_tile_height,
        ),
        None => HashMap::new(),
    };
    // Each frame's raw read and lossless JPEG retile probe is independent and
    // can cost milliseconds (DCT extraction and entropy re-emission), so plan
    // frames on the shared pool. Indexed collection keeps row-major order.
    positions
        .into_par_iter()
        .map(|(row, col)| {
            let preflight = jpeg_preflight.get(&(row, col)).copied().unwrap_or_default();
            plan_lossless_j2k_frame_at(slide, request, row, col, preflight)
        })
        .collect()
}

fn whole_level_profile_preflight(
    slide: &Slide,
    request: LosslessJ2kPlanRequest,
    positions: &[(u64, u64)],
    virtual_tile_width: u32,
    virtual_tile_height: u32,
) -> HashMap<(u64, u64), WholeLevelJpegFramePreflight> {
    let origins_by_position = positions
        .iter()
        .filter_map(|&(row, col)| {
            whole_level_source_origin(request, row, col, virtual_tile_width, virtual_tile_height)
                .map(|origin| ((row, col), origin))
        })
        .collect::<Vec<_>>();
    let mut origins = origins_by_position
        .iter()
        .map(|&(_, origin)| origin)
        .collect::<Vec<_>>();
    origins.sort_unstable();
    origins.dedup();
    let profile_by_origin = origins
        .into_par_iter()
        .map(|origin @ (col, row)| {
            let unsupported = slide
                .read_raw_compressed_tile(&request.location.tile_request(col, row))
                .is_ok_and(|raw| jpeg_direct_htj2k::raw_profile_is_unsupported_for_htj2k(&raw));
            (origin, unsupported)
        })
        .collect::<HashMap<_, _>>();
    origins_by_position
        .into_iter()
        .filter_map(|(position, origin)| {
            // A successful wsi-rs retile preserves its first contributor's
            // JPEG profile and rejects mismatched contributors. A YBR first
            // contributor therefore cannot produce admissible direct HTJ2K.
            profile_by_origin
                .get(&origin)
                .copied()
                .unwrap_or(false)
                .then_some((
                    position,
                    WholeLevelJpegFramePreflight {
                        profile_unsupported: true,
                    },
                ))
        })
        .collect()
}

fn whole_level_source_origin(
    request: LosslessJ2kPlanRequest,
    row: u64,
    col: u64,
    virtual_tile_width: u32,
    virtual_tile_height: u32,
) -> Option<(i64, i64)> {
    let frame_x = col.checked_mul(u64::from(request.grid.frame_columns))?;
    let frame_y = row.checked_mul(u64::from(request.grid.frame_rows))?;
    if frame_x >= request.grid.matrix_columns || frame_y >= request.grid.matrix_rows {
        return None;
    }
    Some((
        i64::try_from(frame_x / u64::from(virtual_tile_width)).ok()?,
        i64::try_from(frame_y / u64::from(virtual_tile_height)).ok()?,
    ))
}

fn plan_lossless_j2k_frame_at(
    slide: &Slide,
    request: LosslessJ2kPlanRequest,
    row: u64,
    col: u64,
    jpeg_preflight: WholeLevelJpegFramePreflight,
) -> Result<LosslessJ2kPlannedFrame, Error> {
    let row_i64 = i64::try_from(row).map_err(|_| Error::Unsupported {
        reason: "J2K row planning tile row exceeds i64".into(),
    })?;
    let col_i64 = i64::try_from(col).map_err(|_| Error::Unsupported {
        reason: "J2K row planning tile column exceeds i64".into(),
    })?;
    let rect = OutputFrameRect::clamped(
        col,
        row,
        request.grid,
        FrameRectOverflowReasons {
            x: "J2K row planning tile x offset overflow",
            y: "J2K row planning tile y offset overflow",
        },
    )?;
    let level = slide
        .dataset()
        .scenes
        .get(request.location.scene_idx)
        .and_then(|scene| scene.series.get(request.location.series_idx))
        .and_then(|series| series.levels.get(request.location.level_idx as usize))
        .ok_or_else(|| Error::Unsupported {
            reason: "J2K frame planning source level is unavailable".into(),
        })?;
    let (native_col, native_row) = match level.tile_layout {
        TileLayout::Regular {
            tile_width,
            tile_height,
            ..
        }
        | TileLayout::WholeLevel {
            virtual_tile_width: tile_width,
            virtual_tile_height: tile_height,
            ..
        } if tile_width > 0 && tile_height > 0 => (
            i64::try_from(rect.x / u64::from(tile_width)).map_err(|_| Error::Unsupported {
                reason: "J2K native tile column exceeds i64".into(),
            })?,
            i64::try_from(rect.y / u64::from(tile_height)).map_err(|_| Error::Unsupported {
                reason: "J2K native tile row exceeds i64".into(),
            })?,
        ),
        _ => (col_i64, row_i64),
    };
    let allow_raw_probe = request.allow_passthrough_probe
        || jpeg_direct_htj2k::transfer_syntax(request.transfer_syntax);
    let (
        source_j2k_syntax,
        source_j2k,
        mut source_jpeg,
        mut source_jpeg_direct_rejected,
        source_jpeg_profile_unsupported,
        mut source_lossy_compression,
        passthrough,
    ) = if allow_raw_probe {
        let tile_request = request.location.tile_request(native_col, native_row);
        match slide.read_raw_compressed_tile(&tile_request) {
            Ok(raw) => {
                let inspection = RawJ2kInspection::new(&raw);
                if inspection.is_none()
                    && matches!(
                        raw.compression(),
                        Compression::Jp2kRgb | Compression::Jp2kYcbcr
                    )
                {
                    return Err(Error::Unsupported {
                        reason: format!(
                            "JPEG 2000 source frame row={row} col={col} cannot be inspected for a preservation route; pixel re-encoding fallback is disabled"
                        ),
                    });
                }
                let source_j2k_syntax = inspection.as_ref().map(RawJ2kInspection::syntax);
                let source_j2k_profile = inspection.as_ref().and_then(RawJ2kInspection::profile);
                let source_lossy_compression = lossy_compression_from_raw(&raw, source_j2k_syntax)?;
                let raw_is_jpeg = raw.compression() == Compression::Jpeg;
                let source_jpeg_profile_unsupported = raw_is_jpeg
                    && jpeg_direct_htj2k::frame_profile_is_unsupported(
                        &raw,
                        request.grid.frame_columns,
                        request.grid.frame_rows,
                        request.transfer_syntax,
                    );
                let passthrough_profile = request
                    .allow_passthrough_probe
                    .then(|| {
                        inspection.as_ref().and_then(|inspection| {
                            inspection.passthrough_profile(
                                &raw,
                                request.grid.frame_columns,
                                request.grid.frame_rows,
                                request.transfer_syntax,
                            )
                        })
                    })
                    .flatten();
                #[cfg(test)]
                let passthrough_syntax = inspection.as_ref().map(RawJ2kInspection::syntax);
                drop(inspection);
                // Passthrough takes priority. Move the payload into its only
                // usable route instead of copying it into competing plans.
                let (source_j2k, source_jpeg, passthrough) =
                    if let Some(profile) = passthrough_profile {
                        (
                            None,
                            None,
                            Some(J2kPassthroughFrame {
                                codestream: raw.into_data(),
                                profile,
                                #[cfg(test)]
                                transfer_syntax: passthrough_syntax
                                    .expect("passthrough profile requires a parsed syntax"),
                            }),
                        )
                    } else if raw_is_jpeg {
                        (
                            None,
                            jpeg_direct_htj2k::frame(
                                raw,
                                request.grid.frame_columns,
                                request.grid.frame_rows,
                                request.transfer_syntax,
                            ),
                            None,
                        )
                    } else {
                        (
                            j2k_direct_htj2k::frame(
                                raw,
                                request.grid.frame_columns,
                                request.grid.frame_rows,
                                request.transfer_syntax,
                                source_j2k_profile,
                            ),
                            None,
                            None,
                        )
                    };
                let source_jpeg_direct_rejected =
                    jpeg_direct_htj2k::transfer_syntax(request.transfer_syntax)
                        && raw_is_jpeg
                        && source_jpeg.is_none();
                (
                    source_j2k_syntax,
                    source_j2k,
                    source_jpeg,
                    source_jpeg_direct_rejected,
                    source_jpeg_profile_unsupported,
                    source_lossy_compression,
                    passthrough,
                )
            }
            Err(wsi_rs::WsiError::Unsupported { .. }) => {
                (None, None, None, false, false, None, None)
            }
            Err(error) => {
                return Err(Error::SlideRead {
                    message: format!("raw source read failed at row={row} col={col}: {error}"),
                });
            }
        }
    } else {
        (None, None, None, false, false, None, None)
    };
    let mut source_jpeg_retiled = false;
    let mut source_jpeg_retile_duration = Duration::ZERO;
    let mut source_jpeg_retile_rejection = None;
    if passthrough.is_none()
        && source_j2k.is_none()
        && source_jpeg.is_none()
        && jpeg_direct_htj2k::transfer_syntax(request.transfer_syntax)
    {
        if jpeg_preflight.profile_unsupported || source_jpeg_profile_unsupported {
            source_jpeg_direct_rejected = true;
            source_jpeg_retile_rejection = Some(JpegRetileRejectionReason::ProfileUnsupported);
        } else {
            match read_raw_jpeg_retile_display_tile(
                slide,
                request.location,
                col,
                row,
                request.grid.frame_columns,
                request.grid.frame_rows,
            )? {
                RawJpegRetileProbe::Accepted(retiled) => {
                    if source_lossy_compression.is_none() {
                        source_lossy_compression = lossy_compression_from_raw(&retiled.raw, None)?;
                    }
                    source_jpeg = jpeg_direct_htj2k::frame(
                        retiled.raw,
                        request.grid.frame_columns,
                        request.grid.frame_rows,
                        request.transfer_syntax,
                    );
                    if source_jpeg.is_some() {
                        source_jpeg_retiled = true;
                        source_jpeg_retile_duration = retiled.duration;
                    } else {
                        source_jpeg_direct_rejected = true;
                        source_jpeg_retile_rejection =
                            Some(JpegRetileRejectionReason::ProfileUnsupported);
                    }
                }
                RawJpegRetileProbe::Rejected(reason) => {
                    source_jpeg_retile_rejection = Some(reason);
                }
            }
        }
    }
    let planned = LosslessJ2kPlannedFrame {
        row,
        col,
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        source_j2k_syntax,
        source_j2k,
        source_jpeg,
        source_jpeg_retiled,
        source_jpeg_retile_duration,
        source_jpeg_retile_rejection,
        source_jpeg_direct_rejected,
        source_lossy_compression,
        passthrough,
    };
    if planned.passthrough.is_none()
        && planned.source_j2k.is_none()
        && planned.source_jpeg.is_none()
        && (planned.source_j2k_syntax.is_some() || planned.source_jpeg_direct_rejected)
    {
        reject_lossy_j2k_lossless_fallback(&planned, request.transfer_syntax, row)?;
        return Err(Error::Unsupported {
            reason: format!(
                "compressed source frame row={row} col={col} cannot be preserved by passthrough or direct transcoding ({source_jpeg_retile_rejection:?}); pixel re-encoding fallback is disabled",
            ),
        });
    }
    Ok(planned)
}

/// Lossy provenance of the source tile at `(col, row)`, or `None` when the
/// tile is unreadable as raw compressed data or was stored losslessly.
pub(super) fn source_tile_lossy_compression(
    slide: &Slide,
    location: InstanceCoordinate,
    col: i64,
    row: i64,
) -> Result<Option<LossyCompressionByteCounts>, Error> {
    let Ok(raw) = slide.read_raw_compressed_tile(&location.tile_request(col, row)) else {
        return Ok(None);
    };
    let syntax = RawJ2kInspection::new(&raw).map(|inspection| inspection.syntax());
    lossy_compression_from_raw(&raw, syntax)
}

fn lossy_compression_from_raw(
    raw: &wsi_rs::RawCompressedTile,
    j2k_syntax: Option<CompressedTransferSyntax>,
) -> Result<Option<LossyCompressionByteCounts>, Error> {
    let method = match raw.compression() {
        Compression::Jpeg => Some(JPEG_BASELINE_METHOD),
        Compression::Jp2kRgb | Compression::Jp2kYcbcr => match j2k_syntax {
            Some(CompressedTransferSyntax::Jpeg2000Lossy) => Some(JPEG_2000_METHOD),
            Some(CompressedTransferSyntax::HtJpeg2000Lossy) => Some(HTJ2K_METHOD),
            _ => None,
        },
        _ => None,
    };
    let Some(method) = method else {
        return Ok(None);
    };
    Ok(Some(LossyCompressionByteCounts::from_raw_tile(
        method, raw,
    )?))
}
