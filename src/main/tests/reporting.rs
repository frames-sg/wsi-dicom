use super::{export_report, route_coverage_report, test_metrics};
use crate::cli_report::{
    format_corpus_coverage_summary_with_memory, format_coverage_summary_with_memory,
    format_profile_summary_with_memory, format_report_summary_with_memory,
};
use std::path::PathBuf;
use wsi_dicom::{
    ExportMetrics, RouteCorpusCoverageFailure, RouteCorpusCoverageReport, RouteCoverageReport,
    RouteProfileReport,
};

fn route_profile_report(
    source_path: &str,
    transfer_syntax_uid: &'static str,
    level: u32,
    requested_frames: u64,
    available_frames: u64,
    metrics: ExportMetrics,
    elapsed_micros: u128,
) -> RouteProfileReport {
    let mut report = RouteProfileReport::default();
    report.source_path = PathBuf::from(source_path);
    report.transfer_syntax_uid = transfer_syntax_uid;
    report.level = level;
    report.requested_frames = requested_frames;
    report.available_frames = available_frames;
    report.metrics = metrics;
    report.elapsed_micros = elapsed_micros;
    report
}

fn corpus_failure(source_path: &str, message: &str) -> RouteCorpusCoverageFailure {
    let mut failure = RouteCorpusCoverageFailure::default();
    failure.source_path = PathBuf::from(source_path);
    failure.message = message.into();
    failure
}

#[allow(clippy::too_many_arguments)]
fn route_corpus_coverage_report(
    source_root: &str,
    transfer_syntax_uids: Vec<&'static str>,
    requested_frames_per_level: u64,
    max_levels: Option<u32>,
    sources_considered: usize,
    available_frames: u64,
    complete_frame_coverage: bool,
    reports: Vec<RouteCoverageReport>,
    failures: Vec<RouteCorpusCoverageFailure>,
    metrics: ExportMetrics,
    elapsed_micros: u128,
) -> RouteCorpusCoverageReport {
    let mut report = RouteCorpusCoverageReport::default();
    report.source_root = PathBuf::from(source_root);
    report.transfer_syntax_uid = common_transfer_syntax_uid(&transfer_syntax_uids);
    report.transfer_syntax_uids = transfer_syntax_uids;
    report.requested_frames_per_level = requested_frames_per_level;
    report.max_levels = max_levels;
    report.sources_considered = sources_considered;
    report.available_frames = available_frames;
    report.complete_frame_coverage = complete_frame_coverage;
    report.reports = reports;
    report.failures = failures;
    report.metrics = metrics;
    report.elapsed_micros = elapsed_micros;
    report
}

