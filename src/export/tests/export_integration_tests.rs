use super::*;

#[test]
fn dicom_export_request_accepts_explicit_research_placeholder_metadata() {
    let request = ExportRequest::new(
        PathBuf::from("source.svs"),
        PathBuf::from("out"),
        ExportOptions::default(),
        MetadataSource::ResearchPlaceholder,
    )
    .unwrap();

    assert!(matches!(
        request.metadata,
        MetadataSource::ResearchPlaceholder
    ));
}

#[test]
fn missing_pixel_spacing_is_rejected_before_frame_export() {
    let err = require_pixel_spacing_mm(None).unwrap_err();

    assert!(
        err.to_string().contains("pixel spacing"),
        "unexpected error: {err}"
    );
}

#[test]
fn fhir_bundle_maps_patient_specimen_service_request_and_report() {
    let bundle = serde_json::json!({
        "resourceType": "Bundle",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": "pat-1",
                    "identifier": [{"value": "MRN123"}],
                    "name": [{"family": "Doe", "given": ["Jane", "Q"]}]
                }
            },
            {
                "resource": {
                    "resourceType": "Specimen",
                    "id": "spec-1",
                    "identifier": [{"value": "S-42"}],
                    "type": {"text": "colon biopsy"}
                }
            },
            {
                "resource": {
                    "resourceType": "ServiceRequest",
                    "id": "sr-1",
                    "identifier": [{"value": "ORDER-7"}],
                    "code": {"text": "Surgical pathology"}
                }
            },
            {
                "resource": {
                    "resourceType": "DiagnosticReport",
                    "identifier": [{"value": "DR-9"}],
                    "code": {"text": "Final pathology report"},
                    "subject": {"reference": "Patient/pat-1"},
                    "specimen": [{"reference": "Specimen/spec-1"}],
                    "basedOn": [{"reference": "ServiceRequest/sr-1"}]
                }
            }
        ]
    });

    let metadata = DicomMetadata::from_fhir_r4_bundle(&bundle).unwrap();

    assert_eq!(metadata.patient_id.as_deref(), Some("MRN123"));
    assert_eq!(metadata.patient_name.as_deref(), Some("Doe^Jane Q"));
    assert_eq!(metadata.specimen_identifier.as_deref(), Some("S-42"));
    assert_eq!(metadata.accession_number.as_deref(), Some("ORDER-7"));
    assert_eq!(
        metadata.study_description.as_deref(),
        Some("Final pathology report")
    );
}

