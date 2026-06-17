use super::*;

#[test]
fn default_options_use_htj2k_lossless_rpcl_and_auto_backend() {
    let options = ExportOptions::default();

    assert_eq!(options.tile_size, 512);
    assert_eq!(options.transfer_syntax.uid(), "1.2.840.10008.1.2.4.202");
    assert_eq!(options.jpeg_quality, 90);
    assert_eq!(options.encode_backend, EncodeBackendPreference::Auto);
    assert_eq!(options.codec_validation, CodecValidation::Disabled);
    assert!(!options.source_device_decode);
    assert_eq!(options.j2k_decomposition_levels, None);
    assert_eq!(options.gpu_encode_inflight_tiles, None);
    assert_eq!(options.gpu_encode_memory_mib, None);
    assert_eq!(options.gpu_pipeline_depth, None);
    assert_eq!(options.gpu_row_batch_rows, None);
    assert_eq!(options.gpu_row_batch_target_tiles, None);
}

#[test]
fn lossless_j2k_direct_pixel_data_storage_is_auto_tuned_by_host_threads() {
    assert!(lossless_j2k_use_direct_pixel_data(130, 512, 8));
    assert!(!lossless_j2k_use_direct_pixel_data(494, 512, 8));
    assert!(!lossless_j2k_use_direct_pixel_data(130, 512, 1));
    assert_eq!(
        lossless_j2k_direct_pixel_data_memory_bytes(8),
        256 * 1024 * 1024
    );
}

#[test]
fn default_export_instance_workers_parallelizes_cpu_safe_jobs_only() {
    let cpu = ExportOptions {
        encode_backend: EncodeBackendPreference::CpuOnly,
        ..ExportOptions::default()
    };
    assert_eq!(default_export_instance_worker_count(&cpu, 1, 8), 1);
    assert_eq!(default_export_instance_worker_count(&cpu, 4, 8), 4);
    assert_eq!(default_export_instance_worker_count(&cpu, 8, 8), 7);
    assert_eq!(default_export_instance_worker_count(&cpu, 8, 1), 1);

    let require_device = ExportOptions {
        encode_backend: EncodeBackendPreference::RequireDevice,
        ..ExportOptions::default()
    };
    assert_eq!(
        default_export_instance_worker_count(&require_device, 8, 8),
        1
    );

    let auto = ExportOptions::default();
    let expected_auto_workers = if cfg!(any(feature = "metal", feature = "cuda")) {
        1
    } else {
        4
    };
    assert_eq!(
        default_export_instance_worker_count(&auto, 4, 8),
        expected_auto_workers
    );
}

#[test]
fn options_reject_out_of_range_jpeg_quality() {
    for jpeg_quality in [0, 101] {
        let err = ExportOptions {
            jpeg_quality,
            ..ExportOptions::default()
        }
        .validate()
        .unwrap_err();
        assert!(
            err.to_string().contains("jpeg_quality"),
            "unexpected error for quality {jpeg_quality}: {err}"
        );
    }
}

#[test]
fn options_reject_zero_gpu_encode_tuning_overrides() {
    let err = ExportOptions {
        gpu_encode_inflight_tiles: Some(0),
        ..ExportOptions::default()
    }
    .validate()
    .unwrap_err();
    assert!(
        err.to_string().contains("gpu_encode_inflight_tiles"),
        "unexpected error: {err}"
    );

    let err = ExportOptions {
        gpu_encode_memory_mib: Some(0),
        ..ExportOptions::default()
    }
    .validate()
    .unwrap_err();
    assert!(
        err.to_string().contains("gpu_encode_memory_mib"),
        "unexpected error: {err}"
    );

    let err = ExportOptions {
        gpu_pipeline_depth: Some(0),
        ..ExportOptions::default()
    }
    .validate()
    .unwrap_err();
    assert!(
        err.to_string().contains("gpu_pipeline_depth"),
        "unexpected error: {err}"
    );

    let err = ExportOptions {
        gpu_row_batch_rows: Some(0),
        ..ExportOptions::default()
    }
    .validate()
    .unwrap_err();
    assert!(
        err.to_string().contains("gpu_row_batch_rows"),
        "unexpected error: {err}"
    );

    let err = ExportOptions {
        gpu_row_batch_target_tiles: Some(0),
        ..ExportOptions::default()
    }
    .validate()
    .unwrap_err();
    assert!(
        err.to_string().contains("gpu_row_batch_target_tiles"),
        "unexpected error: {err}"
    );
}

