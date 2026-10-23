use super::*;

fn export_jpeg_lossless(
    source: &std::path::Path,
    out: std::path::PathBuf,
    tile_size: u32,
) -> ExportReport {
    export_dicom(ExportRequest {
        source_path: source.to_path_buf(),
        output_dir: out,
        options: ExportOptions {
            tile_size,
            transfer_syntax: TransferSyntax::JpegLosslessSv1,
            encode_backend: EncodeBackendPreference::Auto,
            codec_validation: CodecValidation::RoundTrip,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap()
}

fn decode_jpeg_lossless_fragments(object: &dicom_object::DefaultDicomObject) -> Vec<Vec<u8>> {
    object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap()
        .iter()
        .map(|fragment| {
            let payload = dicom_fragment_jpeg_payload(fragment);
            let header = j2k_jpeg::Decoder::inspect(payload).unwrap();
            assert_eq!(header.sof_kind, j2k_jpeg::SofKind::Lossless);
            let (decoded, _) = j2k_jpeg::Decoder::new(payload)
                .unwrap()
                .decode_request(j2k_jpeg::DecodeRequest::full(j2k_jpeg::PixelFormat::Rgb8))
                .unwrap();
            decoded.as_slice().to_vec()
        })
        .collect()
}

fn frame_pixels(pixels: &[u8], width: u32, x0: u32, y0: u32, size: u32) -> Vec<u8> {
    (y0..y0 + size)
        .flat_map(|y| {
            let start = ((y * width + x0) * 3) as usize;
            pixels[start..start + (size * 3) as usize].iter().copied()
        })
        .collect()
}

#[test]
fn export_jpeg_lossless_sv1_reproduces_source_pixels_exactly() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let (width, height, tile) = (32u32, 16u32, 16u32);
    let pixels = crate::synthetic_source::deterministic_rgb_pixels(width, height).unwrap();
    write_source_dicom_with_pixels(
        &source,
        "1.2.826.0.1.3680043.10.999.81",
        width,
        height,
        pixels.clone(),
    );

    let report = export_jpeg_lossless(&source, tmp.path().join("out"), tile);

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.instances[0].frame_count, 2);
    assert_eq!(
        report.instances[0].transfer_syntax_uid,
        "1.2.840.10008.1.2.4.70"
    );
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.cpu_input_frames, 2);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 2);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 0);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax.trim_end_matches('\0'),
        TransferSyntax::JpegLosslessSv1.uid()
    );
    for (tag, expected) in [
        (tags::PHOTOMETRIC_INTERPRETATION, "RGB"),
        (tags::LOSSY_IMAGE_COMPRESSION, "00"),
    ] {
        assert_eq!(
            object.element(tag).unwrap().to_str().unwrap().trim(),
            expected
        );
    }
    assert_eq!(
        decode_jpeg_lossless_fragments(&object),
        [
            frame_pixels(&pixels, width, 0, 0, tile),
            frame_pixels(&pixels, width, tile, 0, tile),
        ]
    );
}

#[test]
fn export_jpeg_lossless_sv1_records_lossy_jpeg_source_history() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.svs");
    let tiles = [
        encode_test_jpeg(16, 16, [160, 20, 40]),
        encode_test_jpeg(16, 16, [20, 160, 40]),
    ];
    write_tiled_jpeg_tiff(&source, 32, 16, 16, 16, &tiles);

    let report = export_jpeg_lossless(&source, tmp.path().join("out"), 16);

    assert_eq!(report.instances[0].frame_count, 2);
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION)
            .unwrap()
            .to_str()
            .unwrap()
            .trim(),
        "01"
    );
    assert_eq!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION_METHOD)
            .unwrap()
            .to_str()
            .unwrap()
            .trim(),
        "ISO_10918_1",
        "JPEG Lossless re-encode must keep the source JPEG stage and add none"
    );
    let decoded = decode_jpeg_lossless_fragments(&object);
    for (frame, expected) in decoded.iter().zip([[160u8, 20, 40], [20, 160, 40]]) {
        for (actual, expected) in frame[..3].iter().zip(expected) {
            assert!(actual.abs_diff(expected) <= 4, "{actual} vs {expected}");
        }
    }
}

