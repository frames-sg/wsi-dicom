use std::io::Write;

use j2k_jpeg::{JpegBackend, JpegSamples, JpegSubsampling};

use super::*;

#[test]
fn jpeg_preparation_rejects_an_edge_that_requires_reencoding() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.svs");
    // The edge tile encodes only its 1-pixel logical width, so it cannot fill
    // a 32-pixel TILED_FULL frame without decoding.
    let tiles = [
        encode_test_jpeg(32, 32, [180, 30, 40]),
        encode_test_jpeg(1, 32, [20, 170, 60]),
    ];
    write_tiled_jpeg_tiff(&source, 33, 32, 32, 32, &tiles);
    let options = NormalizedExportOptions::from_validated(&ExportOptions {
        tile_size: 32,
        transfer_syntax: TransferSyntax::JpegBaseline8Bit,
        encode_backend: EncodeBackendPreference::CpuOnly,
        ..ExportOptions::default()
    });
    let metadata = MetadataSource::ResearchPlaceholder.resolve().unwrap();
    let identity =
        DicomExportIdentity::for_export(&source, &options, &metadata, Some(0), &[]).unwrap();
    let slide = Slide::open(&source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let location = InstanceCoordinate::new(0, 0, 0, 0, 0, 0);
    let geometry = jpeg_baseline_route_frame_geometry(&slide, level, location, 32).unwrap();
    let spool_path = tmp.path().join("frames.spool");
    let error = super::super::jpeg_baseline_frames::prepare_jpeg_frames(
        &slide,
        InstanceExportContext {
            options: &options,
            metadata: &metadata,
            identity: &identity,
            instance_number: 1,
            coordinate: location,
            level,
            per_frame_plan: None,
        },
        geometry,
        spool_path.clone(),
    )
    .err()
    .expect("incompatible JPEG edge must not be re-encoded");
    assert!(error
        .to_string()
        .contains("pixel re-encoding fallback is disabled"));
    assert!(error.to_string().contains("row=0 col=1"));
}

#[test]
fn jpeg_baseline_copies_padded_edge_tiles_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.svs");
    // TIFF pads the 1-pixel logical edge to a full encoded 32x32 tile.
    let tiles = [
        encode_test_jpeg(32, 32, [180, 30, 40]),
        encode_test_jpeg(32, 32, [20, 170, 60]),
    ];
    write_tiled_jpeg_tiff(&source, 33, 32, 32, 32, &tiles);

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 32,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap();

    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.compressed_source_reencoded_frames, 0);
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    for (fragment, tile) in fragments.iter().zip(&tiles) {
        assert_eq!(dicom_fragment_jpeg_payload(fragment), tile.as_slice());
    }
}

/// Rewrite a baseline JPEG's SOF and SOS component identifiers to 0, 1, 2:
/// the marker-less Aperio convention for untransformed RGB components.
fn with_component_ids_012(jpeg: &[u8]) -> Vec<u8> {
    let mut out = jpeg.to_vec();
    let mut ids = Vec::new();
    let mut offset = 2;
    while offset + 4 <= out.len() {
        assert_eq!(out[offset], 0xFF, "segment marker expected");
        let marker = out[offset + 1];
        let length = usize::from(u16::from_be_bytes([out[offset + 2], out[offset + 3]]));
        let payload = offset + 4;
        match marker {
            0xC0 => {
                for component in 0..usize::from(out[payload + 5]) {
                    let at = payload + 6 + 3 * component;
                    ids.push(out[at]);
                    out[at] = component as u8;
                }
            }
            0xDA => {
                for component in 0..usize::from(out[payload]) {
                    let at = payload + 1 + 2 * component;
                    let index = ids.iter().position(|id| *id == out[at]).unwrap();
                    out[at] = index as u8;
                }
                return out;
            }
            _ => {}
        }
        offset += 2 + length;
    }
    panic!("JPEG has no SOS segment");
}

