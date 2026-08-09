use crate::cli_report::{
    format_sustain_export_iteration_summary, format_sustain_iteration_summary,
};
use std::path::PathBuf;
use wsi_dicom::{ExportMetrics, ExportReport, RouteCoverageReport, RouteProfileReport};

fn test_metrics(configure: impl FnOnce(&mut ExportMetrics)) -> ExportMetrics {
    let mut metrics = ExportMetrics::default();
    configure(&mut metrics);
    metrics
}

fn export_report(output_dir: &str, metrics: ExportMetrics) -> ExportReport {
    let mut report = ExportReport::default();
    report.output_dir = PathBuf::from(output_dir);
    report.metrics = metrics;
    report
}
#[allow(clippy::too_many_arguments)]
fn route_coverage_report(
    source_path: &str,
    transfer_syntax_uid: &'static str,
    requested_frames_per_level: u64,
    available_frames: u64,
    complete_frame_coverage: bool,
    levels: Vec<RouteProfileReport>,
    metrics: ExportMetrics,
    elapsed_micros: u128,
) -> RouteCoverageReport {
    let mut report = RouteCoverageReport::default();
    report.source_path = PathBuf::from(source_path);
    report.transfer_syntax_uid = transfer_syntax_uid;
    report.requested_frames_per_level = requested_frames_per_level;
    report.available_frames = available_frames;
    report.complete_frame_coverage = complete_frame_coverage;
    report.levels = levels;
    report.metrics = metrics;
    report.elapsed_micros = elapsed_micros;
    report
}

#[test]
fn cli_sustain_iteration_summary_reports_throughput_memory_and_thermal_state() {
    let summary = format_sustain_iteration_summary(
        2,
        5,
        &route_coverage_report(
            "source.svs",
            "1.2.840.10008.1.2.4.202",
            4,
            12,
            true,
            Vec::new(),
            test_metrics(|metrics| {
                metrics.routes.total_frames = 12;
                metrics.routes.gpu_transcode_frames = 12;
                metrics.routes.resident_gpu_transcode_frames = 12;
                metrics.routes.gpu_input_decode_frames = 12;
                metrics.routes.gpu_encode_frames = 12;
                metrics.timings.gpu_dispatch_micros = 12_500;
            }),
            2_000_000,
        ),
        Some(40 * 1024 * 1024),
        Some("No thermal warning level has been recorded"),
        Some("System-wide memory free percentage: 92%"),
    );

    assert!(summary.contains("sustain_iteration=2/5"));
    assert!(summary.contains("frames=12"));
    assert!(summary.contains("available_frames=12"));
    assert!(summary.contains("sampled_frames_pct=100.0000"));
    assert!(summary.contains("complete_frame_coverage=true"));
    assert!(summary.contains("frames_per_sec=6.00"));
    assert!(summary.contains("route_gpu_transcode=12"));
    assert!(summary.contains("route_resident_gpu_transcode=12"));
    assert!(summary.contains("gpu_dispatch_ms=12.500"));
    assert!(summary.contains("rss_mb=40.0"));
    assert!(summary.contains("thermal=\"No thermal warning level has been recorded\""));
    assert!(summary.contains("memory_pressure=\"System-wide memory free percentage: 92%\""));
    assert_eq!(
        summary,
        concat!(
            "sustain_iteration=2/5 source=source.svs transfer_syntax=1.2.840.10008.1.2.4.202 frames=12 available_frames=12 ",
            "sampled_frames_pct=100.0000 complete_frame_coverage=true frames_per_sec=6.00 route_passthrough=0 ",
            "route_passthrough_pct=0.0 route_gpu_transcode=12 route_gpu_transcode_pct=100.0 route_resident_gpu_transcode=12 ",
            "route_partial_gpu_transcode=0 route_cpu_fallback=0 route_cpu_fallback_pct=0.0 route_unclassified=0 cpu_input=0 ",
            "gpu_input_decode=12 gpu_encode=12 gpu_validation=0 gray_frames=0 rgb_like_frames=0 other_component_frames=0 ",
            "unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=0 ",
            "gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 gpu_encode_effective_inflight_tiles=0 ",
            "gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 gpu_encode_effective_memory_mib=0 ",
            "gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=12.500 gpu_encode_hardware_ms=0.000 ",
            "gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=0 ",
            "auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=0 j2k_passthrough=0 j2k_direct_htj2k=0 ",
            "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=0 ",
            "jpeg_cpu_encode=0 jpeg_metal_encode=0 final_byte_ms=0.000 input_decode_ms=0.000 compose_ms=0.000 encode_ms=0.000 ",
            "validation_ms=0.000 elapsed_ms=2000.000 rss_mb=40.0 thermal=\"No thermal warning level has been recorded\" ",
            "memory_pressure=\"System-wide memory free percentage: 92%\""
        )
    );
}

