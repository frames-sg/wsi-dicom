use std::time::{Duration, Instant};

use statumen::{PlaneSelection, RawCompressedTile, Slide, TileViewRequest, WsiError};

use super::{raw_jpeg_matches_frame_geometry, JpegBaselineFrameLocation};
use crate::error::Error;
use crate::report::JpegRetileRejectionReason;

pub(super) struct RawJpegRetileFrame {
    pub(super) raw: RawCompressedTile,
    pub(super) duration: Duration,
}

pub(super) enum RawJpegRetileProbe {
    Accepted(RawJpegRetileFrame),
    Rejected(JpegRetileRejectionReason),
}

pub(super) fn read_raw_jpeg_retile_display_tile(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    col: u64,
    row: u64,
    frame_columns: u32,
    frame_rows: u32,
) -> Result<RawJpegRetileProbe, Error> {
    let col_i64 = i64::try_from(col).map_err(|_| Error::Unsupported {
        reason: "JPEG retile tile column exceeds i64".into(),
    })?;
    let row_i64 = i64::try_from(row).map_err(|_| Error::Unsupported {
        reason: "JPEG retile tile row exceeds i64".into(),
    })?;
    let started = Instant::now();
    let raw = match slide.read_raw_compressed_display_tile(&TileViewRequest {
        scene: location.scene_idx,
        series: location.series_idx,
        level: location.level_idx,
        plane: PlaneSelection {
            z: location.z,
            c: location.c,
            t: location.t,
        },
        col: col_i64,
        row: row_i64,
        tile_width: frame_columns,
        tile_height: frame_rows,
    }) {
        Ok(raw) => raw,
        Err(err) => {
            return Ok(RawJpegRetileProbe::Rejected(
                classify_raw_jpeg_retile_error(&err),
            ));
        }
    };
    if raw_jpeg_matches_frame_geometry(&raw, frame_columns, frame_rows) {
        Ok(RawJpegRetileProbe::Accepted(RawJpegRetileFrame {
            raw,
            duration: started.elapsed(),
        }))
    } else {
        Ok(RawJpegRetileProbe::Rejected(
            JpegRetileRejectionReason::GeometryMismatch,
        ))
    }
}

const STATUMEN_RAW_JPEG_RETILE_ERROR_MARKERS: [&str; 3] = ["mcu", "restart", "dct"];

fn classify_raw_jpeg_retile_error(err: &WsiError) -> JpegRetileRejectionReason {
    let message = match err {
        WsiError::TileRead { reason, .. } | WsiError::Unsupported { reason } => reason.as_str(),
        WsiError::Jpeg(reason) => reason.as_str(),
        _ => return JpegRetileRejectionReason::SourceUnsupported,
    };
    let message = message.to_ascii_lowercase();
    if STATUMEN_RAW_JPEG_RETILE_ERROR_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
    {
        JpegRetileRejectionReason::McuInvalid
    } else {
        JpegRetileRejectionReason::SourceUnsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_jpeg_retile_error_classification_uses_statumen_error_variants() {
        assert_eq!(
            classify_raw_jpeg_retile_error(&WsiError::TileRead {
                col: 0,
                row: 0,
                level: 0,
                reason: "NDPI MCU-starts table is not strictly increasing".into(),
            }),
            JpegRetileRejectionReason::McuInvalid
        );
        assert_eq!(
            classify_raw_jpeg_retile_error(&WsiError::Unsupported {
                reason: "NDPI raw JPEG retile requires restart segments to align to image rows"
                    .into(),
            }),
            JpegRetileRejectionReason::McuInvalid
        );
        assert_eq!(
            classify_raw_jpeg_retile_error(&WsiError::Jpeg(
                "NDPI raw JPEG retile DCT extract failed".into()
            )),
            JpegRetileRejectionReason::McuInvalid
        );
        assert_eq!(
            classify_raw_jpeg_retile_error(&WsiError::Unsupported {
                reason: "raw compressed tile is unavailable".into(),
            }),
            JpegRetileRejectionReason::SourceUnsupported
        );
    }
}