#[test]
fn sparse_exports_omit_absent_tiles_but_keep_acquired_black_tiles() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("sparse.tiff");
    let tiles = [
        Vec::new(),
        Vec::new(),
        encode_test_jpeg(8, 8, [160, 20, 40]),
        encode_test_jpeg(8, 8, [0, 0, 0]),
        Vec::new(),
        encode_test_jpeg(8, 8, [20, 40, 160]),
    ];
    write_tiled_jpeg_tiff(&source, 16, 24, 8, 8, &tiles);
    for (index, transfer_syntax) in [
        TransferSyntax::JpegBaseline8Bit,
        TransferSyntax::JpegLosslessSv1,
        TransferSyntax::Jpeg2000Lossless,
    ]
    .into_iter()
    .enumerate()
    {
        let report = export_dicom(ExportRequest {
            source_path: source.clone(),
            output_dir: tmp.path().join(format!("out-{index}")),
            options: ExportOptions {
                tile_size: 8,
                transfer_syntax,
                encode_backend: EncodeBackendPreference::CpuOnly,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: Some(0),
        })
        .unwrap();
        assert_eq!(report.instances[0].frame_count, 3, "{transfer_syntax:?}");
        assert_eq!(report.metrics.routes.total_frames, 3);
        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object
                .element(tags::DIMENSION_ORGANIZATION_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "TILED_SPARSE"
        );
        assert_eq!(frame_positions_for_test(&object), [(9, 1), (9, 9), (17, 9)]);
        // Required for TILED_FULL and kept for TILED_SPARSE.
        for tag in [
            tags::NUMBER_OF_OPTICAL_PATHS,
            tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES,
        ] {
            assert_eq!(object.element(tag).unwrap().to_int::<u32>().unwrap(), 1);
        }
        assert_eq!(
            object
                .element(tags::PIXEL_DATA)
                .unwrap()
                .value()
                .fragments()
                .unwrap()
                .len(),
            3
        );
        let original = wsi_rs::Slide::open(&source).unwrap();
        let written = wsi_rs::Slide::open(&report.instances[0].path).unwrap();
        assert_eq!(
            written
                .tile_is_present(&wsi_rs::TileRequest::new(0, 0, 0, 0, 0))
                .unwrap(),
            Some(false)
        );
        for (col, row) in [(0, 1), (1, 1), (1, 2)] {
            let request = wsi_rs::TileRequest::new(0, 0, 0, col, row);
            let expected = original.read_tile(&request).unwrap();
            let actual = written.read_tile(&request).unwrap();
            assert_eq!(actual.data().as_u8(), expected.data().as_u8());
        }
        run_sparse_dicom_validators_for_test(&report.instances[0].path);
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn sparse_jpeg_tiff_j2k_device_route_preserves_tiles_across_an_interior_gap() {
    if j2k_metal_support::system_default_device().is_err() {
        eprintln!("skipping sparse Metal export test: no Metal device");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("sparse-metal.tiff");
    let tiles = [
        Vec::new(),
        Vec::new(),
        Vec::new(),
        encode_test_jpeg(8, 8, [160, 20, 40]),
        Vec::new(),
        encode_test_jpeg(8, 8, [20, 40, 160]),
    ];
    write_tiled_jpeg_tiff(&source, 24, 16, 8, 8, &tiles);
    let original = wsi_rs::Slide::open(&source).unwrap();
    let expected: Vec<_> = [0, 2]
        .into_iter()
        .map(|col| {
            original
                .read_tile(&wsi_rs::TileRequest::new(0, 0, 0, col, 1))
                .unwrap()
                .data()
                .as_u8()
                .unwrap()
                .to_vec()
        })
        .collect();

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::Jpeg2000Lossless,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: true,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap();

    assert_eq!(report.instances[0].frame_count, 2);
    // Canonical JPEG sources use host decode with the requested Metal encoder.
    // The two noncontiguous frames must reach it as separate grid runs.
    assert_eq!(report.metrics.routes.cpu_input_frames, 2);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 2);
    assert_eq!(report.metrics.routes.gpu_encode_batches, 2);
    assert_eq!(report.metrics.routes.partial_gpu_transcode_frames, 2);
    assert_eq!(report.metrics.routes.resident_gpu_transcode_frames, 0);
    let written = wsi_rs::Slide::open(&report.instances[0].path).unwrap();
    for ((col, expected), frame_index) in [0, 2].into_iter().zip(expected).zip(0..) {
        let actual = written
            .read_tile(&wsi_rs::TileRequest::new(0, 0, 0, col, 1))
            .unwrap();
        let actual = actual.data().as_u8().unwrap();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                actual.abs_diff(expected) <= 4,
                "frame {frame_index} sample mismatch: {actual} vs {expected}"
            );
        }
    }
}

#[test]
fn export_jpeg_lossless_preserves_sparse_tiff_pixels_across_native_tiles() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("sparse.tiff");
    let tiles = [
        Vec::new(),
        encode_test_jpeg(8, 8, [160, 20, 40]),
        encode_test_jpeg(8, 8, [20, 160, 40]),
        encode_test_jpeg(8, 8, [20, 40, 160]),
    ];
    write_tiled_jpeg_tiff(&source, 16, 16, 8, 8, &tiles);
    let mut expected = vec![0; 32 * 32 * 3];
    for (index, jpeg) in tiles.iter().enumerate().skip(1) {
        let (decoded, _) = j2k_jpeg::Decoder::new(jpeg)
            .unwrap()
            .decode_request(j2k_jpeg::DecodeRequest::full(j2k_jpeg::PixelFormat::Rgb8))
            .unwrap();
        for row in 0..8 {
            let offset = ((index / 2 * 8 + row) * 32 + index % 2 * 8) * 3;
            expected[offset..offset + 8 * 3]
                .copy_from_slice(&decoded.as_slice()[row * 8 * 3..(row + 1) * 8 * 3]);
        }
    }

    let report = export_jpeg_lossless(&source, tmp.path().join("out"), 32);

    assert_eq!(report.instances[0].frame_count, 1);
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(decode_jpeg_lossless_fragments(&object), [expected]);
}

