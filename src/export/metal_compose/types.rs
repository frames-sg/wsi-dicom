use super::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(in crate::export) struct PackedMetalStrips {
    pub(in crate::export) image: j2k_metal_support::ResidentMetalImage,
    pub(in crate::export) first_col: i64,
    pub(in crate::export) first_row: i64,
    pub(in crate::export) tiles_across: u32,
    pub(in crate::export) tiles_down: u32,
    pub(in crate::export) tile_width: u32,
    pub(in crate::export) tile_height: u32,
    pub(in crate::export) slot_stride: usize,
    pub(in crate::export) tile_slot_bytes: usize,
    pub(in crate::export) format: J2kPixelFormat,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
pub(in crate::export) struct MetalComposeTileRequest {
    pub(in crate::export) src_origin_x: u32,
    pub(in crate::export) src_origin_y: u32,
    pub(in crate::export) valid_width: u32,
    pub(in crate::export) valid_height: u32,
    pub(in crate::export) output_width: u32,
    pub(in crate::export) output_height: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct MetalComposeStripsParams {
    pub(super) src_origin_x: u32,
    pub(super) src_origin_y: u32,
    pub(super) valid_width: u32,
    pub(super) valid_height: u32,
    pub(super) output_width: u32,
    pub(super) output_height: u32,
    pub(super) bytes_per_pixel: u32,
    pub(super) src_tile_width: u32,
    pub(super) src_tile_height: u32,
    pub(super) src_slot_stride: u32,
    pub(super) src_tile_slot_bytes: u32,
    pub(super) src_first_col: u32,
    pub(super) src_first_row: u32,
    pub(super) src_tiles_across: u32,
    pub(super) dst_stride: u32,
}
