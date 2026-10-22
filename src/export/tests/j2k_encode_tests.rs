use super::*;
use crate::test_support::j2k_cod_marker_offset;

fn raw_compressed_tile(
    compression: Compression,
    width: u32,
    height: u32,
    bits_allocated: u16,
    samples_per_pixel: u16,
    photometric_interpretation: EncodedTilePhotometricInterpretation,
    data: Vec<u8>,
) -> RawCompressedTile {
    RawCompressedTile::builder(compression)
        .dimensions(width, height)
        .bits_allocated(bits_allocated)
        .samples_per_pixel(samples_per_pixel)
        .photometric_interpretation(photometric_interpretation)
        .data(data)
        .build()
        .expect("valid raw compressed tile")
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn require_device_uses_metal_j2k_encode_for_wsi_sized_tile() {
    let mut bytes = Vec::with_capacity(128 * 128 * 3);
    for y in 0..128u32 {
        for x in 0..128u32 {
            bytes.push(((x * 3 + y * 5) & 0xFF) as u8);
            bytes.push(((x * 7 + y * 11) & 0xFF) as u8);
            bytes.push(((x * 13 + y * 17) & 0xFF) as u8);
        }
    }
    let samples =
        J2kLosslessSamples::new(&bytes, 128, 128, 3, 8, false).expect("valid RGB samples");

    let codestream = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::RequireDevice)
        .expect("Metal backend should encode WSI-sized DICOM tile");

    assert_j2k_facade_roundtrip(samples, &codestream);
}

#[cfg(feature = "cuda")]
#[test]
fn require_device_uses_cuda_j2k_encode_for_wsi_sized_tile() {
    let mut bytes = Vec::with_capacity(128 * 128 * 3);
    for y in 0..128u32 {
        for x in 0..128u32 {
            bytes.push(((x * 3 + y * 5) & 0xFF) as u8);
            bytes.push(((x * 7 + y * 11) & 0xFF) as u8);
            bytes.push(((x * 13 + y * 17) & 0xFF) as u8);
        }
    }
    let samples =
        J2kLosslessSamples::new(&bytes, 128, 128, 3, 8, false).expect("valid RGB samples");

    let codestream = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::RequireDevice)
        .expect("CUDA backend should encode WSI-sized DICOM tile");

    assert_j2k_facade_roundtrip(samples, &codestream);
}

