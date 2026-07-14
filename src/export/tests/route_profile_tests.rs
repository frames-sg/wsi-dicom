use super::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn write_route_profile_source_levels_for_test(
    work_dir: &std::path::Path,
    level0_uid: &str,
    level1_uid: &str,
) -> PathBuf {
    let source_dir = work_dir.join("source");
    std::fs::create_dir_all(&source_dir).unwrap();
    let source_level0 = source_dir.join("level0.dcm");
    let source_level1 = source_dir.join("level1.dcm");
    write_source_dicom_with_dimensions(&source_level0, level0_uid, 4, 4);
    write_source_dicom_with_dimensions(&source_level1, level1_uid, 2, 2);
    source_level0
}

fn route_coverage_request_for_test(target: PathBuf) -> RouteCoverageRequest {
    RouteCoverageRequest {
        target: RouteCoverageTarget::Source(target),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Htj2kLossless,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        max_frames_per_level: 1,
        max_levels: None,
        max_level_elapsed: None,
        progress: None,
        max_sources: 100_000,
        max_depth: 64,
    }
}

#[test]
fn profile_dicom_route_coverage_aggregates_all_levels_without_writing_dicom() {
    let tmp = tempfile::tempdir().unwrap();
    let source_level0 = write_route_profile_source_levels_for_test(
        tmp.path(),
        "1.2.826.0.1.3680043.10.999.31",
        "1.2.826.0.1.3680043.10.999.32",
    );

    let report =
        profile_dicom_route_coverage(route_coverage_request_for_test(source_level0)).unwrap();

    assert_eq!(report.requested_frames_per_level, 1);
    assert_eq!(report.levels.len(), 2);
    assert_eq!(report.levels[0].level, 0);
    assert_eq!(report.levels[1].level, 1);
    assert_eq!(report.levels[0].available_frames, 4);
    assert_eq!(report.levels[1].available_frames, 1);
    assert_eq!(report.available_frames, 5);
    assert!(!report.complete_frame_coverage);
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.cpu_input_frames, 2);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 2);
    assert_eq!(report.metrics.route_passthrough_frames(), 0);
    assert_eq!(report.metrics.routes.gpu_transcode_frames, 0);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
    assert!(report.elapsed_micros > 0);
}

#[test]
fn profile_dicom_route_coverage_can_limit_levels_for_bounded_real_checks() {
    let tmp = tempfile::tempdir().unwrap();
    let source_level0 = write_route_profile_source_levels_for_test(
        tmp.path(),
        "1.2.826.0.1.3680043.10.999.41",
        "1.2.826.0.1.3680043.10.999.42",
    );

    let mut request = route_coverage_request_for_test(source_level0);
    request.max_levels = Some(1);
    let report = profile_dicom_route_coverage(request).unwrap();

    assert_eq!(report.levels.len(), 1);
    assert_eq!(report.levels[0].level, 0);
    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 1);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
}

#[test]
fn profile_dicom_route_coverage_rejects_zero_level_elapsed_limit() {
    let mut request = route_coverage_request_for_test(PathBuf::from("source.dcm"));
    request.max_levels = Some(1);
    request.max_level_elapsed = Some(Duration::ZERO);
    let err = profile_dicom_route_coverage(request).unwrap_err();

    assert!(
        err.to_string().contains("max_level_elapsed"),
        "unexpected error: {err}"
    );
}

#[test]
fn route_level_deadline_reports_elapsed_limit() {
    let deadline = RouteLevelDeadline {
        started: Instant::now() - Duration::from_millis(2),
        max_elapsed: Duration::from_millis(1),
    };
    let err = check_route_level_deadline(Some(deadline), 3).unwrap_err();

    assert!(
        err.to_string().contains("max_level_elapsed"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("level 3"),
        "unexpected error: {err}"
    );
}

#[test]
fn profile_dicom_route_corpus_coverage_rejects_zero_level_elapsed_limit() {
    let err = profile_dicom_route_corpus_coverage(RouteCoverageRequest {
        target: RouteCoverageTarget::Corpus(PathBuf::from("slides")),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Htj2kLossless,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        max_frames_per_level: 1,
        max_levels: Some(1),
        max_level_elapsed: Some(Duration::ZERO),
        progress: None,
        max_sources: 100_000,
        max_depth: 64,
    })
    .unwrap_err();

    assert!(
        err.to_string().contains("max_level_elapsed"),
        "unexpected error: {err}"
    );
}

#[test]
fn profile_dicom_routes_limits_frames_without_writing_dicom() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom(&source);

    let report = profile_dicom_routes(RouteProfileRequest {
        source_path: source,
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Htj2kLossless,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        level: 0,
        max_frames: 1,
    })
    .unwrap();

    assert_eq!(report.level, 0);
    assert_eq!(report.requested_frames, 1);
    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.cpu_input_frames, 1);
    assert_eq!(report.metrics.route_passthrough_frames(), 0);
    assert_eq!(report.metrics.routes.gpu_transcode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 1);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
    assert!(report.elapsed_micros > 0);
}

