use super::super::{
    ExportMetrics, GpuEncodeMetrics, JpegDirectHtj2kMetrics, JpegRetileRejectionReason,
    RouteCounters, WriteTimings,
};
use crate::tile::PixelProfile;
use j2k_transcode::TranscodeTimingReport;
use std::time::Duration;

#[test]
fn dicom_export_metrics_serializes_stable_public_fields() {
    let metrics = ExportMetrics {
        routes: RouteCounters {
            total_frames: 10,
            cpu_fallback_frames: 4,
            ..RouteCounters::default()
        },
        jpeg_direct_htj2k: JpegDirectHtj2kMetrics {
            jpeg_direct_htj2k_rejected_frames: 3,
            ..JpegDirectHtj2kMetrics::default()
        },
        gpu_encode: GpuEncodeMetrics {
            gpu_encode_wall_micros: 5_000,
            gpu_encode_hardware_micros: 2_500,
            ..GpuEncodeMetrics::default()
        },
        ..ExportMetrics::default()
    };

    let value = serde_json::to_value(metrics).expect("serialize metrics");

    assert_eq!(value["total_frames"], 10);
    assert_eq!(value["jpeg_direct_htj2k_rejected_frames"], 3);
    assert_eq!(value["cpu_fallback_frames"], 4);
    assert_eq!(value["gpu_encode_wall_micros"], 5_000);
    assert_eq!(value["gpu_encode_hardware_micros"], 2_500);
    assert_eq!(value["gpu_encode_effective_parallelism"], 0.5);
    assert!(value.get("jpeg_retile_to_htj2k_53_frames").is_some());
    assert!(value.get("jpeg_retile_source_unsupported_frames").is_some());
    assert!(value.get("jpeg_retile_geometry_mismatch_frames").is_some());
    assert!(value
        .get("jpeg_retile_profile_unsupported_frames")
        .is_some());
    assert!(value.get("jpeg_retile_mcu_invalid_frames").is_some());
    assert!(value.get("writer_backpressure_micros").is_some());
    assert_eq!(
        value
            .as_object()
            .expect("metrics serialize as object")
            .len(),
        ExportMetrics::SERIALIZED_FIELD_COUNT
    );
}

#[test]
fn metrics_aggregation_saturates_counters_and_preserves_configuration_maxima() {
    let mut aggregate = ExportMetrics {
        routes: RouteCounters {
            total_frames: u64::MAX,
            jpeg_retile_frames: 2,
            ..RouteCounters::default()
        },
        jpeg_direct_htj2k: JpegDirectHtj2kMetrics {
            jpeg_direct_htj2k_53_frames: 3,
            ..JpegDirectHtj2kMetrics::default()
        },
        gpu_encode: GpuEncodeMetrics {
            gpu_encode_configured_inflight_tiles: 8,
            gpu_encode_wall_micros: 5,
            ..GpuEncodeMetrics::default()
        },
        ..ExportMetrics::default()
    };
    let mut next = ExportMetrics {
        routes: RouteCounters {
            total_frames: 1,
            jpeg_retile_frames: 7,
            ..RouteCounters::default()
        },
        jpeg_direct_htj2k: JpegDirectHtj2kMetrics {
            jpeg_direct_htj2k_53_frames: 11,
            ..JpegDirectHtj2kMetrics::default()
        },
        gpu_encode: GpuEncodeMetrics {
            gpu_encode_configured_inflight_tiles: 4,
            gpu_encode_effective_memory_mib: 32,
            gpu_encode_wall_micros: 13,
            ..GpuEncodeMetrics::default()
        },
        ..ExportMetrics::default()
    };
    next.timings.write_micros = 17;

    aggregate.add_assign(next);

    assert_eq!(aggregate.routes.total_frames, u64::MAX);
    assert_eq!(aggregate.routes.jpeg_retile_frames, 9);
    assert_eq!(aggregate.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames, 14);
    assert_eq!(aggregate.timings.write_micros, 17);
    assert_eq!(aggregate.gpu_encode.gpu_encode_wall_micros, 18);
    assert_eq!(aggregate.gpu_encode.gpu_encode_configured_inflight_tiles, 8);
    assert_eq!(aggregate.gpu_encode.gpu_encode_effective_memory_mib, 32);
}