#[test]
fn transfer_syntax_uids_include_htj2k_profiles() {
    assert_eq!(TransferSyntax::Jpeg2000.uid(), "1.2.840.10008.1.2.4.91");
    assert_eq!(
        TransferSyntax::Jpeg2000Lossless.uid(),
        "1.2.840.10008.1.2.4.90"
    );
    assert_eq!(TransferSyntax::Htj2k.uid(), "1.2.840.10008.1.2.4.203");
    assert_eq!(
        TransferSyntax::Htj2kLossless.uid(),
        "1.2.840.10008.1.2.4.201"
    );
    assert_eq!(
        TransferSyntax::Htj2kLosslessRpcl.uid(),
        "1.2.840.10008.1.2.4.202"
    );
}

#[test]
fn default_transfer_syntax_prefers_jpeg_passthrough_for_jpeg_backed_source() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(512, 512, [160, 20, 40]);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 512, 512, 512, 512, std::slice::from_ref(&jpeg));

    let selected = default_transfer_syntax_for_source(DefaultTransferSyntaxRequest {
        source_path: source,
        tile_size: 512,
        level_filter: None,
        max_levels: None,
    })
    .unwrap();

    assert_eq!(selected, TransferSyntax::JpegBaseline8Bit);
}

#[test]
fn default_transfer_syntax_prefers_general_jpeg2000_passthrough_source() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes: Vec<u8> = (0..2 * 2 * 3)
        .map(|value| ((value * 13) & 0xFF) as u8)
        .collect();
    let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();
    let source = tmp.path().join("source.svs");
    write_tiled_jp2k_ycbcr_tiff(&source, 2, 2, 2, 2, std::slice::from_ref(&codestream));

    let selected = default_transfer_syntax_for_source(DefaultTransferSyntaxRequest {
        source_path: source,
        tile_size: 512,
        level_filter: None,
        max_levels: None,
    })
    .unwrap();

    assert_eq!(selected, TransferSyntax::Jpeg2000);
}

#[test]
fn default_transfer_syntax_retiles_oversized_jpeg2000_source() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes: Vec<u8> = (0..4 * 4 * 3)
        .map(|value| ((value * 13) & 0xFF) as u8)
        .collect();
    let samples = J2kLosslessSamples::new(&bytes, 4, 4, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();
    let source = tmp.path().join("source.svs");
    write_tiled_jp2k_ycbcr_tiff(&source, 4, 4, 4, 4, std::slice::from_ref(&codestream));

    let selected = default_transfer_syntax_for_source(DefaultTransferSyntaxRequest {
        source_path: source,
        tile_size: 2,
        level_filter: None,
        max_levels: None,
    })
    .unwrap();

    assert_eq!(selected, TransferSyntax::Htj2kLosslessRpcl);
}

#[test]
fn source_aware_builder_writes_requested_tile_geometry_for_oversized_jpeg2000_source() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes: Vec<u8> = (0..4 * 4 * 3)
        .map(|value| ((value * 17) & 0xFF) as u8)
        .collect();
    let samples = J2kLosslessSamples::new(&bytes, 4, 4, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();
    let source = tmp.path().join("source.svs");
    write_tiled_jp2k_ycbcr_tiff(&source, 4, 4, 4, 4, std::slice::from_ref(&codestream));

    let report = Export::from_slide(&source)
        .to_directory(tmp.path().join("out"))
        .tile_size(2)
        .encode_backend(EncodeBackendPreference::CpuOnly)
        .codec_validation(CodecValidation::Disabled)
        .with_research_placeholder_metadata()
        .run()
        .unwrap();

    assert_eq!(report.instances[0].frame_count, 4);
    assert_eq!(
        report.instances[0].transfer_syntax_uid,
        TransferSyntax::Htj2kLosslessRpcl.uid()
    );
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.element(tags::ROWS).unwrap().to_int::<u16>().unwrap(),
        2
    );
    assert_eq!(
        object
            .element(tags::COLUMNS)
            .unwrap()
            .to_int::<u16>()
            .unwrap(),
        2
    );
}

#[test]
fn default_transfer_syntax_falls_back_to_htj2k_when_passthrough_is_unavailable() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom(&source);

    let selected = default_transfer_syntax_for_source(DefaultTransferSyntaxRequest {
        source_path: source,
        tile_size: 2,
        level_filter: None,
        max_levels: None,
    })
    .unwrap();

    assert_eq!(selected, TransferSyntax::Htj2kLosslessRpcl);
}

#[test]
fn dicom_export_builder_defaults_to_source_aware_transfer_syntax() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(512, 512, [160, 20, 40]);
    let source = tmp.path().join("source.svs");
    let output_dir = tmp.path().join("dicom-out");
    write_tiled_jpeg_tiff(&source, 512, 512, 512, 512, std::slice::from_ref(&jpeg));

    let request = Export::from_slide(&source)
        .to_directory(&output_dir)
        .with_research_placeholder_metadata()
        .build_request()
        .unwrap();

    assert_eq!(request.source_path, source);
    assert_eq!(request.output_dir, output_dir);
    assert_eq!(request.metadata, MetadataSource::ResearchPlaceholder);
    assert_eq!(request.level_filter, None);
    assert_eq!(
        request.options.transfer_syntax,
        TransferSyntax::JpegBaseline8Bit
    );
    assert_eq!(
        request.options.tile_size,
        ExportOptions::default().tile_size
    );
}