#[test]
fn export_jpeg_lossless_sv1_rejects_require_device() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.82", 16, 16);

    let error = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 16,
            transfer_syntax: TransferSyntax::JpegLosslessSv1,
            encode_backend: EncodeBackendPreference::RequireDevice,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap_err();

    assert!(matches!(error, Error::InvalidOptions { .. }), "{error}");
    assert!(
        !tmp.path().join("out").exists()
            || std::fs::read_dir(tmp.path().join("out"))
                .unwrap()
                .next()
                .is_none()
    );
}

#[test]
fn export_jpeg_lossless_sv1_preserves_16_bit_grayscale_samples() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.tiff");
    let samples: Vec<u16> = (0..16u16 * 16)
        .map(|i| i.wrapping_mul(4_099) ^ 0x8001)
        .collect();
    let tile: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    crate::test_support::write_tiled_uncompressed_gray16_tiff(&source, 16, 16, 16, 16, &[tile]);

    let report = export_jpeg_lossless(&source, tmp.path().join("out"), 16);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    for (tag, expected) in [
        (tags::PHOTOMETRIC_INTERPRETATION, "MONOCHROME2"),
        (tags::BITS_STORED, "16"),
    ] {
        assert_eq!(
            object.element(tag).unwrap().to_str().unwrap().trim(),
            expected
        );
    }
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    let payload = dicom_fragment_jpeg_payload(&fragments[0]);
    let (decoded, _) = j2k_jpeg::Decoder::new(payload)
        .unwrap()
        .decode_request(j2k_jpeg::DecodeRequest::full(j2k_jpeg::PixelFormat::Gray16))
        .unwrap();
    let (pairs, _) = decoded.as_slice().as_chunks::<2>();
    let decoded: Vec<u16> = pairs.iter().map(|pair| u16::from_ne_bytes(*pair)).collect();
    assert_eq!(decoded, samples);
}

#[test]
fn sixteen_bit_dicom_pyramid_round_trips_through_lossless_export() {
    use dicom_core::{DataElement, PrimitiveValue};
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let mut levels = Vec::new();
    for (level, size) in [4_u32, 2].into_iter().enumerate() {
        let path = source.join(format!("level-{level}.dcm"));
        write_source_dicom_with_dimensions(
            &path,
            &format!("1.2.826.0.1.3680043.10.999.94{level}"),
            size,
            size,
        );
        let values: Vec<u16> = (0..size * size * 3)
            .map(|index| (index as u16).wrapping_mul(4099) ^ 0x8001)
            .collect();
        let mut object = dicom_object::open_file(&path).unwrap();
        for (tag, value) in [
            (tags::BITS_ALLOCATED, 16_u16),
            (tags::BITS_STORED, 16),
            (tags::HIGH_BIT, 15),
        ] {
            object.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
        }
        object.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OW,
            PrimitiveValue::U16(values.clone().into()),
        ));
        object.write_to_file(&path).unwrap();
        levels.push((size, values));
    }
    for (index, transfer_syntax) in [
        TransferSyntax::JpegLosslessSv1,
        TransferSyntax::Jpeg2000Lossless,
    ]
    .into_iter()
    .enumerate()
    {
        let report = export_dicom(ExportRequest {
            source_path: source.clone(),
            output_dir: tmp.path().join(format!("out-{index}")),
            options: ExportOptions {
                tile_size: 2,
                transfer_syntax,
                encode_backend: EncodeBackendPreference::CpuOnly,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();
        assert_eq!(report.instances.len(), 2);
        let output = wsi_rs::Slide::open(&report.output_dir).unwrap();
        for (level, (size, values)) in levels.iter().enumerate() {
            for row in 0..size / 2 {
                for col in 0..size / 2 {
                    let tile = output
                        .read_tile(&wsi_rs::TileRequest::new(
                            0,
                            0,
                            level as u32,
                            i64::from(col),
                            i64::from(row),
                        ))
                        .unwrap();
                    let expected: Vec<u16> = (0..2)
                        .flat_map(|y| {
                            let start = (((row * 2 + y) * size + col * 2) * 3) as usize;
                            values[start..start + 6].iter().copied()
                        })
                        .collect();
                    assert_eq!(tile.data().as_u16(), Some(expected.as_slice()));
                }
            }
        }
    }
}

#[test]
fn exported_jpeg_lossless_sv1_passes_dicom_validators() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.83", 32, 32);
    let report = export_jpeg_lossless(&source, tmp.path().join("out"), 16);
    let path = &report.instances[0].path;

    run_dicom_validators_for_test(path);
}