#[test]
fn export_dicom_writes_jpeg2000_lossless_vl_wsi_instances() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let out = tmp.path().join("out");
    write_source_dicom(&source);

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out.clone(),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000Lossless,
            encode_backend: EncodeBackendPreference::PreferDevice,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.instances[0].frame_count, 2);
    assert_eq!(report.instances[0].metrics.routes.total_frames, 2);
    assert_eq!(report.instances[0].metrics.routes.cpu_input_frames, 2);
    assert_eq!(
        report.instances[0].metrics.routes.gpu_input_decode_frames,
        0
    );
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.cpu_input_frames, 2);
    assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
    assert!(report.metrics.timings.input_decode_micros > 0);
    assert!(report.metrics.timings.encode_micros > 0);
    assert!(report.metrics.timings.write_micros > 0);
    assert!(report.metrics.timings.compose_micros > 0);
    if report.metrics.routes.gpu_validation_frames == 0 {
        assert_eq!(report.metrics.timings.validation_micros, 0);
    } else {
        assert!(report.metrics.timings.validation_micros > 0);
    }
    assert_eq!(
        report.instances[0].transfer_syntax_uid,
        TransferSyntax::Jpeg2000Lossless.uid()
    );
    assert!(report.instances[0].path.starts_with(&out));

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax,
        TransferSyntax::Jpeg2000Lossless.uid()
    );
    assert_eq!(
        object
            .element(tags::SOP_CLASS_UID)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE
    );
    assert_eq!(
        object
            .element(tags::DIMENSION_ORGANIZATION_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "TILED_FULL"
    );
    assert!(object.element(tags::PYRAMID_UID).is_ok());
    assert_eq!(object.element(tags::PYRAMID_UID).unwrap().vr(), VR::UI);
    assert_eq!(object.element(tags::PYRAMID_LABEL).unwrap().vr(), VR::LO);
    assert_eq!(
        object.element(tags::FRAME_OF_REFERENCE_UID).unwrap().vr(),
        VR::UI
    );
    assert_eq!(
        object
            .element(tags::NUMBER_OF_FRAMES)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        2
    );
    assert_eq!(
        object
            .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        3
    );
    assert_eq!(
        object
            .element(tags::TOTAL_PIXEL_MATRIX_ROWS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        2
    );
    assert_eq!(object.element(tags::SERIES_NUMBER).unwrap().vr(), VR::IS);
    assert_eq!(object.element(tags::INSTANCE_NUMBER).unwrap().vr(), VR::IS);
    assert_eq!(
        object
            .element(tags::ACQUISITION_DATE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "19700101"
    );
    assert_eq!(
        object
            .element(tags::ACQUISITION_TIME)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "000000"
    );
    assert_eq!(
        object.element(tags::NUMBER_OF_OPTICAL_PATHS).unwrap().vr(),
        VR::UL
    );
    assert!(object.element(tags::EXTENDED_OFFSET_TABLE).is_ok());
    assert!(object.element(tags::EXTENDED_OFFSET_TABLE_LENGTHS).is_ok());
    assert_eq!(
        object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn export_dicom_refuses_existing_output_unless_overwrite_is_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let out = tmp.path().join("out");
    write_source_dicom(&source);

    let request = ExportRequest {
        source_path: source.clone(),
        output_dir: out.clone(),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000Lossless,
            encode_backend: EncodeBackendPreference::CpuOnly,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    };
    export_dicom(request.clone()).unwrap();

    let err = export_dicom(request.clone()).unwrap_err();
    assert!(
        err.to_string().contains("AlreadyExists") || err.to_string().contains("exists"),
        "unexpected error: {err}"
    );

    let mut overwrite_request = request;
    overwrite_request.options.overwrite = true;
    let report = export_dicom(overwrite_request).unwrap();
    assert_eq!(report.instances.len(), 1);
}

#[test]
fn external_openjpeg_decodes_jpeg2000_exported_frame_when_available() {
    let Some(opj_decompress) = find_command_for_test("opj_decompress") else {
        eprintln!("skipping external OpenJPEG parity smoke: opj_decompress not found");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let frame = write_external_j2k_decoder_frame_for_test(
        tmp.path(),
        "1.2.826.0.1.3680043.10.999.91",
        TransferSyntax::Jpeg2000Lossless,
    );

    let status = std::process::Command::new(opj_decompress)
        .args(["-i"])
        .arg(&frame.codestream_path)
        .args(["-o"])
        .arg(&frame.ppm_path)
        .status()
        .unwrap();
    assert!(status.success(), "opj_decompress failed with {status}");

    assert_external_decoder_ppm_matches_source_for_test(&frame.ppm_path, &frame.expected_pixels);
}

#[test]
fn external_dicom_validators_accept_jpeg_baseline_passthrough_when_available() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    run_dicom_validators_for_test(&report.instances[0].path);
}

#[test]
fn external_dicom_validators_accept_general_j2k_passthrough_when_available() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.svs");
    write_general_j2k_ycbcr_passthrough_tiff_for_test(&source, 17);

    let report = export_general_j2k_passthrough_for_test(source, tmp.path().join("out"));

    run_dicom_validators_for_test(&report.instances[0].path);
}

#[test]
fn external_dicom_validators_accept_htj2k_rpcl_when_available() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let out = tmp.path().join("out");
    write_source_dicom_with_pixels(
        &source,
        "1.2.826.0.1.3680043.10.999.94",
        3,
        2,
        vec![
            255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
        ],
    );

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 3,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    run_htj2k_dicom_validators_for_test(&report.instances[0].path);
}

#[test]
fn export_dicom_writes_htj2k_lossless_vl_wsi_instances() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let out = tmp.path().join("out");
    write_source_dicom(&source);

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out.clone(),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Htj2kLossless,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.cpu_input_frames, 2);
    assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 0);
    assert_eq!(report.metrics.routes.gpu_validation_frames, 0);
    assert_eq!(
        report.instances[0].transfer_syntax_uid,
        TransferSyntax::Htj2kLossless.uid()
    );

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax.trim_end_matches('\0'),
        TransferSyntax::Htj2kLossless.uid()
    );
    assert_eq!(
        object
            .element(tags::PHOTOMETRIC_INTERPRETATION)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "YBR_RCT"
    );
    assert_eq!(
        object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn export_dicom_tags_sibling_levels_as_one_pyramid_series() {
    let tmp = tempfile::tempdir().unwrap();
    let source_dir = tmp.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();
    let source_level0 = source_dir.join("level0.dcm");
    let source_level1 = source_dir.join("level1.dcm");
    let out = tmp.path().join("out");
    write_source_dicom_with_dimensions(&source_level0, "1.2.826.0.1.3680043.10.999.11", 4, 4);
    write_source_dicom_with_dimensions(&source_level1, "1.2.826.0.1.3680043.10.999.12", 2, 2);

    let report = export_dicom(ExportRequest {
        source_path: source_level0,
        output_dir: out,
        options: ExportOptions {
            tile_size: 2,
            encode_backend: EncodeBackendPreference::CpuOnly,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(report.instances.len(), 2);

    let level0 = dicom_object::open_file(&report.instances[0].path).unwrap();
    let level1 = dicom_object::open_file(&report.instances[1].path).unwrap();
    let series_uid = level0
        .element(tags::SERIES_INSTANCE_UID)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(
        level1
            .element(tags::SERIES_INSTANCE_UID)
            .unwrap()
            .to_str()
            .unwrap(),
        series_uid
    );
    let pyramid_uid = level0.element(tags::PYRAMID_UID).unwrap().to_str().unwrap();
    assert_eq!(
        level1.element(tags::PYRAMID_UID).unwrap().to_str().unwrap(),
        pyramid_uid
    );
    let frame_of_reference_uid = level0
        .element(tags::FRAME_OF_REFERENCE_UID)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(
        level1
            .element(tags::FRAME_OF_REFERENCE_UID)
            .unwrap()
            .to_str()
            .unwrap(),
        frame_of_reference_uid
    );
    assert_eq!(
        level0
            .element(tags::IMAGE_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "ORIGINAL\\PRIMARY\\VOLUME\\NONE"
    );
    assert_eq!(
        level1
            .element(tags::IMAGE_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED"
    );
    assert_eq!(
        level0
            .element(tags::INSTANCE_NUMBER)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        1
    );
    assert_eq!(
        level1
            .element(tags::INSTANCE_NUMBER)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        2
    );

    let slide = Slide::open(&report.instances[0].path).unwrap();
    let levels = &slide.dataset().scenes[0].series[0].levels;
    assert_eq!(levels.len(), 2);
    assert_eq!(levels[0].dimensions, (4, 4));
    assert_eq!(levels[1].dimensions, (2, 2));
}

#[test]
fn export_dicom_can_limit_to_single_pyramid_level() {
    let tmp = tempfile::tempdir().unwrap();
    let source_dir = tmp.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();
    let source_level0 = source_dir.join("level0.dcm");
    let source_level1 = source_dir.join("level1.dcm");
    write_source_dicom_with_dimensions(&source_level0, "1.2.826.0.1.3680043.10.999.21", 4, 4);
    write_source_dicom_with_dimensions(&source_level1, "1.2.826.0.1.3680043.10.999.22", 2, 2);

    let report = export_dicom(ExportRequest {
        source_path: source_level0,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 2,
            encode_backend: EncodeBackendPreference::CpuOnly,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(1),
    })
    .unwrap();

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.instances[0].level, 1);
    assert_eq!(report.instances[0].frame_count, 1);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object
            .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        2
    );
    assert_eq!(
        object
            .element(tags::IMAGE_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED"
    );
}

#[test]
fn export_dicom_jpeg_baseline_reencodes_non_passthrough_source() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom(&source);

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.instances[0].frame_count, 2);
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.cpu_input_frames, 2);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 0);
    assert_eq!(report.metrics.route_passthrough_frames(), 0);
    assert_eq!(report.metrics.routes.gpu_transcode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 2);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax.trim_end_matches('\0'),
        TransferSyntax::JpegBaseline8Bit.uid()
    );
    assert_eq!(
        object
            .element(tags::PHOTOMETRIC_INTERPRETATION)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "YBR_FULL_422"
    );
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 2);
    for fragment in fragments {
        let fragment = dicom_fragment_jpeg_payload(fragment);
        assert!(fragment.starts_with(&[0xFF, 0xD8]));
        assert!(fragment.ends_with(&[0xFF, 0xD9]));
        let decoder = j2k_jpeg::Decoder::new(fragment).unwrap();
        let (_rgb, outcome) = decoder
            .decode_request(j2k_jpeg::DecodeRequest::full(j2k_jpeg::PixelFormat::Rgb8))
            .unwrap();
        assert_eq!((outcome.decoded.w, outcome.decoded.h), (2, 2));
    }
}

