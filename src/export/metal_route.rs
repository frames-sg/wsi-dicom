use super::*;

pub(super) fn regular_tiled_source_layout(
    level: &statumen::Level,
) -> Option<WholeLevelStripLayout> {
    let TileLayout::Regular {
        tile_width,
        tile_height,
        ..
    } = level.tile_layout
    else {
        return None;
    };
    nonzero_strip_layout(tile_width, tile_height)
}

pub(super) fn whole_level_strip_layout(level: &statumen::Level) -> Option<WholeLevelStripLayout> {
    let TileLayout::WholeLevel {
        virtual_tile_width,
        virtual_tile_height,
        ..
    } = level.tile_layout
    else {
        return None;
    };
    nonzero_strip_layout(virtual_tile_width, virtual_tile_height)
}

fn nonzero_strip_layout(width: u32, height: u32) -> Option<WholeLevelStripLayout> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(WholeLevelStripLayout { width, height })
}

pub(super) fn output_tile_maps_to_statumen_tile(level: &statumen::Level, tile_size: u32) -> bool {
    output_frame_maps_to_statumen_tile(level, tile_size, tile_size)
}

pub(super) fn output_frame_maps_to_statumen_tile(
    level: &statumen::Level,
    frame_columns: u32,
    frame_rows: u32,
) -> bool {
    matches!(
        level.tile_layout,
        TileLayout::Regular {
            tile_width,
            tile_height,
            ..
        } if tile_width == frame_columns && tile_height == frame_rows
    ) || matches!(
        level.tile_layout,
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } if virtual_tile_width == frame_columns && virtual_tile_height == frame_rows
    )
}