#[test]
fn rgb_jpeg_with_component_ids_012_is_copied_with_an_adobe_rgb_marker() {
    let tmp = tempfile::tempdir().unwrap();
    // A 12x8 level in 8x8 tiles: the second tile is a padded edge tile.
    let tiles = [
        with_component_ids_012(&crate::test_support::encode_test_jpeg_with_subsampling(
            8,
            8,
            [200, 40, 90],
            JpegSubsampling::Ybr444,
        )),
        with_component_ids_012(&crate::test_support::encode_test_jpeg_with_subsampling(
            8,
            8,
            [30, 160, 220],
            JpegSubsampling::Ybr444,
        )),
    ];
    assert_eq!(
        super::super::jpeg_baseline::jpeg_color_evidence(&tiles[0]).unwrap(),
        super::super::jpeg_baseline::JpegColorEvidence::RgbComponentIds012
    );
    let source = tmp.path().join("rgb-ids-012.tiff");
    crate::test_support::write_tiled_compressed_tiff(&source, 12, 8, 8, 8, 7, 2, 3, &tiles);

    let report = export_dicom(ExportRequest {
        source_path: source.clone(),
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap();

    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.compressed_source_reencoded_frames, 0);
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object
            .element(tags::PHOTOMETRIC_INTERPRETATION)
            .unwrap()
            .to_str()
            .unwrap()
            .trim(),
        "RGB"
    );
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    let slide = Slide::open(&source).unwrap();
    for (index, tile) in tiles.iter().enumerate() {
        let frame = dicom_fragment_jpeg_payload(&fragments[index]);
        // Only an Adobe APP14 transform-0 segment is added after SOI.
        assert_eq!(&frame[..4], &[0xFF, 0xD8, 0xFF, 0xEE]);
        assert_eq!(&frame[4..18], b"\x00\x0eAdobe\x00\x64\x00\x00\x00\x00\x00");
        assert_eq!(&frame[18..], &tile[2..]);
        // A decoder that reads only the bitstream now agrees with wsi-rs.
        let (decoded, _) = j2k_jpeg::Decoder::new(frame)
            .unwrap()
            .decode_request(j2k_jpeg::DecodeRequest::full(j2k_jpeg::PixelFormat::Rgb8))
            .unwrap();
        let width = if index == 0 { 8 } else { 4 };
        let region = slide
            .read_region(&wsi_rs::RegionRequest::new(
                0usize,
                0usize,
                0u32,
                (index as i64 * 8, 0),
                (width, 8),
            ))
            .unwrap();
        let wsi_rs::CpuTileData::U8(expected) = region.data() else {
            panic!("expected 8-bit RGB source region")
        };
        for y in 0..8usize {
            let row = width as usize * 3;
            assert_eq!(
                &decoded.as_slice()[y * 24..y * 24 + row],
                &expected[y * row..(y + 1) * row],
                "frame {index} row {y}"
            );
        }
    }
}

#[test]
fn export_htj2k_reencodes_jpeg_strips_losslessly_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.svs");
    let tiles = [
        encode_test_jpeg(16, 2, [160, 20, 40]),
        encode_test_jpeg(16, 2, [20, 160, 40]),
        encode_test_jpeg(16, 2, [40, 20, 160]),
        encode_test_jpeg(16, 2, [160, 160, 40]),
    ];
    write_tiled_jpeg_tiff(&source, 32, 4, 16, 2, &tiles);

    let report = export_dicom(ExportRequest {
        source_path: source.clone(),
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap();

    let frames = report.metrics.routes.total_frames;
    assert_eq!(frames, 4);
    assert_eq!(
        report.metrics.routes.compressed_source_reencoded_frames,
        frames
    );
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION_METHOD)
            .unwrap()
            .to_str()
            .unwrap()
            .trim(),
        "ISO_10918_1",
        "a lossless re-encode keeps the source JPEG stage and adds none"
    );
    let slide = Slide::open(&source).unwrap();
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    for (index, fragment) in fragments.iter().enumerate() {
        let decoded = decode_j2k_frame_for_test(fragment, 8, 8, 3, 8);
        let (x, y) = ((index % 4) as i64 * 8, (index / 4) as i64 * 8);
        let region = slide
            .read_region(&wsi_rs::RegionRequest::new(
                0usize,
                0usize,
                0u32,
                (x, y),
                (8, 4),
            ))
            .unwrap();
        let wsi_rs::CpuTileData::U8(expected) = region.data() else {
            panic!("expected 8-bit RGB source region")
        };
        assert_eq!(
            &decoded[..expected.len()],
            expected.as_slice(),
            "frame {index}"
        );
    }
}

