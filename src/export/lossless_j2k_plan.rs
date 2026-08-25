use std::time::Duration;

use j2k_core::CompressedTransferSyntax;
use wsi_rs::{Compression, Slide};

use super::frame_region::{FrameRectGrid, FrameRectOverflowReasons, OutputFrameRect};
use super::j2k_policy::RawJ2kInspection;
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
    pub(super) source_j2k_dimensions: Option<(u32, u32)>,
    pub(super) source_j2k_syntax: Option<CompressedTransferSyntax>,
    pub(super) source_j2k_profile: Option<PixelProfile>,
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
            j2k_reencode: self.source_j2k_syntax.is_some()
                && self.source_j2k_dimensions == Some((self.width, self.height)),
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

pub(crate) fn plan_lossless_j2k_frames(
    slide: &Slide,
    request: LosslessJ2kPlanRequest,
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
    let mut planned = Vec::new();
    planned
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
        planned.extend(plan_lossless_j2k_row_at(slide, request, row)?);
    }
    Ok(planned)
}

fn plan_lossless_j2k_row_at(
    slide: &Slide,
    request: LosslessJ2kPlanRequest,
    row: u64,
) -> Result<Vec<LosslessJ2kPlannedFrame>, Error> {
    let tile_count = usize::try_from(request.tile_count).map_err(|_| Error::Unsupported {
        reason: "J2K row planning tile count exceeds platform addressable memory".into(),
    })?;
    let row_i64 = i64::try_from(row).map_err(|_| Error::Unsupported {
        reason: "J2K row planning tile row exceeds i64".into(),
    })?;
    let mut planned = Vec::new();
    planned
        .try_reserve_exact(tile_count)
        .map_err(|_| Error::Unsupported {
            reason: "J2K row plan exceeds available memory".into(),
        })?;
    for offset in 0..tile_count {
        let col = request
            .start_col
            .checked_add(u64::try_from(offset).map_err(|_| Error::Unsupported {
                reason: "J2K row planning tile offset exceeds u64".into(),
            })?)
            .ok_or_else(|| Error::Unsupported {
                reason: "J2K row planning tile column overflow".into(),
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
        let allow_raw_probe = request.allow_passthrough_probe
            || jpeg_direct_htj2k::transfer_syntax(request.transfer_syntax);
        let (
            source_j2k_dimensions,
            source_j2k_syntax,
            source_j2k_profile,
            source_j2k,
            mut source_jpeg,
            source_jpeg_direct_rejected,
            mut source_lossy_compression,
            passthrough,
        ) = if allow_raw_probe {
            let tile_request = request.location.tile_request(col_i64, row_i64);
            match slide.read_raw_compressed_tile(&tile_request) {
                Ok(raw) => {
                    let source_j2k_dimensions = Some((raw.width(), raw.height()));
                    let inspection = RawJ2kInspection::new(&raw);
                    let source_j2k_syntax = inspection.as_ref().map(RawJ2kInspection::syntax);
                    let source_j2k_profile =
                        inspection.as_ref().and_then(RawJ2kInspection::profile);
                    let source_lossy_compression =
                        lossy_compression_from_raw(&raw, source_j2k_syntax)?;
                    let source_j2k = j2k_direct_htj2k::frame(
                        &raw,
                        request.grid.frame_columns,
                        request.grid.frame_rows,
                        request.transfer_syntax,
                        source_j2k_profile,
                    );
                    let source_jpeg = jpeg_direct_htj2k::frame(
                        &raw,
                        request.grid.frame_columns,
                        request.grid.frame_rows,
                        request.transfer_syntax,
                    );
                    let source_jpeg_direct_rejected =
                        jpeg_direct_htj2k::transfer_syntax(request.transfer_syntax)
                            && raw.compression() == Compression::Jpeg
                            && source_jpeg.is_none();
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
                    let passthrough = passthrough_profile.map(|profile| J2kPassthroughFrame {
                        codestream: raw.into_data(),
                        profile,
                        #[cfg(test)]
                        transfer_syntax: passthrough_syntax
                            .expect("passthrough profile requires a parsed syntax"),
                    });
                    (
                        source_j2k_dimensions,
                        source_j2k_syntax,
                        source_j2k_profile,
                        source_j2k,
                        source_jpeg,
                        source_jpeg_direct_rejected,
                        source_lossy_compression,
                        passthrough,
                    )
                }
                Err(_) => (None, None, None, None, None, false, None, None),
            }
        } else {
            (None, None, None, None, None, false, None, None)
        };
        let mut source_jpeg_retiled = false;
        let mut source_jpeg_retile_duration = Duration::ZERO;
        let mut source_jpeg_retile_rejection = None;
        if source_jpeg.is_none() && jpeg_direct_htj2k::transfer_syntax(request.transfer_syntax) {
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
                        &retiled.raw,
                        request.grid.frame_columns,
                        request.grid.frame_rows,
                        request.transfer_syntax,
                    );
                    if source_jpeg.is_some() {
                        source_jpeg_retiled = true;
                        source_jpeg_retile_duration = retiled.duration;
                    } else {
                        source_jpeg_retile_rejection =
                            Some(JpegRetileRejectionReason::ProfileUnsupported);
                    }
                }
                RawJpegRetileProbe::Rejected(reason) => {
                    source_jpeg_retile_rejection = Some(reason);
                }
            }
        }
        planned.push(LosslessJ2kPlannedFrame {
            row,
            col,
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            source_j2k_dimensions,
            source_j2k_syntax,
            source_j2k_profile,
            source_j2k,
            source_jpeg,
            source_jpeg_retiled,
            source_jpeg_retile_duration,
            source_jpeg_retile_rejection,
            source_jpeg_direct_rejected,
            source_lossy_compression,
            passthrough,
        });
    }
    Ok(planned)
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