#[test]
fn dicom_export_builder_explicit_transfer_syntax_overrides_auto() {
    let request = Export::from_slide("source.ndpi")
        .to_directory("dicom-out")
        .with_research_placeholder_metadata()
        .transfer_syntax(TransferSyntax::Htj2kLossless)
        .build_request()
        .unwrap();

    assert_eq!(
        request.options.transfer_syntax,
        TransferSyntax::Htj2kLossless
    );
}

#[test]
fn dicom_export_builder_with_options_preserves_explicit_option_fields() {
    let options = ExportOptions {
        tile_size: 256,
        overwrite: true,
        max_prepared_frame_bytes: 128 * 1024 * 1024,
        transfer_syntax: TransferSyntax::Jpeg2000Lossless,
        jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile::Lossless53,
        jpeg_quality: 80,
        icc_profile_policy: IccProfilePolicy::FallbackSrgb,
        encode_backend: EncodeBackendPreference::CpuOnly,
        codec_validation: CodecValidation::RoundTrip,
        source_device_decode: true,
        j2k_decomposition_levels: Some(3),
        gpu_encode_inflight_tiles: Some(8),
        gpu_encode_memory_mib: Some(4096),
        gpu_pipeline_depth: Some(3),
        gpu_row_batch_rows: Some(6),
        gpu_row_batch_target_tiles: Some(96),
    };

    let request = Export::from_slide("source.ndpi")
        .to_directory("dicom-out")
        .with_research_placeholder_metadata()
        .with_options(options.clone())
        .build_request()
        .unwrap();

    assert_eq!(request.options, options);
}

#[test]
fn dicom_export_builder_metadata_and_level_flow_into_request() {
    let request = Export::from_slide("source.ndpi")
        .to_directory("dicom-out")
        .transfer_syntax(TransferSyntax::Htj2kLosslessRpcl)
        .with_metadata(MetadataSource::ResearchPlaceholder)
        .level(3)
        .build_request()
        .unwrap();

    assert_eq!(request.metadata, MetadataSource::ResearchPlaceholder);
    assert_eq!(request.level_filter, Some(3));
}

#[test]
fn dicom_export_builder_can_return_to_source_aware_transfer_syntax() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(512, 512, [160, 20, 40]);
    let source = tmp.path().join("source.svs");
    let output_dir = tmp.path().join("dicom-out");
    write_tiled_jpeg_tiff(&source, 512, 512, 512, 512, std::slice::from_ref(&jpeg));

    let request = Export::from_slide(&source)
        .to_directory(&output_dir)
        .with_research_placeholder_metadata()
        .with_options(ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::Htj2kLossless,
            jpeg_quality: 80,
            ..ExportOptions::default()
        })
        .source_aware_transfer_syntax()
        .build_request()
        .unwrap();

    assert_eq!(
        request.options.transfer_syntax,
        TransferSyntax::JpegBaseline8Bit
    );
    assert_eq!(request.options.jpeg_quality, 80);
}

#[test]
fn export_request_rejects_zero_tile_size() {
    let err = ExportRequest {
        source_path: PathBuf::from("source.svs"),
        output_dir: PathBuf::from("out"),
        options: ExportOptions {
            tile_size: 0,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    }
    .validate()
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("tile_size must be greater than zero"));
}

#[test]
fn export_request_keeps_source_and_output_paths() {
    let request = ExportRequest::new(
        PathBuf::from("source.ndpi"),
        PathBuf::from("dicom-out"),
        ExportOptions::default(),
        MetadataSource::ResearchPlaceholder,
    )
    .unwrap();

    assert_eq!(request.source_path, PathBuf::from("source.ndpi"));
    assert_eq!(request.output_dir, PathBuf::from("dicom-out"));
}

#[cfg(not(any(feature = "cuda", all(feature = "metal", target_os = "macos"))))]
#[test]
fn auto_and_prefer_device_fall_back_to_facade_cpu_when_no_device_backend_is_enabled() {
    let bytes = vec![0; 16 * 16];
    let samples = J2kLosslessSamples::new(&bytes, 16, 16, 1, 8, false).expect("valid samples");

    let auto = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::Auto)
        .expect("auto backend should fall back to CPU");
    assert_j2k_facade_roundtrip(samples, &auto);

    let prefer = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::PreferDevice)
        .expect("prefer-device backend should fall back to CPU");
    assert_j2k_facade_roundtrip(samples, &prefer);

    let require =
        encode_dicom_j2k_lossless(samples, EncodeBackendPreference::RequireDevice).unwrap_err();
    assert!(require.to_string().contains("device encode backend"));
}