#[test]
fn direct_jpeg_buffer_holds_at_most_budget_plus_one_variable_frame() {
    let tmp = tempfile::tempdir().unwrap();
    let small = encode_test_jpeg(32, 32, [180, 30, 40]);
    let mut large = encode_test_jpeg(32, 32, [20, 170, 60]);
    let eoi = large.split_off(large.len() - 2);
    let comment = vec![0x5a; 4096];
    large.extend_from_slice(&[0xFF, 0xFE]);
    large.extend_from_slice(&u16::try_from(comment.len() + 2).unwrap().to_be_bytes());
    large.extend_from_slice(&comment);
    large.extend_from_slice(&eoi);
    let source = tmp.path().join("variable-frames.svs");
    write_tiled_jpeg_tiff(
        &source,
        128,
        32,
        32,
        32,
        &[small.clone(), small.clone(), large.clone(), small],
    );
    let slide = Slide::open(&source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let location = InstanceCoordinate::new(0, 0, 0, 0, 0, 0);
    let geometry = jpeg_baseline_route_frame_geometry(&slide, level, location, 32).unwrap();
    let start = super::super::jpeg_passthrough::try_prepare_direct_jpeg_passthrough(
        &slide, location, level, geometry, 0,
    )
    .unwrap()
    .unwrap();
    let budget = start.first_frame.len() + 32;
    let mut frames = super::super::jpeg_passthrough::DirectJpegPassthroughFrameWriter::new(
        &slide, location, geometry, None, 4, start, 2048, budget,
    );

    frames.frame_len(1).unwrap();

    assert!(frames.buffered_chunk_bytes() <= budget);
    assert_eq!(frames.pending_frame_bytes(), Some(large.len()));
    assert_eq!(frames.frame_len(2).unwrap(), large.len() as u64);
}

fn export_jpeg_baseline_native_geometry_for_test(
    source_path: std::path::PathBuf,
    output_dir: std::path::PathBuf,
) -> ExportReport {
    export_dicom(ExportRequest {
        source_path,
        output_dir,
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
        level_filter: Some(0),
    })
    .unwrap()
}

fn assert_dicom_instance_geometry_for_test(
    path: &std::path::Path,
    rows: u32,
    columns: u32,
    frames: u32,
) {
    let object = dicom_object::open_file(path).unwrap();
    assert_eq!(
        object.element(tags::ROWS).unwrap().to_int::<u32>().unwrap(),
        rows
    );
    assert_eq!(
        object
            .element(tags::COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        columns
    );
    assert_eq!(
        object
            .element(tags::NUMBER_OF_FRAMES)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        frames
    );
}

#[test]
fn export_dicom_passthrough_writes_jpeg_baseline_vl_wsi_instance() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));
    let out = tmp.path().join("out");

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
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

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.instances[0].frame_count, 1);
    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.cpu_input_frames, 0);
    assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 0);
    assert_eq!(report.metrics.route_passthrough_frames(), 1);
    assert_eq!(report.metrics.routes.gpu_transcode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
    assert_eq!(
        report.instances[0].transfer_syntax_uid,
        TransferSyntax::JpegBaseline8Bit.uid()
    );

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax.trim_end_matches('\0'),
        TransferSyntax::JpegBaseline8Bit.uid()
    );
    assert_eq!(object.element(tags::PYRAMID_UID).unwrap().vr(), VR::UI);
    assert_eq!(
        object.element(tags::FRAME_OF_REFERENCE_UID).unwrap().vr(),
        VR::UI
    );
    assert_eq!(
        object
            .element(tags::INSTANCE_NUMBER)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        1
    );
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
        "ISO_10918_1"
    );
    assert!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION_RATIO)
            .unwrap()
            .to_float32()
            .unwrap()
            > 0.0
    );
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0], jpeg);
}

