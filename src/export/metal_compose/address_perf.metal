static inline uint compose_destination_index_u32_reference(
    uint2 gid,
    constant MetalComposeStripsParams &params
) {
    return gid.y * params.dst_stride + gid.x * params.bytes_per_pixel;
}

static inline uint compose_source_index_u32_reference(
    uint2 gid,
    constant MetalComposeStripsParams &params
) {
    const uint global_x = params.src_origin_x + gid.x;
    const uint global_y = params.src_origin_y + gid.y;
    const uint source_col = global_x / params.src_tile_width;
    const uint source_row = global_y / params.src_tile_height;
    const uint in_tile_x = global_x - source_col * params.src_tile_width;
    const uint in_tile_y = global_y - source_row * params.src_tile_height;
    const uint packed_col = source_col - params.src_first_col;
    const uint packed_row = source_row - params.src_first_row;
    const uint tile_idx = packed_row * params.src_tiles_across + packed_col;
    return tile_idx * params.src_tile_slot_bytes
        + in_tile_y * params.src_slot_stride
        + in_tile_x * params.bytes_per_pixel;
}

kernel void wsi_compose_strips_u32_perf_reference(
    device const uchar *src [[buffer(0)]],
    device uchar *dst [[buffer(1)]],
    constant MetalComposeStripsParams &params [[buffer(2)]],
    uint2 gid [[thread_position_in_grid]]
) {
    if (gid.x >= params.output_width || gid.y >= params.output_height) {
        return;
    }

    const uint dst_idx = compose_destination_index_u32_reference(gid, params);
    const bool inside = gid.x < params.valid_width && gid.y < params.valid_height;
    if (!inside) {
        for (uint byte_idx = 0u; byte_idx < params.bytes_per_pixel; ++byte_idx) {
            dst[dst_idx + byte_idx] = uchar(0);
        }
        return;
    }

    const uint src_idx = compose_source_index_u32_reference(gid, params);
    for (uint byte_idx = 0u; byte_idx < params.bytes_per_pixel; ++byte_idx) {
        dst[dst_idx + byte_idx] = src[src_idx + byte_idx];
    }
}
