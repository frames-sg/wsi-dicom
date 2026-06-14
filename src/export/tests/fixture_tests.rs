use super::*;

#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE"]
fn export_dicom_can_export_source_synthetic_downsample_level() {
    let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let slide = Slide::open(&source).unwrap();
    let series = &slide.dataset().scenes[0].series[0];
    let (level_idx, level) = series
        .levels
        .iter()
        .enumerate()
        .rev()
        .find(|(level_idx, _)| {
            slide
                .level_source_kind(0, 0, *level_idx as u32)
                .is_ok_and(|kind| kind == LevelSourceKind::SyntheticDownsample)
        })
        .expect("HE.ndpi fixture should expose at least one synthetic overview level");
    let level_idx = level_idx as u32;

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 1024,
            transfer_syntax: TransferSyntax::Htj2kLossless,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(level_idx),
    })
    .unwrap();

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.instances[0].level, level_idx);
    assert_eq!(report.instances[0].frame_count, 1);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object
            .element(tags::IMAGE_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED"
    );
    assert_eq!(
        object
            .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        level.dimensions.0 as u32
    );
    assert_eq!(
        object
            .element(tags::TOTAL_PIXEL_MATRIX_ROWS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        level.dimensions.1 as u32
    );
}

#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE and Metal"]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn export_dicom_requires_device_encode_for_synthetic_level_with_cpu_source_input() {
    let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
        return;
    };
    if metal::Device::system_default().is_none() {
        eprintln!("skipping synthetic level device export test; Metal is unavailable");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let slide = Slide::open(&source).unwrap();
    let level_idx = slide.dataset().scenes[0].series[0]
        .levels
        .iter()
        .enumerate()
        .rev()
        .find_map(|(level_idx, _)| {
            slide
                .level_source_kind(0, 0, level_idx as u32)
                .is_ok_and(|kind| kind == LevelSourceKind::SyntheticDownsample)
                .then_some(level_idx as u32)
        })
        .expect("HE.ndpi fixture should expose at least one synthetic overview level");

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 1024,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: true,
            j2k_decomposition_levels: Some(1),
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(level_idx),
    })
    .unwrap();

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.cpu_input_frames, 1);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 1);
    assert_eq!(report.metrics.routes.partial_gpu_transcode_frames, 1);
    assert_eq!(report.metrics.routes.resident_gpu_transcode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);
}

#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE"]
fn ndpi_fixture_htj2k_profile_uses_retile_direct_53_not_rgb_fallback() {
    let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
        return;
    };

    let report = profile_dicom_routes(RouteProfileRequest {
        source_path: source,
        options: ExportOptions {
            tile_size: 1024,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        level: 0,
        max_frames: 2,
    })
    .unwrap();

    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.route_passthrough_frames(), 0);
    assert_eq!(report.metrics.routes.j2k_passthrough_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_retile_frames, 2);
    assert_eq!(report.metrics.routes.jpeg_retile_to_htj2k_53_frames, 2);
    assert_eq!(
        report.metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames,
        2
    );
    assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_cpu_encode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_input_frames, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
}