#[test]
fn external_djpeg_decodes_jpeg_baseline_fallback_when_available() {
    let Some(djpeg) = find_command_for_test("djpeg") else {
        eprintln!("skipping external JPEG parity smoke: djpeg not found");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let out = tmp.path().join("out");
    let expected_pixel = [64u8, 128, 192];
    let expected = vec![expected_pixel; 4]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    write_source_dicom_with_pixels(
        &source,
        "1.2.826.0.1.3680043.10.999.92",
        2,
        2,
        expected.clone(),
    );

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();
    assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 1);
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 1);

    let jpeg_path = tmp.path().join("frame.jpg");
    let ppm_path = tmp.path().join("frame.ppm");
    std::fs::write(&jpeg_path, dicom_fragment_jpeg_payload(&fragments[0])).unwrap();
    let status = std::process::Command::new(djpeg)
        .args(["-outfile"])
        .arg(&ppm_path)
        .arg(&jpeg_path)
        .status()
        .unwrap();
    assert!(status.success(), "djpeg failed with {status}");

    let decoded = read_binary_ppm_for_test(&ppm_path);

    assert_eq!(decoded.0, 2);
    assert_eq!(decoded.1, 2);
    assert_eq!(decoded.2.len(), expected.len());
    for (actual, expected) in decoded.2.iter().zip(expected.iter()) {
        assert!(actual.abs_diff(*expected) <= 12);
    }
}