#[test]
fn profile_dicom_routes_reports_jpeg_baseline_cpu_fallback_without_writing_dicom() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom(&source);

    let report = profile_dicom_routes(RouteProfileRequest {
        source_path: source,
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        level: 0,
        max_frames: 1,
    })
    .unwrap();

    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 1);
    assert_eq!(report.metrics.routes.jpeg_cpu_encode_frames, 1);
    assert_eq!(report.metrics.routes.jpeg_metal_encode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_input_frames, 1);
    assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
    assert_eq!(report.metrics.route_passthrough_frames(), 0);
    assert_eq!(report.metrics.routes.gpu_transcode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 1);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
    assert!(report.metrics.timings.input_decode_micros > 0);
    assert!(report.metrics.timings.compose_micros > 0);
    assert!(report.metrics.timings.encode_micros > 0);
    assert!(report.elapsed_micros > 0);
}

#[test]
fn profile_and_export_route_classification_match_for_cpu_pipelines() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let source = temporary_directory.path().join("source.dcm");
    write_source_dicom(&source);

    for (case_name, transfer_syntax) in [
        ("jpeg-baseline", TransferSyntax::JpegBaseline8Bit),
        ("htj2k-lossless", TransferSyntax::Htj2kLossless),
    ] {
        let options = ExportOptions {
            tile_size: 2,
            transfer_syntax,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        };
        let profile = profile_dicom_routes(RouteProfileRequest {
            source_path: source.clone(),
            options: options.clone(),
            source_aware_transfer_syntax: false,
            level: 0,
            max_frames: 2,
        })
        .unwrap();
        let export = export_dicom(ExportRequest {
            source_path: source.clone(),
            output_dir: temporary_directory.path().join(case_name),
            options,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: Some(0),
        })
        .unwrap();

        assert_eq!(
            profile.metrics.routes, export.metrics.routes,
            "profile/export route classification drifted for {case_name}"
        );
        assert_eq!(profile.metrics.route_unclassified_frames(), 0);
        assert_eq!(export.metrics.route_unclassified_frames(), 0);
    }
}