#[test]
fn encode_dicom_j2k_frame_returns_finished_dicom_frame_bytes() {
    let bytes: Vec<u8> = (0..64).map(|value| ((value * 13) & 0xFF) as u8).collect();
    let samples = FrameSamples::new(&bytes, 8, 8, 1, 8, false).expect("valid samples");

    let frame = encode_dicom_j2k_frame(J2kFrameEncodeRequest {
        samples,
        transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
        encode_backend: EncodeBackendPreference::CpuOnly,
        codec_validation: CodecValidation::RoundTrip,
    })
    .unwrap();

    assert_eq!(
        frame.transfer_syntax_uid,
        TransferSyntax::Htj2kLosslessRpcl.uid()
    );
    assert_eq!(frame.bytes[..2], [0xFF, 0x4F]);
    assert!(!frame.used_device_encode);
    assert!(!frame.used_device_validation);
    assert_transfer_syntax_codestream(TransferSyntax::Htj2kLosslessRpcl, &frame.bytes);
    assert_j2k_facade_roundtrip(samples.to_j2k().unwrap(), &frame.bytes);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn encode_dicom_j2k_frame_can_return_metal_finished_bytes_when_required() {
    let mut bytes = Vec::with_capacity(128 * 128 * 3);
    for y in 0..128u32 {
        for x in 0..128u32 {
            bytes.push(((x * 5 + y * 3) & 0xFF) as u8);
            bytes.push(((x * 11 + y * 7) & 0xFF) as u8);
            bytes.push(((x * 17 + y * 13) & 0xFF) as u8);
        }
    }
    let samples = FrameSamples::new(&bytes, 128, 128, 3, 8, false).expect("valid RGB samples");

    let frame = encode_dicom_j2k_frame(J2kFrameEncodeRequest {
        samples,
        transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
        encode_backend: EncodeBackendPreference::RequireDevice,
        codec_validation: CodecValidation::RoundTrip,
    })
    .expect("Metal backend should return finished DICOM frame bytes");

    assert_eq!(
        frame.transfer_syntax_uid,
        TransferSyntax::Htj2kLosslessRpcl.uid()
    );
    assert!(frame.used_device_encode);
    assert!(frame.used_device_validation);
    assert!(frame.validation_micros > 0);
    assert!(!frame.bytes.is_empty());
    assert_transfer_syntax_codestream(TransferSyntax::Htj2kLosslessRpcl, &frame.bytes);
    assert_j2k_facade_roundtrip(samples.to_j2k().unwrap(), &frame.bytes);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn encode_dicom_j2k_frame_can_skip_runtime_codec_validation() {
    let mut bytes = Vec::with_capacity(128 * 128 * 3);
    for y in 0..128u32 {
        for x in 0..128u32 {
            bytes.push(((x * 19 + y * 23) & 0xFF) as u8);
            bytes.push(((x * 29 + y * 31) & 0xFF) as u8);
            bytes.push(((x * 37 + y * 41) & 0xFF) as u8);
        }
    }
    let samples = FrameSamples::new(&bytes, 128, 128, 3, 8, false).expect("valid RGB samples");

    let frame = encode_dicom_j2k_frame(J2kFrameEncodeRequest {
        samples,
        transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
        encode_backend: EncodeBackendPreference::RequireDevice,
        codec_validation: CodecValidation::Disabled,
    })
    .expect("Metal backend should return finished DICOM frame bytes");

    assert!(frame.used_device_encode);
    assert!(!frame.used_device_validation);
    assert_eq!(frame.validation_micros, 0);
    assert_transfer_syntax_codestream(TransferSyntax::Htj2kLosslessRpcl, &frame.bytes);
    assert_j2k_facade_roundtrip(samples.to_j2k().unwrap(), &frame.bytes);
}

#[test]
fn dicom_j2k_decomposition_uses_validated_lossless_safe_profile() {
    let gray = vec![0; 128 * 128];
    let gray_samples = J2kLosslessSamples::new(&gray, 128, 128, 1, 8, false).expect("valid gray");
    assert_eq!(dicom_j2k_decomposition_levels(gray_samples), 1);

    let rgb = vec![0; 128 * 128 * 3];
    let rgb_samples = J2kLosslessSamples::new(&rgb, 128, 128, 3, 8, false).expect("valid rgb");
    assert_eq!(dicom_j2k_decomposition_levels(rgb_samples), 1);
}

#[test]
fn j2k_decomposition_level_override_reaches_lossless_encoders() {
    for transfer_syntax in [
        TransferSyntax::Jpeg2000Lossless,
        TransferSyntax::Htj2kLossless,
    ] {
        for requested_levels in [0, 5] {
            let tmp = tempfile::tempdir().unwrap();
            let source = tmp.path().join("source.dcm");
            write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.77", 128, 128);

            let report = export_dicom(ExportRequest {
                source_path: source,
                output_dir: tmp
                    .path()
                    .join(format!("out-{transfer_syntax:?}-{requested_levels}")),
                options: ExportOptions {
                    tile_size: 128,
                    transfer_syntax,
                    encode_backend: EncodeBackendPreference::CpuOnly,
                    codec_validation: CodecValidation::Disabled,
                    j2k_decomposition_levels: Some(requested_levels),
                    ..ExportOptions::default()
                },
                color_management: ColorManagement::SourceOrSrgb,
                metadata: MetadataSource::ResearchPlaceholder,
                level_filter: None,
            })
            .unwrap();

            let object = dicom_object::open_file(&report.instances[0].path).unwrap();
            let fragments = object
                .element(tags::PIXEL_DATA)
                .unwrap()
                .value()
                .fragments()
                .unwrap();
            assert_eq!(
                j2k_cod_decomposition_levels(dicom_fragment_payload_without_padding(&fragments[0])),
                requested_levels,
                "{transfer_syntax:?} should honor explicit {requested_levels} DWT levels"
            );
        }
    }
}

#[test]
fn dicom_j2k_cpu_encode_round_trips_gray8_tile() {
    let bytes: Vec<u8> = (0..64).map(|value| ((value * 5) & 0xFF) as u8).collect();
    let samples = J2kLosslessSamples::new(&bytes, 8, 8, 1, 8, false).expect("valid samples");

    let codestream = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::CpuOnly).unwrap();

    assert_j2k_facade_roundtrip(samples, &codestream);
}

#[test]
fn dicom_htj2k_cpu_encode_round_trips_gray8_tile() {
    let bytes: Vec<u8> = (0..64).map(|value| ((value * 7) & 0xFF) as u8).collect();
    let samples = J2kLosslessSamples::new(&bytes, 8, 8, 1, 8, false).expect("valid samples");

    let codestream = crate::encode::encode_dicom_lossless(
        samples,
        TransferSyntax::Htj2kLossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();

    assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
    assert_j2k_facade_roundtrip(samples, &codestream);
}

#[test]
fn dicom_htj2k_rpcl_encode_writes_tlm_marker() {
    let bytes: Vec<u8> = (0..64).map(|value| ((value * 11) & 0xFF) as u8).collect();
    let samples = J2kLosslessSamples::new(&bytes, 8, 8, 1, 8, false).expect("valid samples");

    let codestream = crate::encode::encode_dicom_lossless(
        samples,
        TransferSyntax::Htj2kLosslessRpcl,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();

    let cod_offset = j2k_cod_marker_offset(&codestream);
    assert_eq!(codestream[cod_offset + 5], 0x02);
    assert!(codestream.windows(2).any(|window| window == [0xFF, 0x55]));
    assert_j2k_facade_roundtrip(samples, &codestream);
}

#[test]
fn raw_j2k_lossless_tile_can_passthrough_when_geometry_matches() {
    let bytes: Vec<u8> = (0..2 * 2 * 3)
        .map(|value| ((value * 19) & 0xFF) as u8)
        .collect();
    let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();
    let raw = raw_compressed_tile(
        Compression::Jp2kRgb,
        2,
        2,
        8,
        3,
        EncodedTilePhotometricInterpretation::Rgb,
        codestream.clone(),
    );

    let passed = j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Jpeg2000Lossless)
        .unwrap()
        .expect("J2K passthrough");

    assert_eq!(passed.codestream, codestream);
    assert_eq!(
        passed.profile,
        PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "YBR_RCT",
        }
    );
    assert_eq!(
        passed.transfer_syntax,
        CompressedTransferSyntax::Jpeg2000Lossless
    );
}

#[test]
fn raw_j2k_ycbcr_tile_can_passthrough_to_general_jpeg2000() {
    let bytes: Vec<u8> = (0..2 * 2 * 3)
        .map(|value| ((value * 17) & 0xFF) as u8)
        .collect();
    let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();
    let raw = raw_compressed_tile(
        Compression::Jp2kYcbcr,
        2,
        2,
        8,
        3,
        EncodedTilePhotometricInterpretation::YbrFull422,
        codestream.clone(),
    );

    let passed = j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Jpeg2000)
        .unwrap()
        .expect("general J2K passthrough");

    assert_eq!(passed.codestream, codestream);
    assert_eq!(
        passed.profile,
        PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "YBR_RCT",
        }
    );
    assert_eq!(
        passed.transfer_syntax,
        CompressedTransferSyntax::Jpeg2000Lossless
    );
}