fn fully_populated_metrics() -> ExportMetrics {
    ExportMetrics {
        routes: RouteCounters {
            total_frames: 1,
            cpu_input_frames: 1,
            gpu_input_decode_frames: 1,
            gpu_encode_frames: 1,
            gpu_validation_frames: 1,
            gray_frames: 1,
            rgb_like_frames: 1,
            other_component_frames: 1,
            unknown_pixel_profile_frames: 1,
            bits8_frames: 1,
            bits16_frames: 1,
            other_bit_depth_frames: 1,
            gpu_transcode_frames: 1,
            resident_gpu_transcode_frames: 1,
            partial_gpu_transcode_frames: 1,
            gpu_input_decode_batches: 1,
            gpu_compose_batches: 1,
            gpu_encode_batches: 1,
            auto_route_probe_frames: 1,
            auto_route_probe_gpu_batches: 1,
            auto_route_probe_cpu_micros: 1,
            auto_route_probe_gpu_micros: 1,
            auto_route_probe_selected_gpu_input_frames: 1,
            cpu_fallback_frames: 1,
            jpeg_passthrough_frames: 1,
            j2k_passthrough_frames: 1,
            j2k_direct_htj2k_frames: 1,
            jpeg_retile_frames: 1,
            jpeg_retile_rejected_frames: 1,
            jpeg_retile_source_unsupported_frames: 1,
            jpeg_retile_geometry_mismatch_frames: 1,
            jpeg_retile_profile_unsupported_frames: 1,
            jpeg_retile_mcu_invalid_frames: 1,
            jpeg_retile_us: 1,
            jpeg_retile_to_htj2k_53_frames: 1,
            jpeg_cpu_encode_frames: 1,
            jpeg_metal_encode_frames: 1,
            jpeg_decode_fallback_frames: 1,
        },
        jpeg_direct_htj2k: JpegDirectHtj2kMetrics {
            jpeg_direct_htj2k_53_frames: 1,
            jpeg_direct_htj2k_97_frames: 1,
            jpeg_direct_htj2k_rejected_frames: 1,
            jpeg_direct_htj2k_extract_micros: 1,
            jpeg_direct_htj2k_repack_micros: 1,
            jpeg_direct_htj2k_transform_micros: 1,
            jpeg_direct_htj2k_accelerator_micros: 1,
            jpeg_direct_htj2k_cpu_fallback_micros: 1,
            jpeg_direct_htj2k_dwt_decompose_micros: 1,
            jpeg_direct_htj2k_dwt97_pack_upload_micros: 1,
            jpeg_direct_htj2k_dwt97_idct_row_lift_micros: 1,
            jpeg_direct_htj2k_dwt97_column_lift_micros: 1,
            jpeg_direct_htj2k_dwt97_quantize_codeblock_micros: 1,
            jpeg_direct_htj2k_dwt97_ht_encode_micros: 1,
            jpeg_direct_htj2k_dwt97_ht_kernel_micros: 1,
            jpeg_direct_htj2k_dwt97_ht_status_readback_micros: 1,
            jpeg_direct_htj2k_dwt97_ht_compact_micros: 1,
            jpeg_direct_htj2k_dwt97_ht_output_readback_micros: 1,
            jpeg_direct_htj2k_dwt97_ht_codeblock_dispatches: 1,
            jpeg_direct_htj2k_dwt97_readback_micros: 1,
            jpeg_direct_htj2k_htj2k_encode_micros: 1,
            jpeg_direct_htj2k_encode_accelerator_dispatches: 1,
            jpeg_direct_htj2k_encode_ht_code_block_dispatches: 1,
            jpeg_direct_htj2k_encode_packetization_dispatches: 1,
            jpeg_direct_htj2k_batch_count: 1,
            jpeg_direct_htj2k_batch_jobs: 1,
            jpeg_direct_htj2k_accelerator_attempts: 1,
            jpeg_direct_htj2k_accelerator_jobs: 1,
            jpeg_direct_htj2k_accelerator_dispatches: 1,
            jpeg_direct_htj2k_accelerator_dispatched_jobs: 1,
            jpeg_direct_htj2k_cpu_fallback_jobs: 1,
        },
        gpu_encode: GpuEncodeMetrics {
            gpu_encode_configured_inflight_tiles: 1,
            gpu_encode_effective_inflight_tiles: 1,
            gpu_encode_max_observed_inflight_tiles: 1,
            gpu_encode_configured_memory_mib: 1,
            gpu_encode_effective_memory_mib: 1,
            gpu_encode_wall_micros: 1,
            gpu_encode_hardware_micros: 1,
            gpu_encode_dispatch_overhead_micros: 1,
            gpu_encode_plan_micros: 1,
            gpu_encode_prepare_submit_micros: 1,
            gpu_encode_ht_table_build_micros: 1,
            gpu_encode_ht_buffer_allocation_micros: 1,
            gpu_encode_ht_command_encode_micros: 1,
            gpu_encode_codestream_wait_micros: 1,
            gpu_encode_chunk_count: 1,
            gpu_encode_tile_count: 1,
            gpu_encode_code_block_count: 1,
            gpu_pipeline_depth: 1,
            gpu_row_batch_rows_max: 1,
            gpu_row_batch_target_tiles: 1,
        },
        timings: WriteTimings {
            input_decode_micros: 1,
            compose_micros: 1,
            encode_micros: 1,
            validation_micros: 1,
            gpu_dispatch_micros: 1,
            streaming_write_micros: 1,
            pixel_data_patch_micros: 1,
            writer_backpressure_micros: 1,
            write_micros: 1,
        },
    }
}

