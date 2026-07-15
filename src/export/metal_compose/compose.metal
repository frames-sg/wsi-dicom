#include <metal_stdlib>
using namespace metal;

struct MetalComposeStripsParams {
    uint src_origin_x;
    uint src_origin_y;
    uint valid_width;
    uint valid_height;
    uint output_width;
    uint output_height;
    uint bytes_per_pixel;
    uint src_tile_width;
    uint src_tile_height;
    uint src_slot_stride;
    uint src_tile_slot_bytes;
    uint src_first_col;
    uint src_first_row;
    uint src_tiles_across;
    uint dst_stride;
};

static inline ulong compose_destination_index(
    uint2 gid,
    constant MetalComposeStripsParams &params
) {
    return ulong(gid.y) * ulong(params.dst_stride)
        + ulong(gid.x) * ulong(params.bytes_per_pixel);
}

static inline ulong compose_source_index(
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
    const ulong tile_idx = ulong(packed_row) * ulong(params.src_tiles_across)
        + ulong(packed_col);
    return tile_idx * ulong(params.src_tile_slot_bytes)
        + ulong(in_tile_y) * ulong(params.src_slot_stride)
        + ulong(in_tile_x) * ulong(params.bytes_per_pixel);
}

static inline uint compose_destination_index_u32(
    uint2 gid,
    constant MetalComposeStripsParams &params
) {
    return gid.y * params.dst_stride + gid.x * params.bytes_per_pixel;
}

static inline uint compose_source_index_u32(
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

template <typename Index>
static inline void compose_pixel(
    device const uchar *src,
    device uchar *dst,
    Index src_idx,
    Index dst_idx,
    bool inside,
    uint bytes_per_pixel
) {
    if (!inside) {
        for (uint byte_idx = 0u; byte_idx < bytes_per_pixel; ++byte_idx) {
            dst[dst_idx + Index(byte_idx)] = uchar(0);
        }
        return;
    }
    for (uint byte_idx = 0u; byte_idx < bytes_per_pixel; ++byte_idx) {
        dst[dst_idx + Index(byte_idx)] = src[src_idx + Index(byte_idx)];
    }
}

// Keep this hot path explicit. Routing it through compose_pixel regressed the
// release-mode address-width performance guard on Apple GPU hardware.
kernel void wsi_compose_strips_u32(
    device const uchar *src [[buffer(0)]],
    device uchar *dst [[buffer(1)]],
    constant MetalComposeStripsParams &params [[buffer(2)]],
    uint2 gid [[thread_position_in_grid]]
) {
    if (gid.x >= params.output_width || gid.y >= params.output_height) {
        return;
    }

    const uint dst_idx = compose_destination_index_u32(gid, params);
    const bool inside = gid.x < params.valid_width && gid.y < params.valid_height;
    if (!inside) {
        for (uint byte_idx = 0u; byte_idx < params.bytes_per_pixel; ++byte_idx) {
            dst[dst_idx + byte_idx] = uchar(0);
        }
        return;
    }

    const uint src_idx = compose_source_index_u32(gid, params);
    for (uint byte_idx = 0u; byte_idx < params.bytes_per_pixel; ++byte_idx) {
        dst[dst_idx + byte_idx] = src[src_idx + byte_idx];
    }
}

kernel void wsi_compose_strips(
    device const uchar *src [[buffer(0)]],
    device uchar *dst [[buffer(1)]],
    constant MetalComposeStripsParams &params [[buffer(2)]],
    uint2 gid [[thread_position_in_grid]]
) {
    if (gid.x >= params.output_width || gid.y >= params.output_height) {
        return;
    }

    const bool inside = gid.x < params.valid_width && gid.y < params.valid_height;
    const ulong src_idx = inside ? compose_source_index(gid, params) : 0ul;
    compose_pixel(
        src,
        dst,
        src_idx,
        compose_destination_index(gid, params),
        inside,
        params.bytes_per_pixel
    );
}