#[test]
fn export_j2k_passthrough_does_not_touch_gpu_even_when_device_required() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes: Vec<u8> = (0..2 * 2 * 3)
        .map(|value| ((value * 41) & 0xFF) as u8)
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
    write_tiled_jp2k_rgb_tiff(&source, 2, 2, 2, 2, std::slice::from_ref(&codestream));

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000Lossless,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_single_j2k_passthrough_avoids_gpu_for_test(&report);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(
        dicom_fragment_payload_without_padding(&fragments[0]),
        codestream
    );
}

#[test]
fn export_general_j2k_passthrough_accepts_ycbcr_source_without_gpu_work() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.svs");
    let codestream = write_general_j2k_ycbcr_passthrough_tiff_for_test(&source, 13);

    let report = export_general_j2k_passthrough_for_test(source, tmp.path().join("out"));

    assert_single_j2k_passthrough_avoids_gpu_for_test(&report);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax.as_str(),
        TransferSyntax::Jpeg2000.uid()
    );
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
    assert_eq!(
        object
            .element(tags::PHOTOMETRIC_INTERPRETATION)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "YBR_RCT"
    );
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(
        dicom_fragment_payload_without_padding(&fragments[0]),
        codestream
    );
}

#[test]
fn export_general_j2k_rejects_edge_reencoding_without_publishing() {
    let tmp = tempfile::tempdir().unwrap();
    let codestreams = j2k_edge_fallback_codestreams_for_test();
    let source = tmp.path().join("source.svs");
    write_tiled_jp2k_ycbcr_tiff(
        &source,
        3,
        2,
        2,
        2,
        &[codestreams.interior.clone(), codestreams.edge.clone()],
    );

    let error = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::Jpeg2000,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: true,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("passthrough"), "unexpected error: {error}");
    assert!(std::fs::read_dir(tmp.path().join("out"))
        .unwrap()
        .all(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_none_or(|ext| ext != "dcm")));
}