#[test]
fn cli_sustain_convert_summary_reports_real_export_throughput() {
    let summary = format_sustain_export_iteration_summary(
        1,
        3,
        &export_report(
            "out/iteration-0001",
            test_metrics(|metrics| {
                metrics.routes.total_frames = 20;
                metrics.routes.j2k_passthrough_frames = 4;
                metrics.routes.gpu_transcode_frames = 12;
                metrics.routes.resident_gpu_transcode_frames = 10;
                metrics.routes.partial_gpu_transcode_frames = 2;
                metrics.routes.cpu_fallback_frames = 4;
                metrics.routes.gpu_input_decode_frames = 12;
                metrics.routes.gpu_encode_frames = 12;
                metrics.timings.write_micros = 3_500;
                metrics.timings.input_decode_micros = 8_000;
                metrics.timings.compose_micros = 2_000;
                metrics.timings.encode_micros = 4_000;
                metrics.timings.validation_micros = 1_000;
                metrics.timings.gpu_dispatch_micros = 15_000;
            }),
        ),
        2_000_000,
        Some(50 * 1024 * 1024),
        Some("No thermal warning level has been recorded"),
        Some("System-wide memory free percentage: 91%"),
    );

    assert!(summary.contains("sustain_iteration=1/3"));
    assert!(summary.contains("mode=convert"));
    assert!(summary.contains("output=out/iteration-0001"));
    assert!(summary.contains("frames=20"));
    assert!(summary.contains("frames_per_sec=10.00"));
    assert!(summary.contains("route_passthrough=4"));
    assert!(summary.contains("route_gpu_transcode=12"));
    assert!(summary.contains("route_resident_gpu_transcode=10"));
    assert!(summary.contains("route_partial_gpu_transcode=2"));
    assert!(summary.contains("route_cpu_fallback=4"));
    assert!(summary.contains("gpu_dispatch_ms=15.000"));
    assert!(summary.contains("final_byte_ms=3.500"));
    assert!(summary.contains("elapsed_ms=2000.000"));
    assert!(summary.contains("rss_mb=50.0"));
    assert!(summary.contains("thermal=\"No thermal warning level has been recorded\""));
    assert!(summary.contains("memory_pressure=\"System-wide memory free percentage: 91%\""));
    assert_eq!(
        summary,
        concat!(
            "sustain_iteration=1/3 mode=convert output=out/iteration-0001 instances=0 frames=20 frames_per_sec=10.00 ",
            "route_passthrough=4 route_passthrough_pct=20.0 route_gpu_transcode=12 route_gpu_transcode_pct=60.0 ",
            "route_resident_gpu_transcode=10 route_partial_gpu_transcode=2 route_cpu_fallback=4 route_cpu_fallback_pct=20.0 ",
            "route_unclassified=0 cpu_input=0 gpu_input_decode=12 gpu_encode=12 gpu_validation=0 gray_frames=0 rgb_like_frames=0 ",
            "other_component_frames=0 unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 ",
            "gpu_input_batches=0 gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 ",
            "gpu_encode_effective_inflight_tiles=0 gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 ",
            "gpu_encode_effective_memory_mib=0 gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=15.000 ",
            "gpu_encode_hardware_ms=0.000 gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 ",
            "auto_probe_gpu_batches=0 auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=0 j2k_passthrough=4 j2k_direct_htj2k=0 ",
            "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=0 ",
            "jpeg_cpu_encode=0 jpeg_metal_encode=0 final_byte_ms=3.500 input_decode_ms=8.000 ",
            "compose_ms=2.000 encode_ms=4.000 validation_ms=1.000 elapsed_ms=2000.000 rss_mb=50.0 ",
            "thermal=\"No thermal warning level has been recorded\" memory_pressure=\"System-wide memory free percentage: 91%\""
        )
    );
}