#[test]
fn ambiguous_rgb_tiff_jpeg_rejects_implicit_reencoding() {
    let tmp = tempfile::tempdir().unwrap();
    let pixels = [80u8, 140, 220].repeat(8 * 8);
    let jpeg = j2k_jpeg::encode_jpeg_baseline(
        JpegSamples::Rgb8 {
            data: &pixels,
            width: 8,
            height: 8,
        },
        j2k_jpeg::JpegEncodeOptions {
            quality: 95,
            subsampling: JpegSubsampling::Ybr444,
            restart_interval: None,
            backend: JpegBackend::Cpu,
        },
    )
    .unwrap()
    .data;
    assert_eq!(
        super::super::jpeg_baseline::jpeg_color_evidence(&jpeg).unwrap(),
        super::super::jpeg_baseline::JpegColorEvidence::Ambiguous
    );
    let source = tmp.path().join("ambiguous-rgb.tiff");
    crate::test_support::write_tiled_compressed_tiff(
        &source,
        8,
        8,
        8,
        8,
        7,
        2,
        3,
        std::slice::from_ref(&jpeg),
    );
    let error = export_dicom(ExportRequest {
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
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("pixel re-encoding fallback is disabled"),
        "unexpected error: {error}"
    );
}

#[test]
fn export_htj2k_rejects_jpeg_strips_requiring_pixel_reencoding() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.svs");
    let out = tmp.path().join("out");
    let tiles = [
        encode_test_jpeg(16, 2, [160, 20, 40]),
        encode_test_jpeg(16, 2, [20, 160, 40]),
        encode_test_jpeg(16, 2, [40, 20, 160]),
        encode_test_jpeg(16, 2, [160, 160, 40]),
    ];
    write_tiled_jpeg_tiff(&source, 32, 4, 16, 2, &tiles);

    let error = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            pixel_reencode: PixelReencodePolicy::Never,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("pixel re-encoding fallback is disabled"),
        "{error}"
    );
}

#[test]
fn export_lossy_htj2k_from_lossy_grayscale_jpeg_preserves_two_ordered_stages() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.tiff");
    let jpeg = encode_test_gray_jpeg(8, 8, 96);
    write_tiled_grayscale_jpeg_tiff(&source, 8, 8, 8, 8, &[jpeg]);

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::Htj2k,
            jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile::Lossy97,
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

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION_METHOD)
            .unwrap()
            .to_multi_str()
            .unwrap()
            .as_ref(),
        ["ISO_10918_1", "ISO_15444_15"]
    );
    let ratios = object
        .element(tags::LOSSY_IMAGE_COMPRESSION_RATIO)
        .unwrap()
        .to_multi_float64()
        .unwrap();
    assert_eq!(ratios.len(), 2);
    assert!(ratios.iter().all(|ratio| ratio.is_finite() && *ratio > 0.0));
}

#[test]
fn export_ndpi_jpeg_baseline_retiles_restart_strips_without_decode_encode() {
    use j2k_jpeg::transcode::{extract_dct_blocks, DctExtractOptions};

    for subsampling in [JpegSubsampling::Ybr444, JpegSubsampling::Ybr422] {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.ndpi");
        let out = tmp.path().join("out");
        let source_jpeg = write_ndpi_restart_jpeg(&source, 128, 128, subsampling);
        let source_dct = extract_dct_blocks(&source_jpeg, DctExtractOptions::default()).unwrap();
        let (source_pixels, _) = j2k_jpeg::Decoder::new(&source_jpeg)
            .unwrap()
            .decode_request(j2k_jpeg::DecodeRequest::full(j2k_jpeg::PixelFormat::Rgb8))
            .unwrap();

        let report = export_dicom(ExportRequest {
            source_path: source,
            output_dir: out,
            options: ExportOptions {
                tile_size: 32,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: Some(0),
        })
        .unwrap();

        assert_eq!(report.instances.len(), 1);
        assert_eq!(report.instances[0].frame_count, 16);
        assert_eq!(report.metrics.routes.total_frames, 16);
        assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 0);
        assert_eq!(report.metrics.routes.jpeg_retile_frames, 16);
        assert_eq!(report.metrics.routes.jpeg_retile_rejected_frames, 0);
        assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
        assert_eq!(report.metrics.routes.jpeg_cpu_encode_frames, 0);
        assert_eq!(report.metrics.routes.cpu_input_frames, 0);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::JpegBaseline8Bit.uid()
        );
        assert_eq!(
            object.element(tags::ROWS).unwrap().to_int::<u32>().unwrap(),
            32
        );
        assert_eq!(
            object
                .element(tags::COLUMNS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            32
        );
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 16);
        for (index, fragment) in fragments.iter().enumerate() {
            let retiled = extract_dct_blocks(
                dicom_fragment_jpeg_payload(fragment),
                DctExtractOptions::default(),
            )
            .unwrap();
            assert_eq!((retiled.width, retiled.height), (32, 32));
            assert_eq!(retiled.components.len(), source_dct.components.len());
            // New JPEG edges change chroma interpolation neighbors. Exact
            // coefficients and quantization tables verify lossless image content
            // across the native-grid boundary without assuming identical RGB edges.
            for (actual, original) in retiled.components.iter().zip(&source_dct.components) {
                assert_eq!(
                    (actual.h_samp, actual.v_samp),
                    (original.h_samp, original.v_samp)
                );
                assert_eq!(actual.quant_table, original.quant_table);
                assert_eq!(actual.block_cols * 4, original.block_cols);
                assert_eq!(actual.block_rows * 4, original.block_rows);
                let source_col = index % 4 * actual.block_cols as usize;
                let source_row = index / 4 * actual.block_rows as usize;
                for row in 0..actual.block_rows as usize {
                    let start = row * actual.block_cols as usize;
                    let source_start =
                        (source_row + row) * original.block_cols as usize + source_col;
                    assert_eq!(
                        actual.quantized_blocks[start..start + actual.block_cols as usize],
                        original.quantized_blocks
                            [source_start..source_start + actual.block_cols as usize],
                        "retiled NDPI frame {index}, component {}, block row {row}",
                        actual.component_index
                    );
                }
            }
            if subsampling == JpegSubsampling::Ybr444 {
                let (decoded, _) = j2k_jpeg::Decoder::new(dicom_fragment_jpeg_payload(fragment))
                    .unwrap()
                    .decode_request(j2k_jpeg::DecodeRequest::full(j2k_jpeg::PixelFormat::Rgb8))
                    .unwrap();
                for row in 0..32 {
                    let source_offset = ((index / 4 * 32 + row) * 128 + index % 4 * 32) * 3;
                    assert_eq!(
                        &decoded.as_slice()[row * 32 * 3..(row + 1) * 32 * 3],
                        &source_pixels.as_slice()[source_offset..source_offset + 32 * 3],
                        "4:4:4 NDPI pixels at output frame {index}, row {row}"
                    );
                }
            }
        }
    }
}

