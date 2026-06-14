use super::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TileGrid {
    pub(crate) matrix_columns: u64,
    pub(crate) matrix_rows: u64,
    pub(crate) tile_size: u32,
    pub(crate) tiles_across: u64,
    pub(crate) tiles_down: u64,
}

impl TileGrid {
    pub(crate) fn square(
        matrix_columns: u64,
        matrix_rows: u64,
        tile_size: u32,
    ) -> Result<Self, Error> {
        if tile_size == 0 {
            return Err(Error::Unsupported {
                reason: "tile size must be non-zero".into(),
            });
        }
        let tile_size_u64 = u64::from(tile_size);
        Ok(Self {
            matrix_columns,
            matrix_rows,
            tile_size,
            tiles_across: matrix_columns.div_ceil(tile_size_u64),
            tiles_down: matrix_rows.div_ceil(tile_size_u64),
        })
    }

    pub(crate) fn frame_count_u32(self) -> Result<u32, Error> {
        checked_frame_count_u32(self.tiles_across, self.tiles_down)
    }

    pub(crate) fn frame_count_u64(self) -> Result<u64, Error> {
        checked_frame_count_u64(self.tiles_across, self.tiles_down)
    }

    pub(crate) fn row_tile_count(self, row: u64) -> Result<u64, Error> {
        let row_start = row
            .checked_mul(self.tiles_across)
            .ok_or_else(|| Error::Unsupported {
                reason: "frame row offset overflow".into(),
            })?;
        let remaining = self.frame_count_u64()?.saturating_sub(row_start);
        Ok(self.tiles_across.min(remaining))
    }
}

pub(crate) fn checked_frame_count_u64(tiles_across: u64, tiles_down: u64) -> Result<u64, Error> {
    tiles_across
        .checked_mul(tiles_down)
        .ok_or_else(|| Error::Unsupported {
            reason: "frame count overflow".into(),
        })
}

pub(crate) fn checked_frame_count_u32(tiles_across: u64, tiles_down: u64) -> Result<u32, Error> {
    tiles_across
        .checked_mul(tiles_down)
        .and_then(|count| u32::try_from(count).ok())
        .ok_or_else(|| Error::Unsupported {
            reason: "frame count exceeds u32".into(),
        })
}
