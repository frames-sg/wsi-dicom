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
fn exported_jpeg_lossless_sv1_passes_dicom_validators() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.83", 32, 32);
    let report = export_jpeg_lossless(&source, tmp.path().join("out"), 16);
    let path = &report.instances[0].path;

    run_dicom_validators_for_test(path);
}