#[test]
fn export_ndpi_htj2k_rejects_color_reconstruction_for_each_frame_grid() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.ndpi");
    write_ndpi_restart_jpeg(&source, 128, 128, JpegSubsampling::Ybr422);

    for tile_size in [64_u32, 96, 128] {
        let error = export_dicom(ExportRequest {
            source_path: source.clone(),
            output_dir: tmp.path().join(format!("out-{tile_size}")),
            options: ExportOptions {
                tile_size,
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                pixel_reencode: PixelReencodePolicy::Never,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: Some(0),
        })
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("pixel re-encoding fallback is disabled"),
            "tile size {tile_size}: {error}"
        );
        assert!(error.contains("ProfileUnsupported"), "{error}");
        assert!(
            std::fs::read_dir(tmp.path().join(format!("out-{tile_size}")))
                .unwrap()
                .all(|entry| entry
                    .unwrap()
                    .path()
                    .extension()
                    .is_none_or(|ext| ext != "dcm"))
        );
    }
}

#[test]
fn profile_ndpi_htj2k_rejects_implicit_rgb_reconstruction() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.ndpi");
    write_ndpi_restart_jpeg(&source, 128, 128, JpegSubsampling::Ybr422);

    let error = profile_dicom_routes(RouteProfileRequest {
        source_path: source,
        options: ExportOptions {
            tile_size: 64,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            pixel_reencode: PixelReencodePolicy::Never,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        level: 0,
        max_frames: 2,
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("pixel re-encoding fallback is disabled"),
        "{error}"
    );
    assert!(error.contains("ProfileUnsupported"), "{error}");
}

#[test]
fn profile_dicom_routes_reports_jpeg_baseline_passthrough_without_writing_dicom() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg_a = encode_test_jpeg(8, 8, [160, 20, 40]);
    let jpeg_b = encode_test_jpeg(8, 8, [20, 160, 40]);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 16, 8, 8, 8, &[jpeg_a, jpeg_b]);

    let report = profile_dicom_routes(RouteProfileRequest {
        source_path: source,
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        level: 0,
        max_frames: 2,
    })
    .unwrap();

    assert_eq!(report.level, 0);
    assert_eq!(report.requested_frames, 2);
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.rgb_like_frames, 2);
    assert_eq!(report.metrics.routes.gray_frames, 0);
    assert_eq!(report.metrics.routes.bits8_frames, 2);
    assert_eq!(report.metrics.routes.bits16_frames, 0);
    assert_eq!(report.metrics.route_passthrough_frames(), 2);
    assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
    assert_eq!(report.metrics.routes.gpu_transcode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);
    assert!(report.elapsed_micros > 0);
}