#[test]
fn every_serialized_metric_field_has_aggregation_behavior() {
    let mut aggregate = ExportMetrics::default();

    aggregate.add_assign(fully_populated_metrics());

    let aggregated = serde_json::to_value(aggregate).expect("serialize aggregate metrics");
    for (field, value) in aggregated
        .as_object()
        .expect("metrics serialize as a flat object")
    {
        assert_eq!(
            value.as_f64(),
            Some(1.0),
            "serialized metric `{field}` was not aggregated"
        );
    }
}

#[test]
fn metrics_record_route_pixel_and_timing_counters() {
    let mut metrics = ExportMetrics::default();

    metrics.record_cpu_input();
    metrics.record_gpu_input();
    metrics.record_passthrough_frame();
    metrics.record_j2k_passthrough_frame();
    metrics.record_j2k_direct_htj2k_frame(11);
    metrics.record_jpeg_direct_htj2k_53_frame(13);
    metrics.record_jpeg_direct_htj2k_97_frame(17);
    metrics.record_jpeg_direct_htj2k_rejected_frame();
    metrics.record_jpeg_retile_baseline_frame(Duration::from_micros(19));
    metrics.record_jpeg_retile_to_htj2k_53_frame(Duration::from_micros(23));
    metrics.record_jpeg_retile_rejected_frame(JpegRetileRejectionReason::SourceUnsupported);
    metrics.record_jpeg_retile_rejected_frame(JpegRetileRejectionReason::GeometryMismatch);
    metrics.record_jpeg_retile_rejected_frame(JpegRetileRejectionReason::ProfileUnsupported);
    metrics.record_jpeg_retile_rejected_frame(JpegRetileRejectionReason::McuInvalid);
    metrics.record_pixel_profile(PixelProfile {
        components: 1,
        bits_allocated: 8,
        photometric_interpretation: "MONOCHROME2",
    });
    metrics.record_pixel_profile(PixelProfile {
        components: 3,
        bits_allocated: 16,
        photometric_interpretation: "RGB",
    });
    metrics.record_pixel_profile(PixelProfile {
        components: 4,
        bits_allocated: 12,
        photometric_interpretation: "RGBA",
    });
    metrics.record_unknown_pixel_profile();
    metrics.record_transcode_route(true, true);
    metrics.record_transcode_route(true, false);
    metrics.record_transcode_route(false, false);
    metrics.record_gpu_batches(2, 3, 4);
    metrics.record_jpeg_decode_fallback();
    metrics.record_jpeg_cpu_fallback_route_classification();
    metrics.record_j2k_passthrough_only_fallback_classification();
    metrics.record_jpeg_cpu_encode(Duration::from_micros(29));
    metrics.record_jpeg_metal_batch_encode(5, Duration::from_micros(31));
    metrics.record_input_decode_duration(Duration::from_micros(37));
    metrics.record_gpu_input_decode_duration(Duration::from_micros(41));
    metrics.record_compose_duration(Duration::from_micros(43));
    metrics.record_encode_duration(Duration::from_micros(47));
    metrics.record_validation_duration(Duration::from_micros(53));
    metrics.record_gpu_dispatch_duration(Duration::from_micros(59));
    metrics.record_gpu_encode_hardware_duration(
        Some(Duration::from_micros(61)),
        Duration::from_micros(67),
    );
    metrics.record_gpu_encode_wall_duration(Duration::from_micros(69));
    metrics.record_write_duration(Duration::from_micros(71));
    metrics.record_streaming_write_duration(Duration::from_micros(73));
    metrics.record_pixel_data_patch_duration(Duration::from_micros(79));

    metrics.record_jpeg_direct_htj2k_timings(TranscodeTimingReport {
        source_raw_probe_us: 0,
        read_region_decode_us: 0,
        compose_pad_us: 0,
        generated_jpeg_encode_us: 0,
        jpeg_dct_extract_us: 1,
        jpeg_dct_repack_us: 2,
        dct_to_wavelet_total_us: 3,
        dct_to_wavelet_accelerator_us: 4,
        dct_to_wavelet_cpu_fallback_us: 5,
        dwt_decompose_us: 6,
        dwt97_batch_pack_upload_us: 7,
        dwt97_batch_pack_upload_transfers: 0,
        dwt97_batch_pack_upload_bytes: 0,
        dwt97_batch_resident_dct_handoff_count: 0,
        dwt97_batch_idct_row_lift_us: 8,
        dwt97_batch_column_lift_us: 9,
        dwt97_batch_resident_dwt_handoff_count: 0,
        dwt97_batch_quantize_codeblock_us: 10,
        dwt97_batch_ht_encode_us: 11,
        dwt97_batch_ht_kernel_us: 12,
        dwt97_batch_ht_status_readback_us: 13,
        dwt97_batch_ht_status_readback_transfers: 0,
        dwt97_batch_ht_status_readback_bytes: 0,
        dwt97_batch_ht_compact_us: 14,
        dwt97_batch_ht_output_readback_us: 15,
        dwt97_batch_ht_output_readback_transfers: 0,
        dwt97_batch_ht_output_readback_bytes: 0,
        dwt97_batch_ht_codeblock_dispatches: 16,
        dwt97_batch_readback_us: 17,
        dwt97_batch_readback_transfers: 0,
        dwt97_batch_readback_bytes: 0,
        htj2k_encode_us: 18,
        htj2k_encode_accelerator_dispatches: 19,
        htj2k_encode_ht_code_block_dispatches: 20,
        htj2k_encode_packetization_dispatches: 21,
        dicom_spool_write_us: 0,
        dicom_final_write_us: 0,
        tile_count: 0,
        component_count: 0,
        batch_count: 22,
        batch_jobs: 23,
        accelerator_attempts: 24,
        accelerator_jobs: 25,
        accelerator_dispatches: 26,
        accelerator_dispatched_jobs: 27,
        cpu_fallback_jobs: 28,
    });

    assert_eq!(metrics.routes.total_frames, 10);
    assert_eq!(metrics.route_passthrough_frames(), 2);
    assert_eq!(metrics.routes.j2k_direct_htj2k_frames, 1);
    assert_eq!(metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames, 1);
    assert_eq!(metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_97_frames, 1);
    assert_eq!(
        metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_rejected_frames,
        1
    );
    assert_eq!(metrics.jpeg_retile_baseline_frames(), 1);
    assert_eq!(metrics.routes.jpeg_retile_to_htj2k_53_frames, 1);
    assert_eq!(metrics.routes.jpeg_retile_rejected_frames, 4);
    assert_eq!(metrics.routes.jpeg_retile_source_unsupported_frames, 1);
    assert_eq!(metrics.routes.jpeg_retile_geometry_mismatch_frames, 1);
    assert_eq!(metrics.routes.jpeg_retile_profile_unsupported_frames, 1);
    assert_eq!(metrics.routes.jpeg_retile_mcu_invalid_frames, 1);
    assert_eq!(metrics.routes.gray_frames, 1);
    assert_eq!(metrics.routes.rgb_like_frames, 1);
    assert_eq!(metrics.routes.other_component_frames, 1);
    assert_eq!(metrics.routes.bits8_frames, 1);
    assert_eq!(metrics.routes.bits16_frames, 1);
    assert_eq!(metrics.routes.other_bit_depth_frames, 1);
    assert_eq!(metrics.routes.unknown_pixel_profile_frames, 3);
    assert_eq!(metrics.routes.gpu_transcode_frames, 2);
    assert_eq!(metrics.routes.resident_gpu_transcode_frames, 1);
    assert_eq!(metrics.routes.partial_gpu_transcode_frames, 1);
    assert_eq!(metrics.routes.cpu_fallback_frames, 3);
    assert_eq!(metrics.routes.gpu_input_decode_batches, 2);
    assert_eq!(metrics.routes.gpu_compose_batches, 3);
    assert_eq!(metrics.routes.gpu_encode_batches, 4);
    assert_eq!(metrics.routes.jpeg_decode_fallback_frames, 2);
    assert_eq!(metrics.routes.jpeg_cpu_encode_frames, 1);
    assert_eq!(metrics.routes.jpeg_metal_encode_frames, 5);
    assert_eq!(
        metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_extract_micros,
        1
    );
    assert_eq!(
        metrics
            .jpeg_direct_htj2k
            .jpeg_direct_htj2k_encode_packetization_dispatches,
        21
    );
    assert_eq!(
        metrics
            .jpeg_direct_htj2k
            .jpeg_direct_htj2k_dwt97_ht_encode_micros,
        11
    );
    assert_eq!(
        metrics
            .jpeg_direct_htj2k
            .jpeg_direct_htj2k_dwt97_ht_kernel_micros,
        12
    );
    assert_eq!(
        metrics
            .jpeg_direct_htj2k
            .jpeg_direct_htj2k_dwt97_ht_status_readback_micros,
        13
    );
    assert_eq!(
        metrics
            .jpeg_direct_htj2k
            .jpeg_direct_htj2k_dwt97_ht_compact_micros,
        14
    );
    assert_eq!(
        metrics
            .jpeg_direct_htj2k
            .jpeg_direct_htj2k_dwt97_ht_output_readback_micros,
        15
    );
    assert_eq!(
        metrics
            .jpeg_direct_htj2k
            .jpeg_direct_htj2k_dwt97_ht_codeblock_dispatches,
        16
    );
    assert_eq!(
        metrics
            .jpeg_direct_htj2k
            .jpeg_direct_htj2k_cpu_fallback_jobs,
        28
    );
    assert_eq!(metrics.gpu_encode.gpu_encode_wall_micros, 69);
    assert_eq!(metrics.gpu_encode.gpu_encode_hardware_micros, 61);
    assert_eq!(metrics.gpu_encode.gpu_encode_dispatch_overhead_micros, 6);
    assert_eq!(metrics.timings.streaming_write_micros, 73);
    assert_eq!(metrics.timings.pixel_data_patch_micros, 79);
    assert_eq!(metrics.route_unclassified_frames(), 0);
}
