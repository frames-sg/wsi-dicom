use std::time::Duration;

use signinum_core::CompressedTransferSyntax;
use statumen::{Compression, PlaneSelection, Slide, TileRequest};

use super::frame_region::{FrameRectGrid, FrameRectOverflowReasons, OutputFrameRect};
use super::j2k_policy::{j2k_passthrough_frame, j2k_raw_frame_syntax_and_profile};
use super::jpeg_retile::{read_raw_jpeg_retile_display_tile, RawJpegRetileProbe};
use super::{j2k_direct_htj2k, jpeg_direct_htj2k, JpegBaselineFrameLocation};
use crate::error::Error;
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
    pub(super) source_raw_probe_failed: bool,
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
}

#[derive(Clone)]
pub(super) struct J2kPassthroughFrame {
    pub(super) codestream: Vec<u8>,
    pub(super) profile: PixelProfile,
    pub(super) transfer_syntax: CompressedTransferSyntax,
}

impl J2kPassthroughFrame {
    pub(super) fn is_lossy(&self) -> bool {
        matches!(
            self.transfer_syntax,
            CompressedTransferSyntax::Jpeg2000Lossy | CompressedTransferSyntax::HtJpeg2000Lossy
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn plan_lossless_j2k_rows(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    start_row: u64,
    row_count: u64,
    start_col: u64,
    tile_count: u64,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
    transfer_syntax: TransferSyntax,
    allow_passthrough_probe: bool,
) -> Result<Vec<LosslessJ2kPlannedFrame>, Error> {
    let rows = usize::try_from(row_count).map_err(|_| Error::Unsupported {
        reason: "J2K row planning row count exceeds platform addressable memory".into(),
    })?;
    let tiles = usize::try_from(tile_count).map_err(|_| Error::Unsupported {
        reason: "J2K row planning tile count exceeds platform addressable memory".into(),
    })?;
    let mut planned = Vec::with_capacity(rows.saturating_mul(tiles));
    for offset in 0..row_count {
        let row = start_row
            .checked_add(offset)
            .ok_or_else(|| Error::Unsupported {
                reason: "J2K row planning tile row overflow".into(),
            })?;
        planned.extend(plan_lossless_j2k_row(
            slide,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            row,
            start_col,
            tile_count,
            matrix_columns,
            matrix_rows,
            tile_size,
            transfer_syntax,
            allow_passthrough_probe,
        )?);
    }
    Ok(planned)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_lossless_j2k_row(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    row: u64,
    start_col: u64,
    tile_count: u64,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
    transfer_syntax: TransferSyntax,
    allow_passthrough_probe: bool,
) -> Result<Vec<LosslessJ2kPlannedFrame>, Error> {
    let tile_count = usize::try_from(tile_count).map_err(|_| Error::Unsupported {
        reason: "J2K row planning tile count exceeds platform addressable memory".into(),
    })?;
    let row_i64 = i64::try_from(row).map_err(|_| Error::Unsupported {
        reason: "J2K row planning tile row exceeds i64".into(),
    })?;
    let mut planned = Vec::with_capacity(tile_count);
    for offset in 0..tile_count {
        let col = start_col
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
            FrameRectGrid {
                matrix_columns,
                matrix_rows,
                frame_columns: tile_size,
                frame_rows: tile_size,
            },
            FrameRectOverflowReasons {
                x: "J2K row planning tile x offset overflow",
                y: "J2K row planning tile y offset overflow",
            },
        )?;
        let allow_raw_probe =
            allow_passthrough_probe || jpeg_direct_htj2k::transfer_syntax(transfer_syntax);
        let (
            source_j2k_dimensions,
            source_j2k_syntax,
            source_j2k_profile,
            source_j2k,
            mut source_jpeg,
            source_jpeg_direct_rejected,
            source_raw_probe_failed,
            passthrough,
        ) = if allow_raw_probe {
            let tile_request = TileRequest {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                plane: PlaneSelection { z, c, t },
                col: col_i64,
                row: row_i64,
            };
            match slide.read_raw_compressed_tile(&tile_request) {
                Ok(raw) => {
                    let source_j2k_dimensions = Some((raw.width, raw.height));
                    let (source_j2k_syntax, source_j2k_profile) =
                        j2k_raw_frame_syntax_and_profile(&raw);
                    let source_j2k = j2k_direct_htj2k::frame(
                        &raw,
                        tile_size,
                        tile_size,
                        transfer_syntax,
                        source_j2k_profile,
                    );
                    let source_jpeg =
                        jpeg_direct_htj2k::frame(&raw, tile_size, tile_size, transfer_syntax);
                    let source_jpeg_direct_rejected =
                        jpeg_direct_htj2k::transfer_syntax(transfer_syntax)
                            && raw.compression == Compression::Jpeg
                            && source_jpeg.is_none();
                    let passthrough = if allow_passthrough_probe {
                        j2k_passthrough_frame(raw, tile_size, tile_size, transfer_syntax)?
                    } else {
                        None
                    };
                    (
                        source_j2k_dimensions,
                        source_j2k_syntax,
                        source_j2k_profile,
                        source_j2k,
                        source_jpeg,
                        source_jpeg_direct_rejected,
                        false,
                        passthrough,
                    )
                }
                Err(_) => (None, None, None, None, None, false, true, None),
            }
        } else {
            (None, None, None, None, None, false, false, None)
        };
        let mut source_jpeg_retiled = false;
        let mut source_jpeg_retile_duration = Duration::ZERO;
        let mut source_jpeg_retile_rejection = None;
        if source_jpeg.is_none() && jpeg_direct_htj2k::transfer_syntax(transfer_syntax) {
            match read_raw_jpeg_retile_display_tile(
                slide,
                JpegBaselineFrameLocation {
                    scene_idx,
                    series_idx,
                    level_idx,
                    z,
                    c,
                    t,
                },
                col,
                row,
                tile_size,
                tile_size,
            )? {
                RawJpegRetileProbe::Accepted(retiled) => {
                    source_jpeg = jpeg_direct_htj2k::frame(
                        &retiled.raw,
                        tile_size,
                        tile_size,
                        transfer_syntax,
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
            source_raw_probe_failed,
            passthrough,
        });
    }
    Ok(planned)
}