#[test]
fn profile_jpeg_baseline_uses_native_source_tiles_for_passthrough() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg_a = encode_test_jpeg(8, 8, [160, 20, 40]);
    let jpeg_b = encode_test_jpeg(8, 8, [20, 160, 40]);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 16, 8, 8, 8, &[jpeg_a, jpeg_b]);

    let report = profile_dicom_routes(RouteProfileRequest {
        source_path: source,
        options: ExportOptions {
            tile_size: 16,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        level: 0,
        max_frames: 2,
    })
    .unwrap();

    assert_eq!(report.available_frames, 2);
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
    assert_eq!(report.metrics.route_passthrough_frames(), 2);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);
}

#[test]
fn export_jpeg_baseline_preserves_viewer_friendly_native_regular_geometry() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg_a = encode_test_jpeg(512, 512, [160, 20, 40]);
    let jpeg_b = encode_test_jpeg(512, 512, [20, 160, 40]);
    let source = tmp.path().join("source.tiff");
    write_tiled_jpeg_tiff(&source, 1024, 512, 512, 512, &[jpeg_a, jpeg_b]);

    let report = export_jpeg_baseline_native_geometry_for_test(source, tmp.path().join("out"));

    assert_eq!(report.instances[0].frame_count, 2);
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);

    assert_dicom_instance_geometry_for_test(&report.instances[0].path, 512, 512, 2);
}

#[test]
fn export_jpeg_baseline_preserves_native_geometry_when_first_tile_is_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg_a = encode_test_jpeg(512, 512, [160, 20, 40]);
    let jpeg_b = encode_test_jpeg(512, 512, [20, 160, 40]);
    let jpeg_c = encode_test_jpeg(512, 512, [20, 40, 160]);
    let source = tmp.path().join("source.tiff");
    write_tiled_jpeg_tiff(
        &source,
        1024,
        1024,
        512,
        512,
        &[Vec::new(), jpeg_a, jpeg_b, jpeg_c],
    );

    let report = export_jpeg_baseline_native_geometry_for_test(source, tmp.path().join("out"));

    assert_eq!(report.instances[0].frame_count, 3);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 3);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);

    assert_dicom_instance_geometry_for_test(&report.instances[0].path, 512, 512, 3);
}

#[test]
fn export_jpeg_baseline_empty_native_tile_does_not_hide_unreadable_neighbors() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.tiff");
    write_tiled_jpeg_tiff(
        &source,
        16,
        16,
        8,
        8,
        &[
            Vec::new(),
            b"invalid JPEG".to_vec(),
            encode_test_jpeg(8, 8, [20, 160, 40]),
            encode_test_jpeg(8, 8, [20, 40, 160]),
        ],
    );
    let error = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 32,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap_err();
    assert!(
        matches!(error, Error::SlideRead { .. }),
        "unreadable contributors must fail export instead of becoming a blank frame: {error}"
    );
}

#[test]
fn export_jpeg_baseline_rejects_unsupported_regular_tiff_retiling() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(1024, 1024, [160, 20, 40]);
    let source = tmp.path().join("source.tiff");
    write_tiled_jpeg_tiff(&source, 1024, 1024, 1024, 1024, &[jpeg]);

    let error = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("pixel re-encoding fallback is disabled"),
        "{error}"
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
fn profile_jpeg_baseline_rejects_unsupported_regular_tiff_retiling() {
    let tmp = tempfile::tempdir().unwrap();
    let tiles = [
        encode_test_jpeg(16, 2, [160, 20, 40]),
        encode_test_jpeg(16, 2, [20, 160, 40]),
        encode_test_jpeg(16, 2, [40, 20, 160]),
        encode_test_jpeg(16, 2, [160, 160, 40]),
    ];
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 32, 4, 16, 2, &tiles);

    let error = profile_dicom_routes(RouteProfileRequest {
        source_path: source,
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        source_aware_transfer_syntax: false,
        level: 0,
        max_frames: 2,
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("pixel re-encoding fallback is disabled"),
        "{error}"
    );
}