#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE"]
fn ndpi_fixture_exports_all_lossless_j2k_transfer_syntaxes_and_tile_sizes() {
    let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
        return;
    };
    let slide = Slide::open(&source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let (matrix_columns, matrix_rows) = level.dimensions;
    assert!(matrix_columns > 0);
    assert!(matrix_rows > 0);

    for tile_size in [512, 1024, 2048] {
        let tile_size_u64 = u64::from(tile_size);
        let x = ((matrix_columns - 1) / tile_size_u64) * tile_size_u64;
        let y = ((matrix_rows - 1) / tile_size_u64) * tile_size_u64;
        let width = (matrix_columns - x).min(tile_size_u64) as u32;
        let height = (matrix_rows - y).min(tile_size_u64) as u32;
        let region = slide
            .read_region(&RegionRequest {
                scene: SceneId(0),
                series: SeriesId(0),
                level: LevelIdx(0),
                plane: PlaneIdx(PlaneSelection { z: 0, c: 0, t: 0 }),
                origin_px: (x as i64, y as i64),
                size_px: (width, height),
            })
            .unwrap();
        let prepared = prepare_tile_samples(&region, tile_size, tile_size).unwrap();
        let samples = J2kLosslessSamples::new(
            &prepared.bytes,
            tile_size,
            tile_size,
            prepared.profile.components,
            prepared.profile.bits_allocated as u8,
            false,
        )
        .unwrap();

        for transfer_syntax in [
            TransferSyntax::Jpeg2000Lossless,
            TransferSyntax::Htj2kLossless,
            TransferSyntax::Htj2kLosslessRpcl,
        ] {
            let codestream = encode_dicom_lossless(
                samples,
                transfer_syntax,
                EncodeBackendPreference::RequireDevice,
                CodecValidation::RoundTrip,
            )
            .unwrap();
            assert_transfer_syntax_codestream(transfer_syntax, &codestream);
            assert_j2k_facade_roundtrip(samples, &codestream);
        }
    }
}