#[test]
fn export_aperio_33003_yuv422_rejects_implicit_component_resampling() {
    let tmp = tempfile::tempdir().unwrap();
    let encode_yuv422 = |width: u32, height: u32, values: [u8; 3]| {
        let luma = vec![values[0]; (width * height) as usize];
        let chroma_len = (width.div_ceil(2) * height) as usize;
        let chroma_blue = vec![values[1]; chroma_len];
        let chroma_red = vec![values[2]; chroma_len];
        let planes = [
            j2k::J2kLosslessComponentPlane {
                data: &luma,
                x_rsiz: 1,
                y_rsiz: 1,
            },
            j2k::J2kLosslessComponentPlane {
                data: &chroma_blue,
                x_rsiz: 2,
                y_rsiz: 1,
            },
            j2k::J2kLosslessComponentPlane {
                data: &chroma_red,
                x_rsiz: 2,
                y_rsiz: 1,
            },
        ];
        let samples = j2k::J2kLosslessComponentSamples::new(&planes, width, height, 8, false)
            .expect("valid YUV 4:2:2 component samples");
        j2k::encode_j2k_lossless_components(
            samples,
            &j2k::J2kLosslessEncodeOptions::default()
                .with_cpu_only_backend()
                .with_reversible_transform(ReversibleTransform::None53)
                .with_validation(j2k::J2kEncodeValidation::CpuRoundTrip),
        )
        .expect("encode YUV 4:2:2 fixture")
        .codestream
    };
    let source_tiles = [
        encode_yuv422(2, 2, [100, 80, 180]),
        encode_yuv422(1, 2, [120, 90, 160]),
        encode_yuv422(2, 1, [140, 110, 145]),
        encode_yuv422(1, 1, [160, 130, 115]),
    ];
    for codestream in &source_tiles {
        let view = J2kView::parse(codestream).expect("parse source YUV 4:2:2 codestream");
        let sampling = view
            .support_info()
            .expect("source support metadata")
            .components
            .iter()
            .map(|component| (component.x_rsiz, component.y_rsiz))
            .collect::<Vec<_>>();
        assert_eq!(sampling, [(1, 1), (2, 1), (2, 1)]);
        assert_eq!(j2k_cod_mct(codestream), 0);
    }

    let source = tmp.path().join("source.svs");
    write_tiled_aperio_jp2k_ycbcr_tiff(&source, 3, 3, 2, 2, &source_tiles);
    let error = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("passthrough"), "unexpected error: {error}");
}

#[test]
fn export_general_j2k_rejects_rct_edge_reencoding() {
    let tmp = tempfile::tempdir().unwrap();
    let codestreams = j2k_edge_fallback_codestreams_for_test();
    let source = tmp.path().join("source.svs");
    write_tiled_jp2k_rgb_tiff(
        &source,
        3,
        2,
        2,
        2,
        &[codestreams.interior.clone(), codestreams.edge],
    );

    let error = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("passthrough"), "unexpected error: {error}");
}

