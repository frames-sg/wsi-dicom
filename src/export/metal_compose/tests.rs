use super::addressing::{
    max_destination_byte, max_source_byte, select_address_width, ComposeAddressWidth,
};
use super::*;
use std::time::{Duration, Instant};

#[test]
fn metal_compose_params_layout_matches_shader_struct() {
    let rust_fields = [
        "src_origin_x",
        "src_origin_y",
        "valid_width",
        "valid_height",
        "output_width",
        "output_height",
        "bytes_per_pixel",
        "src_tile_width",
        "src_tile_height",
        "src_slot_stride",
        "src_tile_slot_bytes",
        "src_first_col",
        "src_first_row",
        "src_tiles_across",
        "dst_stride",
    ];
    let shader_params = WSI_COMPOSE_STRIPS_METAL
        .split_once("struct MetalComposeStripsParams {")
        .and_then(|(_, body)| body.split_once("};"))
        .map(|(body, _)| body)
        .expect("Metal compose parameter struct");
    let shader_fields = shader_params
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("uint "))
        .map(|field| field.trim_end_matches(';'))
        .collect::<Vec<_>>();

    assert_eq!(shader_fields, rust_fields);
    assert_eq!(
        core::mem::size_of::<MetalComposeStripsParams>(),
        rust_fields.len() * core::mem::size_of::<u32>()
    );
    assert_eq!(
        core::mem::align_of::<MetalComposeStripsParams>(),
        core::mem::align_of::<u32>()
    );
}

fn address_params(tile_count: u32) -> MetalComposeStripsParams {
    let tile_width = 512;
    let tile_height = 512;
    let bytes_per_pixel = 3;
    MetalComposeStripsParams {
        src_origin_x: (tile_count - 1) * tile_width,
        src_origin_y: 0,
        valid_width: tile_width,
        valid_height: tile_height,
        output_width: tile_width,
        output_height: tile_height,
        bytes_per_pixel,
        src_tile_width: tile_width,
        src_tile_height: tile_height,
        src_slot_stride: tile_width * bytes_per_pixel,
        src_tile_slot_bytes: tile_width * tile_height * bytes_per_pixel,
        src_first_col: 0,
        src_first_row: 0,
        src_tiles_across: tile_count,
        dst_stride: tile_width * bytes_per_pixel,
    }
}

#[test]
fn compose_source_address_crosses_four_gib_at_5_462_rgb_tiles() {
    let below = address_params(5_461);
    let above = address_params(5_462);

    assert_eq!(
        max_source_byte(&below).expect("5,461-tile source address"),
        Some(4_294_705_151)
    );
    assert_eq!(
        max_source_byte(&above).expect("5,462-tile source address"),
        Some(4_295_491_583)
    );
    assert!(max_source_byte(&below).unwrap().unwrap() <= u64::from(u32::MAX));
    assert!(max_source_byte(&above).unwrap().unwrap() > u64::from(u32::MAX));
}

#[test]
fn compose_address_plan_selects_width_from_checked_spans() {
    assert_eq!(
        select_address_width(Some(4_294_705_151), 786_431),
        ComposeAddressWidth::U32
    );
    assert_eq!(
        select_address_width(Some(4_295_491_583), 786_431),
        ComposeAddressWidth::U64
    );
    assert_eq!(
        select_address_width(Some(786_431), u64::from(u32::MAX) + 1),
        ComposeAddressWidth::U64
    );
}

