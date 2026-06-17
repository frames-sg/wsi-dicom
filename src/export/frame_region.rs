use statumen::{PlaneSelection, TileRequest};

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameLocation {
    pub(crate) scene_idx: usize,
    pub(crate) series_idx: usize,
    pub(crate) level_idx: u32,
    pub(crate) z: u32,
    pub(crate) c: u32,
    pub(crate) t: u32,
}

impl FrameLocation {
    #[cfg(test)]
    pub(super) fn first_series_level(level_idx: u32) -> Self {
        Self {
            scene_idx: 0,
            series_idx: 0,
            level_idx,
            z: 0,
            c: 0,
            t: 0,
        }
    }

    pub(super) fn tile_request(self, col: i64, row: i64) -> TileRequest {
        TileRequest {
            scene: self.scene_idx,
            series: self.series_idx,
            level: self.level_idx,
            plane: PlaneSelection {
                z: self.z,
                c: self.c,
                t: self.t,
            },
            col,
            row,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OutputFrameRect {
    pub(super) x: u64,
    pub(super) y: u64,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl OutputFrameRect {
    pub(super) const fn new(x: u64, y: u64, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(super) fn clamped(
        col: u64,
        row: u64,
        grid: FrameRectGrid,
        overflow: FrameRectOverflowReasons,
    ) -> Result<Self, Error> {
        let x = col
            .checked_mul(u64::from(grid.frame_columns))
            .ok_or_else(|| Error::Unsupported {
                reason: overflow.x.into(),
            })?;
        let y = row
            .checked_mul(u64::from(grid.frame_rows))
            .ok_or_else(|| Error::Unsupported {
                reason: overflow.y.into(),
            })?;
        Ok(Self {
            x,
            y,
            width: grid
                .matrix_columns
                .saturating_sub(x)
                .min(u64::from(grid.frame_columns)) as u32,
            height: grid
                .matrix_rows
                .saturating_sub(y)
                .min(u64::from(grid.frame_rows)) as u32,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FrameRectGrid {
    pub(super) matrix_columns: u64,
    pub(super) matrix_rows: u64,
    pub(super) frame_columns: u32,
    pub(super) frame_rows: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FrameRectOverflowReasons {
    pub(super) x: &'static str,
    pub(super) y: &'static str,
}

pub(super) struct PreparedCpuRegion {
    pub(super) bytes: Vec<u8>,
    pub(super) profile: crate::tile::PixelProfile,
    pub(super) input_decode_duration: std::time::Duration,
    pub(super) compose_duration: std::time::Duration,
}