#[test]
fn dicom_roundtrip_lossless_pixel_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let width = 8u32;
    let height = 8u32;
    let pixels = (0..width * height * 3)
        .map(|value| ((value * 19 + 7) & 0xFF) as u8)
        .collect::<Vec<_>>();
    write_source_dicom_with_pixels(
        &source,
        "1.2.826.0.1.3680043.10.999.79",
        width,
        height,
        pixels.clone(),
    );

    for transfer_syntax in [
        TransferSyntax::Jpeg2000Lossless,
        TransferSyntax::Htj2kLossless,
        TransferSyntax::Htj2kLosslessRpcl,
    ] {
        let report = export_dicom(ExportRequest {
            source_path: source.clone(),
            output_dir: tmp.path().join(format!("out-{transfer_syntax:?}")),
            options: ExportOptions {
                tile_size: width,
                transfer_syntax,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);
        let payload = dicom_fragment_payload_without_padding(&fragments[0]);
        let actual = decode_j2k_frame_for_test(payload, width, height, 3, 8);

        assert_eq!(
            actual, pixels,
            "{transfer_syntax:?} DICOM pixel data should decode byte-identical"
        );
    }
}

#[test]
fn export_general_j2k_preserves_ict_codestream_and_compression_history() {
    let tmp = tempfile::tempdir().unwrap();
    let mut codestreams = j2k_edge_fallback_codestreams_for_test();
    patch_j2k_cod_wavelet_transform(&mut codestreams.interior, 0);
    assert_eq!(
        j2k_passthrough_transfer_syntax(&codestreams.interior),
        CompressedTransferSyntax::Jpeg2000Lossy
    );
    patch_j2k_cod_wavelet_transform(&mut codestreams.edge, 0);
    let source = tmp.path().join("source.svs");
    write_tiled_jp2k_rgb_tiff(
        &source,
        4,
        2,
        2,
        2,
        &[codestreams.interior.clone(), codestreams.interior.clone()],
    );

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(report.metrics.routes.j2k_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object
            .element(tags::PHOTOMETRIC_INTERPRETATION)
            .unwrap()
            .to_str()
            .unwrap(),
        "YBR_ICT"
    );
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 2);
    for fragment in fragments {
        assert_eq!(
            dicom_fragment_payload_without_padding(fragment),
            codestreams.interior
        );
    }
    assert_eq!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "01"
    );
    assert_eq!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION_METHOD)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "ISO_15444_1"
    );
    assert!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION_RATIO)
            .unwrap()
            .to_float32()
            .unwrap()
            > 0.0
    );
}

#[test]
fn export_general_j2k_rejects_mixed_ict_and_rct_before_publishing() {
    let tmp = tempfile::tempdir().unwrap();
    let rct = j2k_edge_fallback_codestreams_for_test().interior;
    let mut ict = rct.clone();
    patch_j2k_cod_wavelet_transform(&mut ict, 0);
    let source = tmp.path().join("mixed.svs");
    write_tiled_jp2k_rgb_tiff(&source, 4, 2, 2, 2, &[ict, rct]);
    let error = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("pixel profile changed"),
        "unexpected error: {error}"
    );
    assert!(std::fs::read_dir(tmp.path().join("out"))
        .unwrap()
        .all(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_none_or(|ext| ext != "dcm")));
}

#[test]
fn export_htj2k_tiff_preserves_lossless_and_ict_codestreams() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture =
        write_htj2k_rpcl_dicom_source_for_test(tmp.path(), "1.2.826.0.1.3680043.10.999.63", 2, 2);
    for (name, syntax, photometric) in [
        ("lossless", TransferSyntax::Htj2kLosslessRpcl, "YBR_RCT"),
        ("ict", TransferSyntax::Htj2k, "YBR_ICT"),
    ] {
        let mut codestream = fixture.fragments[0].clone();
        if syntax == TransferSyntax::Htj2k {
            patch_j2k_cod_wavelet_transform(&mut codestream, 0);
        }
        let source = tmp.path().join(format!("{name}.tiff"));
        write_tiled_jp2k_rgb_tiff(&source, 2, 2, 2, 2, std::slice::from_ref(&codestream));
        let report = export_dicom(ExportRequest {
            source_path: source,
            output_dir: tmp.path().join(name),
            options: ExportOptions {
                tile_size: if syntax == TransferSyntax::Htj2k {
                    512
                } else {
                    2
                },
                transfer_syntax: syntax,
                jpeg_direct_htj2k_profile: if syntax == TransferSyntax::Htj2k {
                    JpegDirectHtj2kProfile::Lossy97
                } else {
                    JpegDirectHtj2kProfile::Lossless53
                },
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();
        assert_single_j2k_passthrough_avoids_gpu_for_test(&report);
        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object
                .element(tags::PHOTOMETRIC_INTERPRETATION)
                .unwrap()
                .to_str()
                .unwrap(),
            photometric
        );
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);
        assert_eq!(
            dicom_fragment_payload_without_padding(&fragments[0]),
            codestream
        );
    }
}

