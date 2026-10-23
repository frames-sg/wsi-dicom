use super::*;

#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE"]
fn export_dicom_can_export_source_synthetic_downsample_level() {
    let source = required_fixture_path("WSI_DICOM_NDPI_FIXTURE");
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
        color_management: ColorManagement::SourceOrSrgb,
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
    let source = required_fixture_path("WSI_DICOM_NDPI_FIXTURE");
    j2k_metal_support::system_default_device()
        .expect("the explicit synthetic-level encode test requires Metal");
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
        color_management: ColorManagement::SourceOrSrgb,
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

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE and Metal"]
fn ndpi_default_htj2k_export_rejects_implicit_color_reencoding() {
    const MAX_FRAMES: u64 = 256;

    let source = required_fixture_path("WSI_DICOM_NDPI_FIXTURE");
    j2k_metal_support::system_default_device()
        .expect("the explicit NDPI export test requires Metal");
    let tmp = tempfile::tempdir().unwrap();
    let slide = Slide::open(&source).unwrap();
    let options = ExportOptions {
        encode_backend: EncodeBackendPreference::RequireDevice,
        source_device_decode: false,
        pixel_reencode: PixelReencodePolicy::Never,
        ..ExportOptions::default()
    };
    assert_eq!(options.transfer_syntax, TransferSyntax::Htj2kLosslessRpcl);
    let tile_size = options.tile_size;
    let series = &slide.dataset().scenes[0].series[0];
    let (level_idx, _, _, _, _) = series
        .levels
        .iter()
        .enumerate()
        .filter_map(|(level_idx, level)| {
            if slide.level_source_kind(0, 0, level_idx as u32).ok()
                != Some(LevelSourceKind::Physical)
            {
                return None;
            }
            let TileLayout::WholeLevel { width, height, .. } = level.tile_layout else {
                return None;
            };
            if (width, height) != level.dimensions {
                return None;
            }
            let tiles_across = width.div_ceil(u64::from(tile_size));
            let tiles_down = height.div_ceil(u64::from(tile_size));
            let frame_count = tiles_across.checked_mul(tiles_down)?;
            (tiles_across >= 3
                && tiles_down >= 3
                && frame_count <= MAX_FRAMES
                && (width % u64::from(tile_size) != 0 || height % u64::from(tile_size) != 0))
                .then_some((level_idx, level, tiles_across, tiles_down, frame_count))
        })
        .min_by_key(|(level_idx, _, _, _, frame_count)| (*level_idx != 4, *frame_count))
        .expect("NDPI fixture should expose a modest physical whole-level pyramid level");
    let level_idx = level_idx as u32;
    let error = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options,
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(level_idx),
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("pixel re-encoding fallback is disabled"),
        "{error}"
    );
    assert!(error.contains("ProfileUnsupported"), "{error}");
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
#[ignore = "requires WSI_DICOM_NDPI_FIXTURE and Metal"]
fn ndpi_edge_tile_device_encode_round_trips_all_lossless_j2k_profiles() {
    let source = required_fixture_path("WSI_DICOM_NDPI_FIXTURE");
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
            .read_region(&RegionRequest::new(
                0usize,
                0usize,
                0u32,
                (x as i64, y as i64),
                (width, height),
            ))
            .unwrap();
        let prepared = prepare_tile_samples(&region, tile_size, tile_size).unwrap();
        let samples = J2kLosslessSamples::new(
            &prepared.bytes,
            tile_size,
            tile_size,
            u16::from(prepared.profile.components),
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
    let source = required_fixture_path("WSI_DICOM_NDPI_FIXTURE");
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
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    };
    let metadata = request.metadata.resolve().unwrap();
    let normalized_options = NormalizedExportOptions::from_validated(&request.options);
    let identity = DicomExportIdentity::for_export(
        &source,
        &normalized_options,
        &metadata,
        request.level_filter,
        &[],
    )
    .unwrap();
    let slide = Slide::open(&source).unwrap();
    let (level_idx, geometry) = ndpi_jpeg_passthrough_level(&slide, request.options.tile_size);
    let level = &slide.dataset().scenes[0].series[0].levels[level_idx];
    let expected_frames = geometry.tiles_across * geometry.tiles_down;

    let report = export_jpeg_passthrough_instance(
        &slide,
        &request,
        InstanceExportContext {
            options: &normalized_options,
            metadata: &metadata,
            identity: &identity,
            instance_number: 1,
            coordinate: InstanceCoordinate::new(0, 0, level_idx as u32, 0, 0, 0),
            level,
            per_frame_plan: None,
        },
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

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn reversible_jp2k_source_tiles_use_batched_metal_decode_and_encode() {
    const TILE_SIZE: u32 = 16;
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("reversible.svs");
    let pixels = [11u32, 19].map(|multiplier| {
        (0..TILE_SIZE * TILE_SIZE * 3)
            .map(|idx| ((idx * multiplier) & 0xff) as u8)
            .collect::<Vec<_>>()
    });
    let codestreams = pixels.each_ref().map(|pixels| {
        let samples = J2kLosslessSamples::new(pixels, TILE_SIZE, TILE_SIZE, 3, 8, false)
            .expect("RGB source samples");
        encode_dicom_j2k_lossless(samples, EncodeBackendPreference::CpuOnly).unwrap()
    });
    write_tiled_jp2k_rgb_tiff(
        &source,
        TILE_SIZE * 2,
        TILE_SIZE,
        TILE_SIZE,
        TILE_SIZE,
        &codestreams,
    );
    let slide = Slide::open(&source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[0];
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
        MetalInputTileRunRequest {
            level,
            location: JpegBaselineFrameLocation::first_series_level(0),
            row: 0,
            start_col: 0,
            tile_count: 2,
            matrix_columns: level.dimensions.0,
            matrix_rows: level.dimensions.1,
            tile_size: TILE_SIZE,
        },
    )
    .unwrap();

    assert!(
        encoded.used_gpu_input,
        "source samples must be decoded on Metal"
    );
    assert_eq!(encoded.input_decode_batches, 1);
    assert_eq!(encoded.encode_batches, 1);
    assert_eq!(encoded.tiles.len(), pixels.len());
    for (frame, expected) in encoded.tiles.into_iter().zip(pixels) {
        let (frame, profile) = frame.expect("Metal encoded source tile");
        assert!(frame.used_device_encode && frame.used_device_validation);
        assert_eq!(
            decode_j2k_frame_for_test(
                frame.codestream_bytes().unwrap().as_ref(),
                TILE_SIZE,
                TILE_SIZE,
                profile.components,
                profile.bits_allocated,
            ),
            expected
        );
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
#[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE and Metal encode"]
fn aperio_jp2k_device_encoded_256_htj2k_rpcl_tile_matches_cpu() {
    assert_aperio_jp2k_device_encoded_tile_matches_cpu(256);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
#[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE and Metal"]
fn lossy_jp2k_input_preserves_canonical_cpu_pixels_for_lossless_export() {
    let source = required_fixture_path("WSI_DICOM_APERIO_JP2K_FIXTURE");
    let slide = Slide::open(source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[2];
    let raw = slide
        .read_raw_compressed_tile(&wsi_rs::TileRequest::new(0, 0, 2, 0, 0))
        .expect("read the irreversible Aperio JPEG 2000 source tile");
    assert!(matches!(
        raw.compression(),
        Compression::Jp2kRgb | Compression::Jp2kYcbcr
    ));
    assert!(!J2kView::parse(raw.data())
        .unwrap()
        .passthrough_candidate()
        .expect("supported Aperio JPEG 2000 source")
        .transfer_syntax()
        .is_lossless());
    let mut reader = MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Htj2kLosslessRpcl,
        CodecValidation::RoundTrip,
    );
    let run = try_encode_metal_input_tile_run(
        &slide,
        &mut reader,
        &mut encoder,
        MetalInputTileRunRequest {
            level,
            location: JpegBaselineFrameLocation::first_series_level(2),
            row: 0,
            start_col: 0,
            tile_count: 1,
            matrix_columns: level.dimensions.0,
            matrix_rows: level.dimensions.1,
            tile_size: 256,
        },
    )
    .unwrap();
    assert!(
        !run.used_gpu_input,
        "irreversible JP2K uses canonical CPU reconstruction"
    );
    let (frame, _) = run.tiles.into_iter().next().unwrap().unwrap();
    assert!(frame.used_device_encode && frame.used_device_validation);
    let bytes = frame.into_codestream().unwrap();
    let actual = decode_j2k_frame_for_test(&bytes, 256, 256, 3, 8);
    let expected = slide
        .read_region(&RegionRequest::new(
            0usize,
            0usize,
            2u32,
            (0, 0),
            (256, 256),
        ))
        .unwrap();
    let expected = prepare_tile_samples(&expected, 256, 256).unwrap();
    assert_eq!(
        actual.as_slice(),
        expected.bytes.as_slice(),
        "lossless export must preserve canonical decoded source samples"
    );
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
#[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE and Metal encode"]
fn aperio_jp2k_device_encoded_composed_512_htj2k_rpcl_tile_matches_cpu() {
    assert_aperio_jp2k_device_encoded_tile_matches_cpu(512);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn require_device_source_tile_read_rejects_cpu_decode_fallback() {
    if j2k_metal_support::system_default_device().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("raw.dcm");
    write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.81", 4, 4);
    let slide = Slide::open(&source).unwrap();
    let requests = [wsi_rs::TileRequest::new(0, 0, 0, 0, 0)];
    assert_eq!(slide.read_tiles(&requests).unwrap().len(), 1);

    let mut metal_input = MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
    assert!(matches!(
        metal_input.read_source_tiles(&slide, &requests),
        Err(Error::SlideRead { .. })
    ));
    let mut disabled = MetalInputTileReader::new(EncodeBackendPreference::PreferDevice, false);
    assert!(matches!(
        disabled.read_source_tiles(&slide, &requests),
        Err(Error::Unsupported { .. })
    ));
}

#[test]
#[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE"]
fn real_aperio_jp2k_problem_tile_round_trips() {
    let source = required_fixture_path("WSI_DICOM_APERIO_JP2K_FIXTURE");
    let slide = Slide::open(&source).unwrap();
    let dimensions = slide.dataset().scenes[0].series[0].levels[0].dimensions;
    assert!(
        dimensions.0 >= 25 * 512 && dimensions.1 >= 13 * 512,
        "Aperio fixture must contain the complete problem region"
    );
    let region = slide
        .read_region(&RegionRequest::new(
            0usize,
            0usize,
            0u32,
            (24 * 512, 12 * 512),
            (512, 512),
        ))
        .unwrap();
    let prepared = prepare_tile_samples(&region, 512, 512).unwrap();
    let samples = J2kLosslessSamples::new(
        &prepared.bytes,
        512,
        512,
        u16::from(prepared.profile.components),
        prepared.profile.bits_allocated as u8,
        false,
    )
    .unwrap();

    encode_dicom_j2k_lossless(samples, EncodeBackendPreference::CpuOnly).unwrap();
}

#[test]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_strip_composer_returns_ordered_tiles_from_batched_compose() {
    let Ok(device) = j2k_metal_support::system_default_device() else {
        return;
    };
    let composer = MetalStripComposer::new(device.clone()).unwrap();
    let layout = WholeLevelStripLayout {
        width: 4,
        height: 4,
    };
    let source_a = [1u8; 16];
    let source_b = [2u8; 16];
    let tile_a = metal_test_tile(&device, &source_a, 4, 4, J2kPixelFormat::Gray8);
    let tile_b = metal_test_tile(&device, &source_b, 4, 4, J2kPixelFormat::Gray8);
    let packed = composer
        .pack_tiles(&[tile_a, tile_b], layout, 0, 0, 2)
        .expect("pack test tiles");

    assert_eq!(packed.byte_len, 32);
    assert_eq!(packed.slot_stride, 4);

    let composed = composer
        .compose_tiles(
            packed,
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
    assert!(matches!(
        composed[0].storage,
        wsi_rs::output::metal::MetalDeviceStorage::Resident { .. }
    ));
    assert!(matches!(
        composed[1].storage,
        wsi_rs::output::metal::MetalDeviceStorage::Resident { .. }
    ));
    assert_eq!(
        crate::metal_interop::test_tile_bytes(&composed[0]),
        source_a
    );
    assert_eq!(
        crate::metal_interop::test_tile_bytes(&composed[1]),
        source_b
    );
}

#[test]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn pending_metal_compose_feeds_same_queue_j2k_encode_without_host_handoff() {
    let Ok(device) = j2k_metal_support::system_default_device() else {
        return;
    };
    const TILE_SIZE: u32 = 64;
    let composer = MetalStripComposer::new(device.clone()).unwrap();
    let layout = WholeLevelStripLayout {
        width: TILE_SIZE,
        height: TILE_SIZE,
    };
    let source_a = vec![17u8; (TILE_SIZE * TILE_SIZE) as usize];
    let source_b = vec![231u8; (TILE_SIZE * TILE_SIZE) as usize];
    let tile_a = metal_test_tile(
        &device,
        &source_a,
        TILE_SIZE,
        TILE_SIZE,
        J2kPixelFormat::Gray8,
    );
    let tile_b = metal_test_tile(
        &device,
        &source_b,
        TILE_SIZE,
        TILE_SIZE,
        J2kPixelFormat::Gray8,
    );
    let packed = composer
        .pack_tiles(&[tile_a, tile_b], layout, 0, 0, 2)
        .expect("pack pending compose inputs");
    let requests = [
        MetalComposeTileRequest {
            src_origin_x: 0,
            src_origin_y: 0,
            valid_width: TILE_SIZE,
            valid_height: TILE_SIZE,
            output_width: TILE_SIZE,
            output_height: TILE_SIZE,
        },
        MetalComposeTileRequest {
            src_origin_x: TILE_SIZE,
            src_origin_y: 0,
            valid_width: TILE_SIZE,
            valid_height: TILE_SIZE,
            output_width: TILE_SIZE,
            output_height: TILE_SIZE,
        },
    ];
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Htj2kLossless,
        CodecValidation::Disabled,
    );
    assert!(encoder
        .ensure_metal_session_for_command_queue(device, composer.command_queue())
        .expect("bind encoder to compose queue"));
    let pending = composer
        .submit_compose_tiles(packed, &requests, composer.command_queue())
        .expect("submit pending compose");
    let encoded = pending
        .submit(&mut encoder, TILE_SIZE, TILE_SIZE)
        .expect("submit same-queue encode")
        .wait()
        .expect("complete same-queue compose and encode");

    let actual = encoded
        .frames
        .into_iter()
        .map(|frame| {
            let frame = frame.expect("device encoded frame");
            assert!(frame.used_device_encode);
            decode_j2k_frame_for_test(
                frame.codestream_bytes().unwrap().as_ref(),
                TILE_SIZE,
                TILE_SIZE,
                1,
                8,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, vec![source_a, source_b]);
}

#[test]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_strip_composer_accepts_explicit_completed_buffer_tiles() {
    let Ok(device) = j2k_metal_support::system_default_device() else {
        return;
    };
    let composer = MetalStripComposer::new(device.clone()).unwrap();
    let source = [7u8; 16];
    let tile = metal_test_tile(&device, &source, 4, 4, J2kPixelFormat::Gray8);

    let packed = composer
        .pack_tiles(
            &[tile],
            WholeLevelStripLayout {
                width: 4,
                height: 4,
            },
            0,
            0,
            1,
        )
        .expect("adopted completed buffer must pack");

    assert_eq!(packed.byte_len, 16);
    let composed = composer
        .compose_tiles(
            packed,
            &[MetalComposeTileRequest {
                src_origin_x: 0,
                src_origin_y: 0,
                valid_width: 4,
                valid_height: 4,
                output_width: 4,
                output_height: 4,
            }],
        )
        .unwrap();
    assert_eq!(crate::metal_interop::test_tile_bytes(&composed[0]), source);
}

#[test]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_strip_composer_preserves_pitched_u16_input_and_zero_padding() {
    let device = j2k_metal_support::system_default_device().unwrap();
    let composer = MetalStripComposer::new(device.clone()).unwrap();
    // A nonzero origin and padding bytes must never enter the output pixels.
    let bytes = [99u8, 99, 1, 2, 3, 4, 99, 99, 5, 6, 7, 8, 99, 99];
    let buffer = j2k_metal_support::checked_shared_buffer_with_slice(&device, &bytes).unwrap();
    let tile = crate::metal_interop::test_tile_from_completed_buffer(
        buffer,
        2,
        2,
        2,
        6,
        wsi_rs::PixelFormat::Gray16,
    );
    let packed = composer
        .pack_tiles(
            &[tile],
            WholeLevelStripLayout {
                width: 3,
                height: 2,
            },
            0,
            0,
            1,
        )
        .unwrap();
    let composed = composer
        .compose_tiles(
            packed,
            &[MetalComposeTileRequest {
                src_origin_x: 0,
                src_origin_y: 0,
                valid_width: 2,
                valid_height: 2,
                output_width: 3,
                output_height: 3,
            }],
        )
        .unwrap();
    assert_eq!(
        crate::metal_interop::test_tile_bytes(&composed[0]),
        [1, 2, 3, 4, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_strip_composer_rejects_metadata_mismatch_and_out_of_grid_reads() {
    let Ok(device) = j2k_metal_support::system_default_device() else {
        return;
    };
    let composer = MetalStripComposer::new(device.clone()).unwrap();
    let source = [13u8; 16];
    let mut mismatched = metal_test_tile(&device, &source, 4, 4, J2kPixelFormat::Gray8);
    mismatched.pitch_bytes += 1;
    let error = match composer.pack_tiles(
        &[mismatched],
        WholeLevelStripLayout {
            width: 4,
            height: 4,
        },
        0,
        0,
        1,
    ) {
        Ok(_) => panic!("resident metadata mismatch must be rejected"),
        Err(error) => error,
    };
    assert!(matches!(&error, Error::Unsupported { .. }));
    assert!(error.to_string().contains("metadata"));

    let packed = composer
        .pack_tiles(
            &[metal_test_tile(
                &device,
                &source,
                4,
                4,
                J2kPixelFormat::Gray8,
            )],
            WholeLevelStripLayout {
                width: 4,
                height: 4,
            },
            0,
            0,
            1,
        )
        .expect("pack resident source");
    let error = composer
        .compose_tiles(
            packed,
            &[MetalComposeTileRequest {
                src_origin_x: 4,
                src_origin_y: 0,
                valid_width: 4,
                valid_height: 4,
                output_width: 4,
                output_height: 4,
            }],
        )
        .expect_err("out-of-grid source read must be rejected before submission");
    assert!(matches!(&error, Error::Unsupported { .. }));
    assert!(error.to_string().contains("outside the packed source"));
}

#[test]
#[cfg(all(feature = "metal", target_os = "macos"))]
fn jpeg_baseline_metal_tile_entries_reject_changed_geometry() {
    let Ok(device) = j2k_metal_support::system_default_device() else {
        return;
    };
    let full_a = metal_test_tile(&device, &[1u8; 16], 4, 4, J2kPixelFormat::Gray8);
    let edge = metal_test_tile(&device, &[2u8; 12], 3, 4, J2kPixelFormat::Gray8);
    let full_b = metal_test_tile(&device, &[3u8; 16], 4, 4, J2kPixelFormat::Gray8);
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

    let error = jpeg_baseline_metal_tile_entries(vec![full_a, edge, full_b], &frames)
        .expect_err("changed input geometry must not select CPU fallback");
    assert!(error.to_string().contains("input geometry changed"));
}