#[test]
fn route_coverage_report_serializes_metrics_for_batch_aggregation() {
    let report = RouteCoverageReport {
        source_path: PathBuf::from("source.svs"),
        transfer_syntax_uid: TransferSyntax::Htj2kLosslessRpcl.uid(),
        requested_frames_per_level: 8,
        available_frames: 64,
        complete_frame_coverage: false,
        levels: vec![RouteProfileReport {
            source_path: PathBuf::from("source.svs"),
            transfer_syntax_uid: TransferSyntax::Htj2kLosslessRpcl.uid(),
            level: 2,
            requested_frames: 8,
            available_frames: 64,
            metrics: ExportMetrics {
                routes: RouteCounters {
                    total_frames: 8,
                    gpu_transcode_frames: 6,
                    resident_gpu_transcode_frames: 6,
                    cpu_fallback_frames: 2,
                    ..RouteCounters::default()
                },
                ..ExportMetrics::default()
            },
            elapsed_micros: 42_000,
        }],
        metrics: ExportMetrics {
            routes: RouteCounters {
                total_frames: 8,
                gpu_transcode_frames: 6,
                resident_gpu_transcode_frames: 6,
                cpu_fallback_frames: 2,
                ..RouteCounters::default()
            },
            gpu_encode: GpuEncodeMetrics {
                gpu_encode_configured_inflight_tiles: 8,
                gpu_encode_effective_inflight_tiles: 4,
                gpu_encode_max_observed_inflight_tiles: 4,
                gpu_encode_configured_memory_mib: 4096,
                gpu_encode_effective_memory_mib: 3277,
                gpu_encode_wall_micros: 5_000,
                gpu_encode_hardware_micros: 2_500,
                gpu_encode_dispatch_overhead_micros: 5_000,
                gpu_encode_plan_micros: 750,
                gpu_encode_prepare_submit_micros: 1_250,
                gpu_encode_ht_table_build_micros: 1_500,
                gpu_encode_ht_buffer_allocation_micros: 1_750,
                gpu_encode_ht_command_encode_micros: 2_250,
                gpu_encode_codestream_wait_micros: 2_750,
                gpu_encode_chunk_count: 3,
                gpu_encode_tile_count: 96,
                gpu_encode_code_block_count: 11_520,
                gpu_pipeline_depth: 3,
                gpu_row_batch_rows_max: 6,
                gpu_row_batch_target_tiles: 96,
            },
            timings: WriteTimings {
                gpu_dispatch_micros: 7_500,
                streaming_write_micros: 2_000,
                pixel_data_patch_micros: 300,
                writer_backpressure_micros: 700,
                ..WriteTimings::default()
            },
            ..ExportMetrics::default()
        },
        elapsed_micros: 45_000,
    };

    let value = serde_json::to_value(&report).unwrap();

    assert_eq!(value["source_path"], "source.svs");
    assert_eq!(value["metrics"]["total_frames"], 8);
    assert_eq!(value["metrics"]["gpu_transcode_frames"], 6);
    assert_eq!(value["metrics"]["gpu_dispatch_micros"], 7_500);
    assert_eq!(value["metrics"]["gpu_encode_configured_inflight_tiles"], 8);
    assert_eq!(value["metrics"]["gpu_encode_effective_inflight_tiles"], 4);
    assert_eq!(
        value["metrics"]["gpu_encode_max_observed_inflight_tiles"],
        4
    );
    assert_eq!(value["metrics"]["gpu_encode_configured_memory_mib"], 4096);
    assert_eq!(value["metrics"]["gpu_encode_effective_memory_mib"], 3277);
    assert_eq!(value["metrics"]["gpu_encode_wall_micros"], 5_000);
    assert_eq!(value["metrics"]["gpu_encode_effective_parallelism"], 0.5);
    assert_eq!(value["metrics"]["gpu_encode_hardware_micros"], 2_500);
    assert_eq!(
        value["metrics"]["gpu_encode_dispatch_overhead_micros"],
        5_000
    );
    assert_eq!(value["metrics"]["gpu_encode_plan_micros"], 750);
    assert_eq!(value["metrics"]["gpu_encode_prepare_submit_micros"], 1_250);
    assert_eq!(value["metrics"]["gpu_encode_ht_table_build_micros"], 1_500);
    assert_eq!(
        value["metrics"]["gpu_encode_ht_buffer_allocation_micros"],
        1_750
    );
    assert_eq!(
        value["metrics"]["gpu_encode_ht_command_encode_micros"],
        2_250
    );
    assert_eq!(value["metrics"]["gpu_encode_codestream_wait_micros"], 2_750);
    assert_eq!(value["metrics"]["gpu_encode_chunk_count"], 3);
    assert_eq!(value["metrics"]["gpu_encode_tile_count"], 96);
    assert_eq!(value["metrics"]["gpu_encode_code_block_count"], 11_520);
    assert_eq!(value["metrics"]["gpu_pipeline_depth"], 3);
    assert_eq!(value["metrics"]["gpu_row_batch_rows_max"], 6);
    assert_eq!(value["metrics"]["gpu_row_batch_target_tiles"], 96);
    assert_eq!(value["metrics"]["streaming_write_micros"], 2_000);
    assert_eq!(value["metrics"]["pixel_data_patch_micros"], 300);
    assert_eq!(value["metrics"]["writer_backpressure_micros"], 700);
    assert_eq!(value["metrics"]["cpu_fallback_frames"], 2);
    assert_eq!(value["available_frames"], 64);
    assert_eq!(value["complete_frame_coverage"], false);
    assert_eq!(value["levels"][0]["level"], 2);
    assert_eq!(value["levels"][0]["available_frames"], 64);
}