#[test]
fn jpeg2000_lossless_rejects_lossy_edge_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let mut codestreams = j2k_edge_fallback_codestreams_for_test();
    patch_j2k_cod_wavelet_transform(&mut codestreams.edge, 0);
    assert_eq!(
        j2k_passthrough_transfer_syntax(&codestreams.edge),
        CompressedTransferSyntax::Jpeg2000Lossy
    );
    let source = tmp.path().join("source.svs");
    write_tiled_jp2k_ycbcr_tiff(
        &source,
        3,
        2,
        2,
        2,
        &[codestreams.interior, codestreams.edge],
    );

    let err = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000Lossless,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap_err()
    .to_string();

    assert!(err.contains("lossy"), "unexpected error: {err}");
}

#[test]
fn export_htj2k_rpcl_passthrough_does_not_touch_gpu_even_when_device_required() {
    let tmp = tempfile::tempdir().unwrap();
    let source =
        write_htj2k_rpcl_dicom_source_for_test(tmp.path(), "1.2.826.0.1.3680043.10.999.43", 2, 2);
    assert_eq!(source.fragments.len(), 1);

    let slide = open_slide(&source.path).unwrap();
    let planned = plan_lossless_j2k_frames(
        &slide,
        LosslessJ2kPlanRequest {
            location: InstanceCoordinate::first_series_level(0),
            start_row: 0,
            row_count: 1,
            start_col: 0,
            tile_count: 1,
            grid: FrameRectGrid {
                matrix_columns: 2,
                matrix_rows: 2,
                frame_columns: 2,
                frame_rows: 2,
            },
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            allow_passthrough_probe: true,
        },
    )
    .unwrap();
    assert!(planned[0].has_passthrough());
    assert!(planned[0].source_j2k.is_none());
    assert!(planned[0].source_jpeg.is_none());
    assert!(planned[0].source_jpeg_retile_rejection.is_none());

    let report = export_htj2k_rpcl_dicom_passthrough_for_test(&source, tmp.path().join("out"));

    assert_single_j2k_passthrough_avoids_gpu_for_test(&report);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax.trim_end_matches('\0'),
        TransferSyntax::Htj2kLosslessRpcl.uid()
    );
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(
        dicom_fragment_payload_without_padding(&fragments[0]),
        source.fragments[0]
    );
}

#[test]
fn export_htj2k_rpcl_dicom_edge_passthrough_keeps_padded_source_frame() {
    let tmp = tempfile::tempdir().unwrap();
    let source =
        write_htj2k_rpcl_dicom_source_for_test(tmp.path(), "1.2.826.0.1.3680043.10.999.53", 3, 2);
    assert_eq!(source.fragments.len(), 2);

    let report = export_htj2k_rpcl_dicom_passthrough_for_test(&source, tmp.path().join("out"));

    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.j2k_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.cpu_input_frames, 0);
    assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 2);
    assert_eq!(
        dicom_fragment_payload_without_padding(&fragments[0]),
        source.fragments[0]
    );
    assert_eq!(
        dicom_fragment_payload_without_padding(&fragments[1]),
        source.fragments[1]
    );
}

#[test]
fn raw_j2k_passthrough_rejects_geometry_or_syntax_mismatch() {
    let bytes: Vec<u8> = (0..2 * 2 * 3)
        .map(|value| ((value * 23) & 0xFF) as u8)
        .collect();
    let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();
    let raw = raw_compressed_tile(
        Compression::Jp2kRgb,
        2,
        2,
        8,
        3,
        EncodedTilePhotometricInterpretation::Rgb,
        codestream,
    );

    assert!(
        j2k_passthrough_frame(raw.clone(), 1, 2, TransferSyntax::Jpeg2000Lossless)
            .unwrap()
            .is_none()
    );
    assert!(
        j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Htj2kLosslessRpcl)
            .unwrap()
            .is_none()
    );
}

