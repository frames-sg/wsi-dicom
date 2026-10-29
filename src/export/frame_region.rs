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

    /// This rectangle as the in-matrix part of a `frame_columns` x
    /// `frame_rows` output frame.
    pub(super) const fn extent(self, frame_columns: u32, frame_rows: u32) -> FrameExtent {
        FrameExtent {
            columns: frame_columns,
            rows: frame_rows,
            inside_columns: self.width,
            inside_rows: self.height,
        }
    }
}

/// An output frame's size and how much of it lies inside the Total Pixel
/// Matrix: less than the frame only in the last column or row of frames,
/// whose remaining pixels are padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameExtent {
    pub(crate) columns: u32,
    pub(crate) rows: u32,
    pub(crate) inside_columns: u32,
    pub(crate) inside_rows: u32,
}

impl FrameExtent {
    /// A frame wholly inside the Total Pixel Matrix.
    #[cfg(test)]
    pub(crate) const fn interior(columns: u32, rows: u32) -> Self {
        Self {
            columns,
            rows,
            inside_columns: columns,
            inside_rows: rows,
        }
    }

    /// The frame at grid position (`col`, `row`) of `columns` x `rows`
    /// frames over a `matrix_columns` x `matrix_rows` Total Pixel Matrix.
    pub(crate) fn at(
        col: u64,
        row: u64,
        columns: u32,
        rows: u32,
        matrix_columns: u64,
        matrix_rows: u64,
    ) -> Self {
        let inside = |index: u64, size: u32, matrix: u64| {
            index
                .checked_mul(u64::from(size))
                .map_or(0, |start| matrix.saturating_sub(start).min(u64::from(size)))
                as u32
        };
        Self {
            columns,
            rows,
            inside_columns: inside(col, columns, matrix_columns),
            inside_rows: inside(row, rows, matrix_rows),
        }
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
    pub(super) bytes: std::sync::Arc<Vec<u8>>,
    pub(super) profile: crate::tile::PixelProfile,
    pub(super) input_decode_duration: std::time::Duration,
    pub(super) compose_duration: std::time::Duration,
}