#[test]
fn corpus_route_coverage_aggregates_sources_and_records_failures() {
    let tmp = tempfile::tempdir().unwrap();
    let source_dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&source_dir).unwrap();
    let jpeg = encode_test_jpeg(8, 8, [120, 30, 90]);
    write_tiled_jpeg_tiff(&source_dir.join("good.svs"), 8, 8, 8, 8, &[jpeg]);
    std::fs::write(source_dir.join("bad.svs"), b"not a slide").unwrap();
    std::fs::write(source_dir.join("ignored.txt"), b"ignored").unwrap();

    let report = profile_dicom_route_corpus_coverage(RouteCoverageRequest {
        target: RouteCoverageTarget::Corpus(source_dir),
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        max_frames_per_level: 1,
        max_levels: Some(1),
        max_level_elapsed: None,
        progress: None,
        max_sources: 100_000,
        max_depth: 64,
    })
    .unwrap();

    assert_eq!(report.sources_considered, 2);
    assert_eq!(report.reports.len(), 1);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(
        report.transfer_syntax_uid,
        Some(TransferSyntax::JpegBaseline8Bit.uid())
    );
    assert_eq!(
        report.transfer_syntax_uids,
        vec![TransferSyntax::JpegBaseline8Bit.uid()]
    );
    assert_eq!(report.available_frames, 1);
    assert_eq!(report.reports[0].available_frames, 1);
    assert!(report.failures[0]
        .source_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .contains("bad.svs"));
    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 1);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
}

#[test]
fn profile_dicom_route_coverage_classifies_jpeg_fallback_without_decoding() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom(&source);

    let report = profile_dicom_route_coverage(RouteCoverageRequest {
        target: RouteCoverageTarget::Source(source),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        max_frames_per_level: 1,
        max_levels: Some(1),
        max_level_elapsed: None,
        progress: None,
        max_sources: 100_000,
        max_depth: 64,
    })
    .unwrap();

    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 1);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 1);
    assert_eq!(report.metrics.routes.cpu_input_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_cpu_encode_frames, 0);
    assert_eq!(report.metrics.timings.input_decode_micros, 0);
    assert_eq!(report.metrics.timings.encode_micros, 0);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
}

#[test]
fn profile_dicom_route_coverage_resolves_source_aware_transfer_syntax_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.svs");
    let jpeg = encode_test_jpeg(8, 8, [120, 30, 90]);
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, &[jpeg]);

    let mut request = RouteCoverageRequest::new(
        source,
        ExportOptions {
            tile_size: 8,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
    );
    request.max_levels = Some(1);

    let report = profile_dicom_route_coverage(request).unwrap();

    assert_eq!(
        report.transfer_syntax_uid,
        TransferSyntax::JpegBaseline8Bit.uid()
    );
    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 1);
}

#[test]
fn profile_dicom_route_corpus_coverage_resolves_source_aware_transfer_syntax_per_source() {
    let tmp = tempfile::tempdir().unwrap();
    let source_dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&source_dir).unwrap();
    let jpeg = encode_test_jpeg(8, 8, [120, 30, 90]);
    write_tiled_jpeg_tiff(&source_dir.join("jpeg.svs"), 8, 8, 8, 8, &[jpeg]);
    write_source_dicom(&source_dir.join("raw.dcm"));

    let mut request = RouteCoverageRequest::new_corpus(
        source_dir,
        ExportOptions {
            tile_size: 8,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
    );
    request.max_frames_per_level = 1;
    request.max_levels = Some(1);

    let report = profile_dicom_route_corpus_coverage(request).unwrap();

    assert_eq!(report.sources_considered, 2);
    assert_eq!(report.reports.len(), 2);
    assert!(report.failures.is_empty());
    assert_eq!(report.transfer_syntax_uid, None);
    assert_eq!(
        report.transfer_syntax_uids,
        vec![
            TransferSyntax::Htj2kLosslessRpcl.uid(),
            TransferSyntax::JpegBaseline8Bit.uid(),
        ]
    );
    assert!(report
        .reports
        .iter()
        .any(|report| report.transfer_syntax_uid == TransferSyntax::JpegBaseline8Bit.uid()));
    assert!(report
        .reports
        .iter()
        .any(|report| report.transfer_syntax_uid == TransferSyntax::Htj2kLosslessRpcl.uid()));
}