#[test]
fn raw_htj2k_rpcl_tile_can_passthrough_when_geometry_matches() {
    let bytes: Vec<u8> = (0..2 * 2 * 3)
        .map(|value| ((value * 31) & 0xFF) as u8)
        .collect();
    let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Htj2kLosslessRpcl,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();
    let raw = raw_compressed_tile(
        Compression::Jp2kRgb,
        2,
        2,
        8,
        3,
        EncodedTilePhotometricInterpretation::Rgb,
        codestream.clone(),
    );

    let passed = j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Htj2kLosslessRpcl)
        .unwrap()
        .expect("HTJ2K RPCL passthrough");

    assert_eq!(passed.codestream, codestream);
    assert_eq!(
        passed.profile,
        PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "YBR_RCT",
        }
    );
}

#[test]
fn raw_htj2k_lrcp_tile_rejects_rpcl_passthrough() {
    let bytes: Vec<u8> = (0..2 * 2 * 3)
        .map(|value| ((value * 37) & 0xFF) as u8)
        .collect();
    let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Htj2kLossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();
    let raw = raw_compressed_tile(
        Compression::Jp2kRgb,
        2,
        2,
        8,
        3,
        EncodedTilePhotometricInterpretation::Rgb,
        codestream,
    );

    assert!(
        j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Htj2kLosslessRpcl)
            .unwrap()
            .is_none()
    );
}

#[test]
fn cpu_j2k_batch_matches_serial_ordered_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.71", 4, 2);
    let slide = Slide::open(&source).unwrap();
    let frames = [
        LosslessJ2kCpuBatchFrame {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        },
        LosslessJ2kCpuBatchFrame {
            x: 2,
            y: 0,
            width: 2,
            height: 2,
        },
    ];
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let location = JpegBaselineFrameLocation::first_series_level(0);

    let batch = encode_cpu_input_lossless_j2k_tile_batch(
        &slide,
        level,
        LosslessJ2kCpuBatchSettings {
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            codec_validation: CodecValidation::RoundTrip,
            j2k_decomposition_levels: None,
            reversible_transform: ReversibleTransform::Rct53,
            max_prepared_frame_bytes: u64::MAX,
        },
        location,
        &frames,
        2,
    )
    .unwrap();

    let mut serial_encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::CpuOnly,
        TransferSyntax::Htj2kLosslessRpcl,
        CodecValidation::RoundTrip,
    );
    let serial = frames
        .iter()
        .map(|frame| {
            encode_cpu_input_tile(&slide, &mut serial_encoder, location, *frame, 2).unwrap()
        })
        .collect::<Vec<_>>();

    assert_eq!(batch.len(), serial.len());
    for (batch, serial) in batch.iter().zip(serial.iter()) {
        assert_eq!(batch.profile, serial.1);
        assert_eq!(
            batch.encoded.as_ref().unwrap().codestream_bytes().unwrap(),
            serial.0.as_ref().unwrap().codestream_bytes().unwrap()
        );
    }
}

#[test]
fn jpeg_baseline_cpu_batch_matches_serial_ordered_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.72", 4, 2);
    let slide = Slide::open(&source).unwrap();
    let location = JpegBaselineFrameLocation::first_series_level(0);
    let frames = [
        JpegBaselineFallbackFrame {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        },
        JpegBaselineFallbackFrame {
            x: 2,
            y: 0,
            width: 2,
            height: 2,
        },
    ];
    let settings = JpegBaselineCpuEncodeSettings {
        frame_columns: 2,
        frame_rows: 2,
        jpeg_quality: 90,
        optimize_huffman: false,
        max_prepared_frame_bytes: u64::MAX,
    };

    let batch =
        encode_jpeg_baseline_cpu_input_tile_batch(&slide, location, &frames, settings).unwrap();
    let serial = frames
        .iter()
        .map(|frame| {
            encode_jpeg_baseline_cpu_input_tile(&slide, location, *frame, settings).unwrap()
        })
        .collect::<Vec<_>>();

    assert_eq!(batch.len(), serial.len());
    for (batch, serial) in batch.iter().zip(serial.iter()) {
        assert_eq!(batch.encoded.data, serial.encoded.data);
        assert_eq!(batch.profile, serial.profile);
    }
}