#[test]
fn jpeg_baseline_whole_level_pathological_strip_uses_requested_tile_geometry() {
    let level = wsi_rs::Level::new(
        (130, 31),
        1.0,
        TileLayout::WholeLevel {
            width: 130,
            height: 31,
            virtual_tile_width: 64,
            virtual_tile_height: 8,
        },
    );

    let geometry = jpeg_baseline_frame_geometry(&level, 512).unwrap();

    assert_eq!(geometry.frame_columns, 512);
    assert_eq!(geometry.frame_rows, 512);
    assert_eq!(geometry.tiles_across, 1);
    assert_eq!(geometry.tiles_down, 1);
}

#[test]
fn jpeg_baseline_regular_fallback_uses_requested_tile_geometry() {
    let level = wsi_rs::Level::new(
        (17, 9),
        1.0,
        TileLayout::Regular {
            tile_width: 8,
            tile_height: 8,
            tiles_across: 3,
            tiles_down: 2,
        },
    );

    let geometry = jpeg_baseline_frame_geometry(&level, 16).unwrap();

    assert_eq!(geometry.frame_columns, 16);
    assert_eq!(geometry.frame_rows, 16);
    assert_eq!(geometry.tiles_across, 2);
    assert_eq!(geometry.tiles_down, 1);
}

#[test]
fn jpeg_baseline_raw_passthrough_requires_jpeg_compression_and_matching_geometry() {
    let raw_j2k = RawCompressedTile::builder(Compression::Jp2kRgb)
        .dimensions(512, 512)
        .bits_allocated(8)
        .samples_per_pixel(3)
        .photometric_interpretation(EncodedTilePhotometricInterpretation::Rgb)
        .data(vec![0xFF, 0x4F])
        .build()
        .unwrap();
    let raw_jpeg = RawCompressedTile::builder(Compression::Jpeg)
        .dimensions(512, 512)
        .bits_allocated(8)
        .samples_per_pixel(3)
        .photometric_interpretation(EncodedTilePhotometricInterpretation::Rgb)
        .data(vec![0xFF, 0xD8])
        .build()
        .unwrap();

    assert!(!raw_jpeg_matches_frame_geometry(&raw_j2k, 512, 512));
    assert!(raw_jpeg_matches_frame_geometry(&raw_jpeg, 512, 512));
    assert!(!raw_jpeg_matches_frame_geometry(&raw_jpeg, 256, 512));
}