fn encode_restart_test_jpeg(width: u32, height: u32, subsampling: JpegSubsampling) -> Vec<u8> {
    let mut pixels = vec![0u8; width as usize * height as usize * 3];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 3) as usize;
            pixels[idx] = 32u8.saturating_add((x % 96) as u8);
            pixels[idx + 1] = 48u8.saturating_add((y % 96) as u8);
            pixels[idx + 2] = 96u8.saturating_add(((x + y) % 96) as u8);
        }
    }
    j2k_jpeg::encode_jpeg_baseline(
        JpegSamples::Rgb8 {
            data: &pixels,
            width,
            height,
        },
        j2k_jpeg::JpegEncodeOptions {
            quality: 90,
            subsampling,
            restart_interval: Some(8),
            backend: JpegBackend::Cpu,
        },
    )
    .unwrap()
    .data
}

fn write_ndpi_restart_jpeg(
    path: &std::path::Path,
    width: u32,
    height: u32,
    subsampling: JpegSubsampling,
) -> Vec<u8> {
    let encoded = encode_restart_test_jpeg(width, height, subsampling);
    let entropy_start = find_test_jpeg_bitstream_start(&encoded).unwrap();
    let mcu_starts = test_jpeg_restart_segment_starts(&encoded);
    assert!(!mcu_starts.is_empty());
    let jpeg_header = &encoded[..entropy_start];

    let mut buf = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    let first_ifd_pos = buf.len();
    buf.extend_from_slice(&0u32.to_le_bytes());

    let strip_offset = buf.len() as u32;
    buf.extend_from_slice(&encoded);
    let strip_byte_count = buf.len() as u32 - strip_offset;

    let mcu_starts_array_offset = buf.len() as u32;
    for value in &mcu_starts {
        buf.extend_from_slice(&value.to_le_bytes());
    }
    let x_resolution_offset = buf.len() as u32;
    buf.extend_from_slice(&40_000u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    let y_resolution_offset = buf.len() as u32;
    buf.extend_from_slice(&40_000u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());

    let ifd_offset = buf.len() as u32;
    buf[first_ifd_pos..first_ifd_pos + 4].copy_from_slice(&ifd_offset.to_le_bytes());
    let mut tags = vec![
        tiff_tag(256, 4, 1, width.to_le_bytes()),
        tiff_tag(257, 4, 1, height.to_le_bytes()),
        tiff_tag(259, 3, 1, tiff_short_value(7)),
        tiff_tag(262, 3, 1, tiff_short_value(6)),
        tiff_tag(273, 4, 1, strip_offset.to_le_bytes()),
        tiff_tag(277, 3, 1, tiff_short_value(3)),
        tiff_tag(279, 4, 1, strip_byte_count.to_le_bytes()),
        tiff_tag(282, 5, 1, x_resolution_offset.to_le_bytes()),
        tiff_tag(283, 5, 1, y_resolution_offset.to_le_bytes()),
        tiff_tag(296, 3, 1, tiff_short_value(3)),
        tiff_tag(65420, 4, 1, 1u32.to_le_bytes()),
        tiff_tag(65421, 11, 1, 40.0f32.to_le_bytes()),
        tiff_tag(
            65426,
            4,
            mcu_starts.len() as u32,
            mcu_starts_array_offset.to_le_bytes(),
        ),
    ];
    tags.sort_by_key(|tag| tag.0);

    buf.extend_from_slice(&(tags.len() as u16).to_le_bytes());
    for (tag, typ, count, value) in &tags {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        buf.extend_from_slice(value);
    }
    buf.extend_from_slice(&0u64.to_le_bytes());

    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(&buf).unwrap();
    file.flush().unwrap();

    assert!(jpeg_header.starts_with(&[0xFF, 0xD8]));
    encoded
}

fn find_test_jpeg_bitstream_start(data: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < data.len().saturating_sub(1) {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        if marker == 0xD8 || marker == 0x00 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        if i + 3 >= data.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if marker == 0xDA {
            return Some(i + 2 + seg_len);
        }
        i += 2 + seg_len;
    }
    None
}

fn test_jpeg_restart_segment_starts(data: &[u8]) -> Vec<u32> {
    let mut starts = Vec::new();
    if let Some(entropy_start) = find_test_jpeg_bitstream_start(data) {
        starts.push(entropy_start as u32);
    }
    let mut i = starts.first().copied().unwrap_or(0) as usize;
    while i + 1 < data.len() {
        if data[i] == 0xFF && (0xD0..=0xD7).contains(&data[i + 1]) {
            starts.push(i as u32);
            i += 2;
            continue;
        }
        i += 1;
    }
    starts
}
