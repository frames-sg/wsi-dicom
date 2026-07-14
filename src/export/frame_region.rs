pub(crate) use crate::coordinate::InstanceCoordinate as FrameLocation;
use crate::error::Error;

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
pub(crate) struct FrameRectGrid {
    pub(crate) matrix_columns: u64,
    pub(crate) matrix_rows: u64,
    pub(crate) frame_columns: u32,
    pub(crate) frame_rows: u32,
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