#[test]
fn metal_address_probe_returns_the_checked_64_bit_indices() {
    let Some(device) = metal::Device::system_default() else {
        return;
    };
    let source = format!(
        "{WSI_COMPOSE_STRIPS_METAL}\n{}",
        include_str!("address_probe.metal")
    );
    let library = device
        .new_library_with_source(&source, &metal::CompileOptions::new())
        .expect("compile compose address probe");
    let function = library
        .get_function("wsi_compose_address_probe", None)
        .expect("load compose address probe");
    let pipeline = device
        .new_compute_pipeline_state_with_function(&function)
        .expect("create compose address probe pipeline");
    let output = j2k_metal_support::checked_shared_buffer_for_len::<u64>(&device, 2)
        .expect("allocate compose address output");
    let params = address_params(5_462);
    let coordinate = [511_u32, 511_u32];
    let queue = device.new_command_queue();
    let command_buffer = j2k_metal_support::checked_command_buffer(&queue)
        .expect("create compose address command buffer");
    let encoder = command_buffer.new_compute_command_encoder();
    encoder.set_compute_pipeline_state(&pipeline);
    encoder.set_buffer(0, Some(&output), 0);
    encoder.set_bytes(
        1,
        core::mem::size_of_val(&params) as u64,
        std::ptr::from_ref(&params).cast(),
    );
    encoder.set_bytes(
        2,
        core::mem::size_of_val(&coordinate) as u64,
        coordinate.as_ptr().cast(),
    );
    encoder.dispatch_threads(
        metal::MTLSize {
            width: 1,
            height: 1,
            depth: 1,
        },
        metal::MTLSize {
            width: 1,
            height: 1,
            depth: 1,
        },
    );
    encoder.end_encoding();
    command_buffer.commit();
    command_buffer.wait_until_completed();
    j2k_metal_support::ensure_completed(&command_buffer).expect("compose address completion");

    assert_eq!(
        crate::metal_interop::test_u64_buffer_values(&output, 2),
        [4_295_491_583, 786_431]
    );
}

#[test]
fn compose_destination_address_uses_checked_u64_arithmetic() {
    let mut params = address_params(1);
    params.output_width = 1;
    params.output_height = 2;
    params.dst_stride = u32::MAX;

    assert_eq!(
        max_destination_byte(&params).expect("large destination address"),
        u64::from(u32::MAX) + 2
    );
}

#[test]
fn compose_source_address_rejects_packed_span_overflow() {
    let mut params = address_params(1);
    params.src_origin_x = 0;
    params.src_origin_y = u32::MAX;
    params.valid_height = 1;
    params.src_tile_width = 1;
    params.src_tile_height = 1;
    params.src_first_row = 0;
    params.src_tiles_across = u32::MAX;
    params.src_tile_slot_bytes = u32::MAX;

    let error = max_source_byte(&params).expect_err("packed address must overflow u64");
    assert!(matches!(error, Error::Unsupported { .. }));
    assert!(error.to_string().contains("source address"));
}