#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE"]
fn ndpi_fixture_exports_full_jpeg_baseline_passthrough_instance() {
    let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
        return;
    };
    let output_dir_env = std::env::var_os("WSI_DICOM_NDPI_JPEG_OUT")
        .or_else(|| std::env::var_os("WSI_DICOM_NDPI_LEVEL3_JPEG_OUT"))
        .map(PathBuf::from);
    let temp_dir = output_dir_env
        .is_none()
        .then(|| tempfile::tempdir().unwrap());
    let output_dir =
        output_dir_env.unwrap_or_else(|| temp_dir.as_ref().unwrap().path().to_path_buf());
    std::fs::create_dir_all(&output_dir).unwrap();

    let request = ExportRequest {
        source_path: source.clone(),
        output_dir: output_dir.clone(),
        options: ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    };
    let metadata = request.metadata.resolve().unwrap();
    let study_uid = metadata
        .study_instance_uid
        .clone()
        .unwrap_or_else(|| uid_from_seed(&format!("study:{}", source.display())));
    let slide = Slide::open(&source).unwrap();
    let (level_idx, geometry) = ndpi_jpeg_passthrough_level(&slide, request.options.tile_size);
    let level = &slide.dataset().scenes[0].series[0].levels[level_idx];
    let expected_frames = geometry.tiles_across * geometry.tiles_down;

    let report = export_jpeg_passthrough_instance(
        &slide,
        &request,
        &metadata,
        &study_uid,
        1,
        0,
        0,
        level_idx as u32,
        0,
        0,
        0,
        level,
    )
    .unwrap();

    assert_eq!(report.level, level_idx as u32);
    assert_eq!(report.frame_count, expected_frames as u32);
    assert_eq!(report.metrics.routes.total_frames, expected_frames);
    assert_eq!(
        report.metrics.routes.jpeg_passthrough_frames,
        expected_frames
    );
    assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_cpu_encode_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_metal_encode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_input_frames, 0);
    assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 0);

    let object = dicom_object::open_file(&report.path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax.trim_end_matches('\0'),
        TransferSyntax::JpegBaseline8Bit.uid()
    );
    assert_eq!(
        object.element(tags::ROWS).unwrap().to_int::<u32>().unwrap(),
        geometry.frame_rows
    );
    assert_eq!(
        object
            .element(tags::COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        geometry.frame_columns
    );
    assert_eq!(
        object
            .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        level.dimensions.0 as u32
    );
    assert_eq!(
        object
            .element(tags::TOTAL_PIXEL_MATRIX_ROWS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        level.dimensions.1 as u32
    );
    assert_eq!(
        object
            .element(tags::NUMBER_OF_FRAMES)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        expected_frames as u32
    );
    assert_eq!(
        object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap()
            .len(),
        expected_frames as usize
    );
}

#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE"]
fn ndpi_fixture_exports_jpeg_baseline_passthrough_pyramid_subset_for_qupath() {
    let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
        return;
    };
    let output_dir = std::env::var_os("WSI_DICOM_NDPI_PYRAMID_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| tempfile::tempdir().unwrap().keep());
    std::fs::create_dir_all(&output_dir).unwrap();

    let request = ExportRequest {
        source_path: source.clone(),
        output_dir,
        options: ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    };
    let metadata = request.metadata.resolve().unwrap();
    let study_uid = metadata
        .study_instance_uid
        .clone()
        .unwrap_or_else(|| uid_from_seed(&format!("study:{}", source.display())));
    let slide = Slide::open(&source).unwrap();
    let levels = ndpi_jpeg_passthrough_levels(&slide, request.options.tile_size);
    assert!(
        levels.len() >= 2,
        "NDPI fixture must expose at least two JPEG passthrough levels for pyramid testing"
    );

    let mut reports = Vec::with_capacity(levels.len());
    for (instance_idx, (level_idx, geometry)) in levels.iter().copied().enumerate() {
        let level = &slide.dataset().scenes[0].series[0].levels[level_idx];
        let expected_frames = geometry.tiles_across * geometry.tiles_down;
        let report = export_jpeg_passthrough_instance(
            &slide,
            &request,
            &metadata,
            &study_uid,
            (instance_idx + 1) as u32,
            0,
            0,
            level_idx as u32,
            0,
            0,
            0,
            level,
        )
        .unwrap();

        assert_eq!(report.metrics.routes.total_frames, expected_frames);
        assert_eq!(
            report.metrics.routes.jpeg_passthrough_frames,
            expected_frames
        );
        assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
        assert_eq!(report.metrics.routes.jpeg_cpu_encode_frames, 0);
        assert_eq!(report.metrics.routes.cpu_input_frames, 0);
        reports.push(report);
    }

    let first = dicom_object::open_file(&reports[0].path).unwrap();
    let series_uid = first
        .element(tags::SERIES_INSTANCE_UID)
        .unwrap()
        .to_str()
        .unwrap();
    let pyramid_uid = first.element(tags::PYRAMID_UID).unwrap().to_str().unwrap();
    for report in &reports[1..] {
        let object = dicom_object::open_file(&report.path).unwrap();
        assert_eq!(
            object
                .element(tags::SERIES_INSTANCE_UID)
                .unwrap()
                .to_str()
                .unwrap(),
            series_uid
        );
        assert_eq!(
            object.element(tags::PYRAMID_UID).unwrap().to_str().unwrap(),
            pyramid_uid
        );
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
#[ignore = "requires WSI_DICOM_METAL_INPUT_FIXTURE"]
fn fixture_first_mappable_tiles_use_batched_statumen_metal_input_decode_and_metal_encode() {
    let Some(source) = std::env::var_os("WSI_DICOM_METAL_INPUT_FIXTURE").map(PathBuf::from) else {
        return;
    };
    std::env::set_var("STATUMEN_JPEG_DEVICE_DECODE", "1");
    std::env::set_var("STATUMEN_JP2K_DEVICE_DECODE", "1");

    let slide = Slide::open(&source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let tile_size = match level.tile_layout {
        TileLayout::Regular {
            tile_width,
            tile_height,
            ..
        } => {
            assert_eq!(tile_width, tile_height);
            tile_width
        }
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } if virtual_tile_width == virtual_tile_height => virtual_tile_width,
        TileLayout::WholeLevel { .. } => 512,
        _ => {
            panic!("fixture first level must use a mappable Regular or WholeLevel tile layout")
        }
    };
    let tiles_across = level.dimensions.0.div_ceil(u64::from(tile_size));
    let tile_count = tiles_across.min(2);
    assert!(tile_count > 0);

    let mut metal_input = MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Jpeg2000Lossless,
        CodecValidation::RoundTrip,
    );
    let encoded = try_encode_metal_input_tile_run(
        &slide,
        &mut metal_input,
        &mut encoder,
        level,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        tile_count,
        level.dimensions.0,
        level.dimensions.1,
        tile_size,
    )
    .unwrap();

    assert_eq!(encoded.tiles.len(), tile_count as usize);
    assert!(encoded.input_decode_duration > Duration::ZERO);
    for frame in encoded.tiles {
        let frame = frame.expect("fixture tile should decode and encode on Metal");
        assert!(frame.0.used_device_encode);
        assert!(frame.0.used_device_validation);
        assert_transfer_syntax_codestream(
            TransferSyntax::Jpeg2000Lossless,
            frame.0.codestream_bytes().expect("codestream bytes"),
        );
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
#[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE and Metal JP2K device decode"]
fn aperio_jp2k_aligned_metal_input_256_htj2k_rpcl_tile_matches_cpu() {
    assert_aperio_jp2k_metal_input_tile_matches_cpu(256);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
#[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE and Metal JP2K device decode"]
fn aperio_jp2k_regular_tiled_metal_input_composes_512_htj2k_rpcl_tile_matches_cpu() {
    assert_aperio_jp2k_metal_input_tile_matches_cpu(512);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn require_device_source_tile_preference_rejects_cpu_decode_fallback() {
    let mut metal_input = MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
    let Ok(output) = metal_input.source_tile_output_preference() else {
        return;
    };

    assert!(output.requires_device());
    assert!(output.compressed_device_decode_enabled());
}

#[test]
#[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE"]
fn real_aperio_jp2k_problem_tile_round_trips() {
    let Some(source) = std::env::var_os("WSI_DICOM_APERIO_JP2K_FIXTURE").map(PathBuf::from) else {
        return;
    };
    let slide = Slide::open(&source).unwrap();
    let region = slide
        .read_region(&RegionRequest {
            scene: SceneId(0),
            series: SeriesId(0),
            level: LevelIdx(0),
            plane: PlaneIdx(PlaneSelection { z: 0, c: 0, t: 0 }),
            origin_px: (24 * 512, 12 * 512),
            size_px: (512, 512),
        })
        .unwrap();
    let prepared = prepare_tile_samples(&region, 512, 512).unwrap();
    let samples = J2kLosslessSamples::new(
        &prepared.bytes,
        512,
        512,
        prepared.profile.components,
        prepared.profile.bits_allocated as u8,
        false,
    )
    .unwrap();

    let tile_out = std::env::var_os("WSI_DICOM_APERIO_JP2K_TILE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/aperio-jp2k-problem-tile.rgb"));
    if let Some(parent) = tile_out.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(tile_out, &prepared.bytes).unwrap();
    encode_dicom_j2k_lossless(samples, EncodeBackendPreference::CpuOnly).unwrap();
}

#[test]
#[ignore = "requires WSI_DICOM_EXPORT_DIR"]
fn exported_aperio_jp2k_dicom_instances_read_back() {
    let Some(output_dir) = std::env::var_os("WSI_DICOM_EXPORT_DIR").map(PathBuf::from) else {
        return;
    };
    let expected = [
        (
            "level-0000-z0000-c0000-t0000.dcm",
            15374u32,
            17497u32,
            1085u32,
        ),
        ("level-0001-z0000-c0000-t0000.dcm", 3843u32, 4374u32, 72u32),
        ("level-0002-z0000-c0000-t0000.dcm", 1921u32, 2187u32, 20u32),
    ];

    for (file_name, columns, rows, frames) in expected {
        let object = dicom_object::open_file(output_dir.join(file_name)).unwrap();
        assert_eq!(
            object.meta().media_storage_sop_class_uid,
            uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE
        );
        assert_eq!(object.meta().transfer_syntax, uids::JPEG2000_LOSSLESS);
        assert_eq!(
            object
                .element(tags::SOP_CLASS_UID)
                .unwrap()
                .to_str()
                .unwrap(),
            uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE
        );
        assert_eq!(
            object
                .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            columns
        );
        assert_eq!(
            object
                .element(tags::TOTAL_PIXEL_MATRIX_ROWS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            rows
        );
        assert_eq!(
            object
                .element(tags::NUMBER_OF_FRAMES)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            frames
        );
        assert_eq!(
            object
                .element(tags::PIXEL_DATA)
                .unwrap()
                .value()
                .fragments()
                .unwrap()
                .len(),
            frames as usize
        );
    }
}

#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE and Metal device decode"]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn ndpi_whole_level_metal_rows_do_not_turn_black_after_reused_encoder_state() {
    let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
        return;
    };
    std::env::set_var("STATUMEN_JPEG_DEVICE_DECODE", "1");
    let slide = Slide::open(&source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let (matrix_columns, matrix_rows) = level.dimensions;
    let tile_size = 512u32;
    let tiles_across = matrix_columns.div_ceil(u64::from(tile_size));
    let target_row = 12u64.min(matrix_rows.div_ceil(u64::from(tile_size)).saturating_sub(1));
    let target_col = 0u64;
    let mut metal_input = MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
    let mut j2k_encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Htj2kLossless,
        CodecValidation::RoundTrip,
    );

    let mut target = None;
    for row in 0..=target_row {
        let mut metal_row = try_encode_metal_input_tile_run(
            &slide,
            &mut metal_input,
            &mut j2k_encoder,
            level,
            0,
            0,
            0,
            0,
            0,
            0,
            row,
            0,
            tiles_across,
            matrix_columns,
            matrix_rows,
            tile_size,
        )
        .unwrap();
        if row == target_row {
            target = metal_row.tiles[target_col as usize].take();
        }
    }
    let (encoded, profile) = target.expect("fixture frame should encode through Metal input path");
    assert_eq!(profile.components, 3);
    assert!(encoded.used_device_encode);
    assert!(encoded.used_device_validation);

    let x = target_col * u64::from(tile_size);
    let y = target_row * u64::from(tile_size);
    let valid_width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
    let valid_height = (matrix_rows - y).min(u64::from(tile_size)) as u32;
    let cpu_region = slide
        .read_region(&RegionRequest {
            scene: SceneId(0),
            series: SeriesId(0),
            level: LevelIdx(0),
            plane: PlaneIdx(PlaneSelection { z: 0, c: 0, t: 0 }),
            origin_px: (x as i64, y as i64),
            size_px: (valid_width, valid_height),
        })
        .unwrap();
    let expected = prepare_tile_samples(&cpu_region, tile_size, tile_size).unwrap();
    let actual = decode_j2k_frame_for_test(
        encoded.codestream_bytes().expect("codestream bytes"),
        tile_size,
        tile_size,
        profile.components,
        profile.bits_allocated,
    );

    if actual != expected.bytes {
        let actual_nonzero = actual.iter().filter(|value| **value != 0).count();
        let expected_nonzero = expected.bytes.iter().filter(|value| **value != 0).count();
        panic!(
                "Metal WholeLevel frame mismatch at row {target_row}, col {target_col}: actual_nonzero={actual_nonzero}, expected_nonzero={expected_nonzero}, total={}",
                actual.len()
            );
    }
}

#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE and Metal device decode"]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn ndpi_whole_level_metal_composes_multi_tile_run_in_one_batch() {
    let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
        return;
    };
    std::env::set_var("STATUMEN_JPEG_DEVICE_DECODE", "1");
    let slide = Slide::open(&source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let Some(strip_layout) = whole_level_strip_layout(level) else {
        return;
    };
    let (matrix_columns, matrix_rows) = level.dimensions;
    let tile_size = 512u32;
    let tile_count = matrix_columns.div_ceil(u64::from(tile_size)).min(3);
    assert!(tile_count > 1);

    let mut metal_input = MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
    let mut j2k_encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Htj2kLossless,
        CodecValidation::RoundTrip,
    );

    let encoded = try_encode_metal_whole_level_strip_run(
        &slide,
        &mut metal_input,
        &mut j2k_encoder,
        strip_layout,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        tile_count as usize,
        matrix_columns,
        matrix_rows,
        tile_size,
    )
    .unwrap();

    assert_eq!(encoded.tiles.len(), tile_count as usize);
    assert_eq!(encoded.compose_batches, 1);
    assert!(encoded.compose_duration > Duration::ZERO);
    for frame in encoded.tiles {
        let (frame, _) = frame.expect("fixture frame should encode through Metal input path");
        assert!(frame.used_device_encode);
    }
}

#[test]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_strip_composer_returns_ordered_tiles_from_batched_compose() {
    let Some(device) = metal::Device::system_default() else {
        return;
    };
    let composer = MetalStripComposer::new(device.clone()).unwrap();
    let layout = WholeLevelStripLayout {
        width: 4,
        height: 4,
    };
    let source_a = [1u8; 16];
    let source_b = [2u8; 16];
    let tile_a = metal_test_tile(&device, &source_a, 4, 4, SigninumPixelFormat::Gray8);
    let tile_b = metal_test_tile(&device, &source_b, 4, 4, SigninumPixelFormat::Gray8);
    let packed = composer
        .pack_tiles(&[tile_a, tile_b], layout, 0, 0, 2)
        .expect("pack test tiles");

    let composed = composer
        .compose_tiles(
            &packed,
            &[
                MetalComposeTileRequest {
                    src_origin_x: 0,
                    src_origin_y: 0,
                    valid_width: 4,
                    valid_height: 4,
                    output_width: 4,
                    output_height: 4,
                },
                MetalComposeTileRequest {
                    src_origin_x: 4,
                    src_origin_y: 0,
                    valid_width: 4,
                    valid_height: 4,
                    output_width: 4,
                    output_height: 4,
                },
            ],
        )
        .expect("batched compose");

    assert_eq!(composed.len(), 2);
    assert_eq!(composed[0].width, 4);
    assert_eq!(composed[1].width, 4);
    assert_eq!(composed[0].height, 4);
    assert_eq!(composed[1].height, 4);
}

#[test]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn jpeg_baseline_metal_tile_entries_keep_full_tiles_when_edge_geometry_falls_back() {
    let Some(device) = metal::Device::system_default() else {
        return;
    };
    let full_a = metal_test_tile(&device, &[1u8; 16], 4, 4, SigninumPixelFormat::Gray8);
    let edge = metal_test_tile(&device, &[2u8; 12], 3, 4, SigninumPixelFormat::Gray8);
    let full_b = metal_test_tile(&device, &[3u8; 16], 4, 4, SigninumPixelFormat::Gray8);
    let frames = [
        JpegBaselineFallbackFrame {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        JpegBaselineFallbackFrame {
            x: 4,
            y: 0,
            width: 4,
            height: 4,
        },
        JpegBaselineFallbackFrame {
            x: 8,
            y: 0,
            width: 4,
            height: 4,
        },
    ];

    let entries = jpeg_baseline_metal_tile_entries(
        vec![
            TilePixels::Device(DeviceTile::Metal(full_a)),
            TilePixels::Device(DeviceTile::Metal(edge)),
            TilePixels::Device(DeviceTile::Metal(full_b)),
        ],
        &frames,
        EncodeBackendPreference::PreferDevice,
    )
    .unwrap();

    assert_eq!(entries.len(), 3);
    assert!(entries[0].is_some());
    assert!(entries[1].is_none());
    assert!(entries[2].is_some());
}
