kernel void wsi_compose_address_probe(
    device ulong *result [[buffer(0)]],
    constant MetalComposeStripsParams &params [[buffer(1)]],
    constant uint2 &coordinate [[buffer(2)]]
) {
    const ulong last_component = ulong(params.bytes_per_pixel - 1);
    result[0] = compose_source_index(coordinate, params) + last_component;
    result[1] = compose_destination_index(coordinate, params) + last_component;
}