#[test]
fn jpeg_quality_option_changes_fallback_frame_size() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.76", 64, 64);

    let low = export_dicom(ExportRequest {
        source_path: source.clone(),
        output_dir: tmp.path().join("low"),
        options: ExportOptions {
            tile_size: 64,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            jpeg_quality: 40,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();
    let high = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("high"),
        options: ExportOptions {
            tile_size: 64,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            jpeg_quality: 95,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(low.metrics.routes.jpeg_cpu_encode_frames, 1);
    assert_eq!(high.metrics.routes.jpeg_cpu_encode_frames, 1);
    let low_len = first_pixel_data_fragment_payload_len(&low.instances[0].path);
    let high_len = first_pixel_data_fragment_payload_len(&high.instances[0].path);
    assert!(
        high_len > low_len,
        "quality 95 payload ({high_len}) should be larger than quality 40 payload ({low_len})"
    );
}

#[test]
fn jpeg_optimize_huffman_shrinks_fallback_frame_without_changing_pixels() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.77", 64, 64);
    let export = |name: &str, jpeg_optimize_huffman: bool| {
        export_dicom(ExportRequest {
            source_path: source.clone(),
            output_dir: tmp.path().join(name),
            options: ExportOptions {
                tile_size: 64,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                jpeg_optimize_huffman,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap()
    };
    let decode_first_frame = |path: &std::path::Path| {
        let object = dicom_object::open_file(path).unwrap();
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        let payload = dicom_fragment_jpeg_payload(&fragments[0]).to_vec();
        let (decoded, _) = j2k_jpeg::Decoder::new(&payload)
            .unwrap()
            .decode_request(j2k_jpeg::DecodeRequest::full(j2k_jpeg::PixelFormat::Rgb8))
            .unwrap();
        (payload, decoded.as_slice().to_vec())
    };

    let standard = export("standard", false);
    let optimized = export("optimized", true);

    assert_eq!(standard.metrics.routes.jpeg_cpu_encode_frames, 1);
    assert_eq!(optimized.metrics.routes.jpeg_cpu_encode_frames, 1);
    let (standard_payload, standard_pixels) = decode_first_frame(&standard.instances[0].path);
    let (optimized_payload, optimized_pixels) = decode_first_frame(&optimized.instances[0].path);
    assert_ne!(standard_payload, optimized_payload);
    assert!(
        optimized_payload.len() < standard_payload.len(),
        "optimized payload ({}) should be smaller than standard payload ({})",
        optimized_payload.len(),
        standard_payload.len()
    );
    assert_eq!(optimized_pixels, standard_pixels);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn jpeg_optimize_huffman_keeps_prefer_device_fallback_frames_on_cpu() {
    let tmp = tempfile::tempdir().unwrap();
    let pixels = crate::synthetic_source::deterministic_rgb_pixels(64, 64).unwrap();
    let samples = J2kLosslessSamples::new(&pixels, 64, 64, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::Disabled,
    )
    .unwrap();
    let source = tmp.path().join("source.svs");
    write_tiled_jp2k_rgb_tiff(&source, 128, 128, 64, 64, &vec![codestream; 4]);
    let export = |name: &str, jpeg_optimize_huffman: bool| {
        export_dicom(ExportRequest {
            source_path: source.clone(),
            output_dir: tmp.path().join(name),
            options: ExportOptions {
                tile_size: 64,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::PreferDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: true,
                jpeg_optimize_huffman,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: Some(0),
        })
        .unwrap()
    };

    let standard = export("standard", false);
    let optimized = export("optimized", true);

    assert_eq!(standard.metrics.routes.jpeg_metal_encode_frames, 4);
    assert_eq!(optimized.metrics.routes.jpeg_metal_encode_frames, 0);
    assert_eq!(optimized.metrics.routes.jpeg_cpu_encode_frames, 4);
}

#[test]
fn jpeg_baseline_cpu_fallback_writes_restart_markers_for_large_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.73", 160, 64);

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 160,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(report.metrics.routes.jpeg_cpu_encode_frames, 1);
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    let payload = dicom_fragment_jpeg_payload(&fragments[0]);
    assert!(payload.windows(2).any(|window| window == [0xFF, 0xDD]));
    assert!(payload.windows(2).any(|window| window == [0xFF, 0xD0]));
}