fn common_transfer_syntax_uid(transfer_syntax_uids: &[&'static str]) -> Option<&'static str> {
    let first = transfer_syntax_uids.first().copied()?;
    transfer_syntax_uids
        .iter()
        .all(|uid| *uid == first)
        .then_some(first)
}

#[test]
fn cli_summary_reports_passthrough_and_fallback_counts() {
    let summary = format_report_summary_with_memory(
        &export_report(
            "out",
            test_metrics(|metrics| {
                metrics.routes.total_frames = 27;
                metrics.routes.cpu_input_frames = 1;
                metrics.routes.gpu_input_decode_frames = 2;
                metrics.routes.gpu_encode_frames = 3;
                metrics.routes.gpu_validation_frames = 4;
                metrics.routes.jpeg_passthrough_frames = 5;
                metrics.routes.j2k_passthrough_frames = 6;
                metrics.routes.gpu_transcode_frames = 7;
                metrics.routes.resident_gpu_transcode_frames = 4;
                metrics.routes.partial_gpu_transcode_frames = 3;
                metrics.routes.gpu_input_decode_batches = 2;
                metrics.routes.gpu_compose_batches = 1;
                metrics.routes.gpu_encode_batches = 3;
                metrics.routes.cpu_fallback_frames = 9;
                metrics.routes.jpeg_decode_fallback_frames = 1;
                metrics.routes.jpeg_cpu_encode_frames = 1;
                metrics.routes.jpeg_metal_encode_frames = 2;
                metrics.gpu_encode.gpu_encode_configured_inflight_tiles = 8;
                metrics.gpu_encode.gpu_encode_effective_inflight_tiles = 4;
                metrics.gpu_encode.gpu_encode_max_observed_inflight_tiles = 4;
                metrics.gpu_encode.gpu_encode_configured_memory_mib = 4096;
                metrics.gpu_encode.gpu_encode_effective_memory_mib = 3277;
                metrics.gpu_encode.gpu_encode_wall_micros = 5_000;
                metrics.gpu_encode.gpu_encode_hardware_micros = 2_000;
                metrics.gpu_encode.gpu_encode_dispatch_overhead_micros = 4_500;
                metrics.timings.gpu_dispatch_micros = 6_500;
            }),
        ),
        Some(10 * 1024 * 1024),
    );

    assert!(summary.contains("jpeg_passthrough=5"));
    assert!(summary.contains("j2k_passthrough=6"));
    assert!(summary.contains("jpeg_decode_fallback=1"));
    assert!(summary.contains("jpeg_metal_encode=2"));
    assert!(summary.contains("route_passthrough=11"));
    assert!(summary.contains("route_passthrough_pct=40.7"));
    assert!(summary.contains("route_gpu_transcode=7"));
    assert!(summary.contains("route_gpu_transcode_pct=25.9"));
    assert!(summary.contains("route_resident_gpu_transcode=4"));
    assert!(summary.contains("route_partial_gpu_transcode=3"));
    assert!(summary.contains("gpu_input_batches=2"));
    assert!(summary.contains("gpu_compose_batches=1"));
    assert!(summary.contains("gpu_encode_batches=3"));
    assert!(summary.contains("gpu_encode_configured_inflight_tiles=8"));
    assert!(summary.contains("gpu_encode_effective_inflight_tiles=4"));
    assert!(summary.contains("gpu_encode_max_observed_inflight_tiles=4"));
    assert!(summary.contains("gpu_encode_configured_memory_mib=4096"));
    assert!(summary.contains("gpu_encode_effective_memory_mib=3277"));
    assert!(summary.contains("gpu_encode_wall_ms=5.000"));
    assert!(summary.contains("gpu_encode_effective_parallelism=0.400"));
    assert!(summary.contains("gpu_dispatch_ms=6.500"));
    assert!(summary.contains("gpu_encode_hardware_ms=2.000"));
    assert!(summary.contains("gpu_encode_dispatch_overhead_ms=4.500"));
    assert!(summary.contains("route_cpu_fallback=9"));
    assert!(summary.contains("route_cpu_fallback_pct=33.3"));
    assert!(summary.contains("route_unclassified=0"));
    assert!(summary.contains("rss_mb=10.0"));
    assert_eq!(
        summary,
        concat!(
            "wrote 0 WSI DICOM instance(s) and 0 annotation sidecar(s) to out; frames total=27 route_passthrough=11 route_passthrough_pct=40.7 ",
            "route_gpu_transcode=7 route_gpu_transcode_pct=25.9 route_resident_gpu_transcode=4 route_partial_gpu_transcode=3 ",
            "route_cpu_fallback=9 route_cpu_fallback_pct=33.3 route_unclassified=0 cpu_input=1 gpu_input_decode=2 ",
            "gpu_encode=3 gpu_validation=4 gray_frames=0 rgb_like_frames=0 other_component_frames=0 unknown_pixel_profile_frames=0 ",
            "bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=2 gpu_compose_batches=1 gpu_encode_batches=3 ",
            "gpu_encode_configured_inflight_tiles=8 gpu_encode_effective_inflight_tiles=4 gpu_encode_max_observed_inflight_tiles=4 ",
            "gpu_encode_configured_memory_mib=4096 gpu_encode_effective_memory_mib=3277 gpu_encode_wall_ms=5.000 ",
            "gpu_encode_effective_parallelism=0.400 gpu_dispatch_ms=6.500 gpu_encode_hardware_ms=2.000 ",
            "gpu_encode_dispatch_overhead_ms=4.500 auto_probe_frames=0 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=0 ",
            "auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=5 j2k_passthrough=6 j2k_direct_htj2k=0 ",
            "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=1 ",
            "jpeg_cpu_encode=1 jpeg_metal_encode=2 input_decode_ms=0.000 compose_ms=0.000 encode_ms=0.000 ",
            "validation_ms=0.000 write_ms=0.000 rss_mb=10.0"
        )
    );
}

#[test]
fn cli_profile_summary_reports_bounded_route_counts() {
    let summary = format_profile_summary_with_memory(
        &route_profile_report(
            "source.svs",
            "1.2.840.10008.1.2.4.202",
            2,
            12,
            20,
            test_metrics(|metrics| {
                metrics.routes.total_frames = 10;
                metrics.routes.gpu_transcode_frames = 7;
                metrics.routes.resident_gpu_transcode_frames = 4;
                metrics.routes.partial_gpu_transcode_frames = 3;
                metrics.routes.cpu_fallback_frames = 3;
                metrics.routes.gpu_input_decode_frames = 7;
                metrics.routes.gpu_encode_frames = 7;
                metrics.routes.jpeg_passthrough_frames = 2;
                metrics.routes.jpeg_decode_fallback_frames = 1;
                metrics.routes.jpeg_cpu_encode_frames = 1;
                metrics.routes.jpeg_metal_encode_frames = 2;
                metrics.routes.auto_route_probe_frames = 2;
                metrics.routes.auto_route_probe_gpu_batches = 3;
                metrics.routes.auto_route_probe_cpu_micros = 1_200;
                metrics.routes.auto_route_probe_gpu_micros = 1_300;
                metrics.timings.gpu_dispatch_micros = 6_500;
                metrics.timings.write_micros = 1_250;
            }),
            42_500,
        ),
        Some(20 * 1024 * 1024),
    );

    assert!(summary.contains("profiled source.svs"));
    assert!(summary.contains("level=2"));
    assert!(summary.contains("requested_frames=12"));
    assert!(summary.contains("available_frames=20"));
    assert!(summary.contains("sampled_frames_pct=50.0000"));
    assert!(summary.contains("frames total=10"));
    assert!(summary.contains("route_gpu_transcode=7"));
    assert!(summary.contains("route_gpu_transcode_pct=70.0"));
    assert!(summary.contains("route_resident_gpu_transcode=4"));
    assert!(summary.contains("route_partial_gpu_transcode=3"));
    assert!(summary.contains("route_cpu_fallback=3"));
    assert!(summary.contains("jpeg_passthrough=2"));
    assert!(summary.contains("jpeg_decode_fallback=1"));
    assert!(summary.contains("jpeg_cpu_encode=1"));
    assert!(summary.contains("jpeg_metal_encode=2"));
    assert!(summary.contains("auto_probe_frames=2"));
    assert!(summary.contains("auto_probe_selected_gpu_input=0"));
    assert!(summary.contains("auto_probe_gpu_batches=3"));
    assert!(summary.contains("auto_probe_cpu_ms=1.200"));
    assert!(summary.contains("auto_probe_gpu_ms=1.300"));
    assert!(summary.contains("gpu_dispatch_ms=6.500"));
    assert!(summary.contains("final_byte_ms=1.250"));
    assert!(summary.contains("elapsed_ms=42.500"));
    assert!(summary.contains("rss_mb=20.0"));
    assert_eq!(
        summary,
        concat!(
            "profiled source.svs level=2 transfer_syntax=1.2.840.10008.1.2.4.202 requested_frames=12 available_frames=20 ",
            "sampled_frames_pct=50.0000 frames total=10 route_passthrough=2 route_passthrough_pct=20.0 ",
            "route_gpu_transcode=7 route_gpu_transcode_pct=70.0 route_resident_gpu_transcode=4 route_partial_gpu_transcode=3 ",
            "route_cpu_fallback=3 route_cpu_fallback_pct=30.0 route_unclassified=0 cpu_input=0 gpu_input_decode=7 gpu_encode=7 ",
            "gpu_validation=0 gray_frames=0 rgb_like_frames=0 other_component_frames=0 unknown_pixel_profile_frames=0 bits8_frames=0 ",
            "bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=0 gpu_compose_batches=0 gpu_encode_batches=0 ",
            "gpu_encode_configured_inflight_tiles=0 gpu_encode_effective_inflight_tiles=0 gpu_encode_max_observed_inflight_tiles=0 ",
            "gpu_encode_configured_memory_mib=0 gpu_encode_effective_memory_mib=0 gpu_encode_wall_ms=0.000 ",
            "gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=6.500 gpu_encode_hardware_ms=0.000 ",
            "gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=2 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=3 ",
            "auto_probe_cpu_ms=1.200 auto_probe_gpu_ms=1.300 jpeg_passthrough=2 j2k_passthrough=0 j2k_direct_htj2k=0 ",
            "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=1 ",
            "jpeg_cpu_encode=1 jpeg_metal_encode=2 final_byte_ms=1.250 input_decode_ms=0.000 compose_ms=0.000 ",
            "encode_ms=0.000 validation_ms=0.000 elapsed_ms=42.500 rss_mb=20.0"
        )
    );
}

#[test]
fn cli_coverage_summary_reports_aggregate_route_counts() {
    let summary = format_coverage_summary_with_memory(
        &route_coverage_report(
            "source.ndpi",
            "1.2.840.10008.1.2.4.50",
            8,
            20,
            false,
            vec![
                route_profile_report(
                    "source.ndpi",
                    "1.2.840.10008.1.2.4.50",
                    0,
                    8,
                    16,
                    test_metrics(|metrics| {
                        metrics.routes.total_frames = 8;
                        metrics.routes.jpeg_passthrough_frames = 8;
                    }),
                    1_000,
                ),
                route_profile_report(
                    "source.ndpi",
                    "1.2.840.10008.1.2.4.50",
                    1,
                    8,
                    4,
                    test_metrics(|metrics| {
                        metrics.routes.total_frames = 4;
                        metrics.routes.cpu_fallback_frames = 4;
                        metrics.routes.jpeg_decode_fallback_frames = 4;
                        metrics.routes.jpeg_cpu_encode_frames = 4;
                    }),
                    2_000,
                ),
            ],
            test_metrics(|metrics| {
                metrics.routes.total_frames = 12;
                metrics.routes.jpeg_passthrough_frames = 8;
                metrics.routes.cpu_fallback_frames = 4;
                metrics.routes.jpeg_decode_fallback_frames = 4;
                metrics.routes.jpeg_cpu_encode_frames = 4;
                metrics.timings.input_decode_micros = 3_000;
                metrics.timings.encode_micros = 4_000;
                metrics.timings.gpu_dispatch_micros = 7_000;
            }),
            5_000,
        ),
        Some(30 * 1024 * 1024),
    );

    assert!(summary.contains("covered source.ndpi"));
    assert!(summary.contains("levels=2"));
    assert!(summary.contains("requested_frames_per_level=8"));
    assert!(summary.contains("available_frames=20"));
    assert!(summary.contains("sampled_frames_pct=60.0000"));
    assert!(summary.contains("complete_frame_coverage=false"));
    assert!(summary.contains("frames total=12"));
    assert!(summary.contains("route_passthrough=8"));
    assert!(summary.contains("route_passthrough_pct=66.7"));
    assert!(summary.contains("route_cpu_fallback=4"));
    assert!(summary.contains("route_cpu_fallback_pct=33.3"));
    assert!(summary.contains("jpeg_passthrough=8"));
    assert!(summary.contains("jpeg_decode_fallback=4"));
    assert!(summary.contains("jpeg_cpu_encode=4"));
    assert!(summary.contains("gpu_dispatch_ms=7.000"));
    assert!(summary.contains("elapsed_ms=5.000"));
    assert!(summary.contains("rss_mb=30.0"));
    assert_eq!(
        summary,
        concat!(
            "covered source.ndpi levels=2 transfer_syntax=1.2.840.10008.1.2.4.50 requested_frames_per_level=8 ",
            "available_frames=20 sampled_frames_pct=60.0000 complete_frame_coverage=false frames total=12 route_passthrough=8 ",
            "route_passthrough_pct=66.7 route_gpu_transcode=0 route_gpu_transcode_pct=0.0 route_resident_gpu_transcode=0 ",
            "route_partial_gpu_transcode=0 route_cpu_fallback=4 route_cpu_fallback_pct=33.3 route_unclassified=0 cpu_input=0 ",
            "gpu_input_decode=0 gpu_encode=0 gpu_validation=0 gray_frames=0 rgb_like_frames=0 other_component_frames=0 ",
            "unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=0 ",
            "gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 gpu_encode_effective_inflight_tiles=0 ",
            "gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 gpu_encode_effective_memory_mib=0 ",
            "gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=7.000 gpu_encode_hardware_ms=0.000 ",
            "gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=0 ",
            "auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=8 j2k_passthrough=0 j2k_direct_htj2k=0 ",
            "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=4 ",
            "jpeg_cpu_encode=4 jpeg_metal_encode=0 final_byte_ms=0.000 input_decode_ms=3.000 compose_ms=0.000 encode_ms=4.000 ",
            "validation_ms=0.000 elapsed_ms=5.000 rss_mb=30.0"
        )
    );
}

#[test]
fn cli_coverage_summary_formats_full_frame_coverage_request_as_all() {
    let summary = format_coverage_summary_with_memory(
        &route_coverage_report(
            "source.svs",
            "1.2.840.10008.1.2.4.202",
            u64::MAX,
            1,
            true,
            Vec::new(),
            test_metrics(|metrics| {
                metrics.routes.total_frames = 1;
                metrics.routes.gpu_transcode_frames = 1;
            }),
            1_000,
        ),
        None,
    );

    assert!(summary.contains("requested_frames_per_level=all"));
    assert!(summary.contains("complete_frame_coverage=true"));
    assert_eq!(
        summary,
        concat!(
            "covered source.svs levels=0 transfer_syntax=1.2.840.10008.1.2.4.202 requested_frames_per_level=all ",
            "available_frames=1 sampled_frames_pct=100.0000 complete_frame_coverage=true frames total=1 route_passthrough=0 ",
            "route_passthrough_pct=0.0 route_gpu_transcode=1 route_gpu_transcode_pct=100.0 route_resident_gpu_transcode=0 ",
            "route_partial_gpu_transcode=0 route_cpu_fallback=0 route_cpu_fallback_pct=0.0 route_unclassified=0 cpu_input=0 ",
            "gpu_input_decode=0 gpu_encode=0 gpu_validation=0 gray_frames=0 rgb_like_frames=0 other_component_frames=0 ",
            "unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=0 ",
            "gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 gpu_encode_effective_inflight_tiles=0 ",
            "gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 gpu_encode_effective_memory_mib=0 ",
            "gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=0.000 gpu_encode_hardware_ms=0.000 ",
            "gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=0 ",
            "auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=0 j2k_passthrough=0 j2k_direct_htj2k=0 ",
            "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=0 ",
            "jpeg_cpu_encode=0 jpeg_metal_encode=0 final_byte_ms=0.000 input_decode_ms=0.000 compose_ms=0.000 encode_ms=0.000 ",
            "validation_ms=0.000 elapsed_ms=1.000 rss_mb=unknown"
        )
    );
}

#[test]
fn cli_corpus_coverage_summary_reports_sources_failures_and_aggregate_routes() {
    let summary = format_corpus_coverage_summary_with_memory(
        &route_corpus_coverage_report(
            "corpus",
            vec!["1.2.840.10008.1.2.4.202"],
            4,
            Some(1),
            3,
            100_000,
            false,
            vec![route_coverage_report(
                "corpus/source.svs",
                "1.2.840.10008.1.2.4.202",
                4,
                100_000,
                false,
                vec![route_profile_report(
                    "corpus/source.svs",
                    "1.2.840.10008.1.2.4.202",
                    0,
                    4,
                    100_000,
                    test_metrics(|metrics| {
                        metrics.routes.total_frames = 4;
                        metrics.routes.gpu_transcode_frames = 4;
                        metrics.routes.resident_gpu_transcode_frames = 4;
                        metrics.routes.gpu_input_decode_frames = 4;
                        metrics.routes.gpu_encode_frames = 4;
                    }),
                    10_000,
                )],
                test_metrics(|metrics| {
                    metrics.routes.total_frames = 4;
                    metrics.routes.gpu_transcode_frames = 4;
                    metrics.routes.resident_gpu_transcode_frames = 4;
                    metrics.routes.gpu_input_decode_frames = 4;
                    metrics.routes.gpu_encode_frames = 4;
                }),
                10_000,
            )],
            vec![corpus_failure("corpus/bad.svs", "unsupported")],
            test_metrics(|metrics| {
                metrics.routes.total_frames = 4;
                metrics.routes.gpu_transcode_frames = 4;
                metrics.routes.resident_gpu_transcode_frames = 4;
                metrics.routes.gpu_input_decode_frames = 4;
                metrics.routes.gpu_encode_frames = 4;
                metrics.timings.gpu_dispatch_micros = 9_000;
            }),
            12_000,
        ),
        Some(40 * 1024 * 1024),
    );

    assert!(summary.contains("covered_corpus corpus"));
    assert!(summary.contains("sources_considered=3"));
    assert!(summary.contains("sources_profiled=1"));
    assert!(summary.contains("failures=1"));
    assert!(summary.contains("available_frames=100000"));
    assert!(summary.contains("sampled_frames_pct=0.0040"));
    assert!(summary.contains("complete_frame_coverage=false"));
    assert!(summary.contains("route_gpu_transcode=4"));
    assert!(summary.contains("route_gpu_transcode_pct=100.0"));
    assert!(summary.contains("route_resident_gpu_transcode=4"));
    assert!(summary.contains("gpu_dispatch_ms=9.000"));
    assert!(summary.contains("rss_mb=40.0"));
    assert_eq!(
        summary,
        concat!(
            "covered_corpus corpus sources_considered=3 sources_profiled=1 failures=1 common_transfer_syntax=1.2.840.10008.1.2.4.202 transfer_syntaxes=1.2.840.10008.1.2.4.202 ",
            "requested_frames_per_level=4 available_frames=100000 sampled_frames_pct=0.0040 complete_frame_coverage=false ",
            "frames total=4 route_passthrough=0 route_passthrough_pct=0.0 route_gpu_transcode=4 route_gpu_transcode_pct=100.0 ",
            "route_resident_gpu_transcode=4 route_partial_gpu_transcode=0 route_cpu_fallback=0 route_cpu_fallback_pct=0.0 ",
            "route_unclassified=0 cpu_input=0 gpu_input_decode=4 gpu_encode=4 gpu_validation=0 gray_frames=0 rgb_like_frames=0 ",
            "other_component_frames=0 unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 ",
            "gpu_input_batches=0 gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 ",
            "gpu_encode_effective_inflight_tiles=0 gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 ",
            "gpu_encode_effective_memory_mib=0 gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=9.000 ",
            "gpu_encode_hardware_ms=0.000 gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 ",
            "auto_probe_gpu_batches=0 auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=0 j2k_passthrough=0 j2k_direct_htj2k=0 ",
            "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=0 ",
            "jpeg_cpu_encode=0 jpeg_metal_encode=0 final_byte_ms=0.000 input_decode_ms=0.000 ",
            "compose_ms=0.000 encode_ms=0.000 validation_ms=0.000 elapsed_ms=12.000 rss_mb=40.0"
        )
    );
}