#[test]
#[ignore = "run explicitly in release mode for the three-run Metal address-width gate"]
fn metal_compose_selected_u32_stays_within_five_percent_of_reference() {
    const DIMENSION: u32 = 2_048;
    const BYTES_PER_PIXEL: u32 = 3;
    const DISPATCHES_PER_SAMPLE: usize = 12;
    const SAMPLE_COUNT: usize = 3;

    let Some(device) = metal::Device::system_default() else {
        return;
    };
    let source = format!(
        "{WSI_COMPOSE_STRIPS_METAL}\n{}",
        include_str!("address_perf.metal")
    );
    let library = device
        .new_library_with_source(&source, &metal::CompileOptions::new())
        .expect("compile compose address performance kernels");
    let pipeline = |name| {
        let function = library
            .get_function(name, None)
            .expect("load compose address performance function");
        device
            .new_compute_pipeline_state_with_function(&function)
            .expect("create compose address performance pipeline")
    };
    let reference_pipeline = pipeline("wsi_compose_strips_u32_perf_reference");
    let selected_u32_pipeline = pipeline("wsi_compose_strips_u32");
    let u64_pipeline = pipeline("wsi_compose_strips");
    let pitch = DIMENSION * BYTES_PER_PIXEL;
    let byte_len = usize::try_from(pitch)
        .expect("pitch fits usize")
        .checked_mul(usize::try_from(DIMENSION).expect("height fits usize"))
        .expect("performance buffer length");
    let src = j2k_metal_support::checked_shared_buffer_for_len::<u8>(&device, byte_len)
        .expect("allocate performance source");
    let dst = j2k_metal_support::checked_shared_buffer_for_len::<u8>(&device, byte_len)
        .expect("allocate performance destination");
    let params = MetalComposeStripsParams {
        src_origin_x: 0,
        src_origin_y: 0,
        valid_width: DIMENSION,
        valid_height: DIMENSION,
        output_width: DIMENSION,
        output_height: DIMENSION,
        bytes_per_pixel: BYTES_PER_PIXEL,
        src_tile_width: DIMENSION,
        src_tile_height: DIMENSION,
        src_slot_stride: pitch,
        src_tile_slot_bytes: u32::try_from(byte_len).expect("tile bytes fit u32"),
        src_first_col: 0,
        src_first_row: 0,
        src_tiles_across: 1,
        dst_stride: pitch,
    };
    let queue = device.new_command_queue();

    let measure = |pipeline: &metal::ComputePipelineStateRef, dispatches: usize| {
        let command_buffer = j2k_metal_support::checked_command_buffer(&queue)
            .expect("create performance command buffer");
        let thread_width = pipeline.thread_execution_width().max(1);
        let max_threads = pipeline
            .max_total_threads_per_threadgroup()
            .max(thread_width);
        let thread_height = (max_threads / thread_width).max(1);
        let started = Instant::now();
        for _ in 0..dispatches {
            let encoder = command_buffer.new_compute_command_encoder();
            encoder.set_compute_pipeline_state(pipeline);
            encoder.set_buffer(0, Some(&src), 0);
            encoder.set_buffer(1, Some(&dst), 0);
            encoder.set_bytes(
                2,
                core::mem::size_of_val(&params) as u64,
                std::ptr::from_ref(&params).cast(),
            );
            encoder.dispatch_threads(
                metal::MTLSize {
                    width: u64::from(DIMENSION),
                    height: u64::from(DIMENSION),
                    depth: 1,
                },
                metal::MTLSize {
                    width: thread_width,
                    height: thread_height,
                    depth: 1,
                },
            );
            encoder.end_encoding();
        }
        command_buffer.commit();
        command_buffer.wait_until_completed();
        j2k_metal_support::ensure_completed(&command_buffer)
            .expect("complete address performance sample");
        started.elapsed()
    };

    measure(&reference_pipeline, 2);
    measure(&selected_u32_pipeline, 2);
    measure(&u64_pipeline, 2);
    let mut reference_samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut selected_u32_samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut u64_samples = Vec::with_capacity(SAMPLE_COUNT);
    for sample in 0..SAMPLE_COUNT {
        if sample % 2 == 0 {
            reference_samples.push(measure(&reference_pipeline, DISPATCHES_PER_SAMPLE));
            selected_u32_samples.push(measure(&selected_u32_pipeline, DISPATCHES_PER_SAMPLE));
            u64_samples.push(measure(&u64_pipeline, DISPATCHES_PER_SAMPLE));
        } else {
            u64_samples.push(measure(&u64_pipeline, DISPATCHES_PER_SAMPLE));
            selected_u32_samples.push(measure(&selected_u32_pipeline, DISPATCHES_PER_SAMPLE));
            reference_samples.push(measure(&reference_pipeline, DISPATCHES_PER_SAMPLE));
        }
    }

    let median = |samples: &mut Vec<Duration>| {
        samples.sort_unstable();
        samples[samples.len() / 2]
    };
    let reference_median = median(&mut reference_samples);
    let selected_u32_median = median(&mut selected_u32_samples);
    let u64_median = median(&mut u64_samples);
    let selected_ratio = selected_u32_median.as_secs_f64() / reference_median.as_secs_f64();
    let u64_ratio = u64_median.as_secs_f64() / reference_median.as_secs_f64();
    eprintln!(
        "Metal compose address-width benchmark: reference={reference_median:?} selected_u32={selected_u32_median:?} u64={u64_median:?} selected_ratio={selected_ratio:.4} u64_ratio={u64_ratio:.4}"
    );
    assert!(
        selected_ratio <= 1.05,
        "selected u32 Metal composition path regressed by more than 5%: ratio={selected_ratio:.4}"
    );
}
