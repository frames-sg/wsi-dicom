use std::io::Write;

use signinum_jpeg::{JpegBackend, JpegSamples, JpegSubsampling};

use super::*;

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
fn export_htj2k_from_jpeg_strips_writes_regular_generated_frames() {
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

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 8,
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

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.instances[0].frame_count, 4);
    assert_eq!(report.metrics.routes.total_frames, 4);
    assert_eq!(report.metrics.routes.cpu_input_frames, 4);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 0);
    assert_eq!(report.metrics.route_passthrough_frames(), 0);
    assert_eq!(report.metrics.routes.j2k_passthrough_frames, 0);
    assert_eq!(
        report.metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames,
        0
    );
    assert_eq!(
        report
            .metrics
            .jpeg_direct_htj2k
            .jpeg_direct_htj2k_rejected_frames,
        2
    );
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 4);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax.trim_end_matches('\0'),
        TransferSyntax::Htj2kLosslessRpcl.uid()
    );
    assert_eq!(
        object.element(tags::ROWS).unwrap().to_int::<u32>().unwrap(),
        8
    );
    assert_eq!(
        object
            .element(tags::COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        8
    );
    assert_eq!(
        object
            .element(tags::NUMBER_OF_FRAMES)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        4
    );
    assert_eq!(
        object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn export_ndpi_jpeg_baseline_retiles_restart_strips_without_decode_encode() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.ndpi");
    let out = tmp.path().join("out");
    write_ndpi_restart_jpeg(&source, 128, 128);

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 128,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap();

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.instances[0].frame_count, 1);
    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_retile_frames, 1);
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
        128
    );
    assert_eq!(
        object
            .element(tags::COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        128
    );
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 1);
    assert!(dicom_fragment_jpeg_payload(&fragments[0]).starts_with(&[0xFF, 0xD8]));
}

#[test]
fn export_ndpi_htj2k_rpcl_retiles_jpeg_then_direct_53() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.ndpi");
    let out = tmp.path().join("out");
    write_ndpi_restart_jpeg(&source, 128, 128);

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 128,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap();

    assert_eq!(report.instances.len(), 1);
    assert_eq!(report.instances[0].frame_count, 1);
    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.jpeg_retile_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_retile_to_htj2k_53_frames, 0);
    assert_eq!(
        report.metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames,
        0
    );
    assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_cpu_encode_frames, 0);
    assert_eq!(report.metrics.routes.cpu_input_frames, 1);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 1);
    assert_eq!(report.metrics.route_passthrough_frames(), 0);
    assert_eq!(report.metrics.route_unclassified_frames(), 0);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.meta().transfer_syntax.trim_end_matches('\0'),
        TransferSyntax::Htj2kLosslessRpcl.uid()
    );
    assert_eq!(
        object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap()
            .len(),
        1
    );
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
    let out = tmp.path().join("out");

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap();

    assert_eq!(report.instances[0].frame_count, 2);
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.element(tags::ROWS).unwrap().to_int::<u32>().unwrap(),
        512
    );
    assert_eq!(
        object
            .element(tags::COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        512
    );
    assert_eq!(
        object
            .element(tags::NUMBER_OF_FRAMES)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        2
    );
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
    let out = tmp.path().join("out");

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap();

    assert_eq!(report.instances[0].frame_count, 4);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 3);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 1);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.element(tags::ROWS).unwrap().to_int::<u32>().unwrap(),
        512
    );
    assert_eq!(
        object
            .element(tags::COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        512
    );
    assert_eq!(
        object
            .element(tags::NUMBER_OF_FRAMES)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        4
    );
}

#[test]
fn export_jpeg_baseline_retiles_oversized_native_regular_geometry() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(1024, 1024, [160, 20, 40]);
    let source = tmp.path().join("source.tiff");
    write_tiled_jpeg_tiff(&source, 1024, 1024, 1024, 1024, &[jpeg]);
    let out = tmp.path().join("out");

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap();

    assert_eq!(report.instances[0].frame_count, 4);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 4);

    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    assert_eq!(
        object.element(tags::ROWS).unwrap().to_int::<u32>().unwrap(),
        512
    );
    assert_eq!(
        object
            .element(tags::COLUMNS)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        512
    );
    assert_eq!(
        object
            .element(tags::NUMBER_OF_FRAMES)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        4
    );
}

#[test]
fn profile_jpeg_baseline_retiles_pathological_native_regular_source_tiles() {
    let tmp = tempfile::tempdir().unwrap();
    let tiles = [
        encode_test_jpeg(16, 2, [160, 20, 40]),
        encode_test_jpeg(16, 2, [20, 160, 40]),
        encode_test_jpeg(16, 2, [40, 20, 160]),
        encode_test_jpeg(16, 2, [160, 160, 40]),
    ];
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 32, 4, 16, 2, &tiles);

    let report = profile_dicom_routes(RouteProfileRequest {
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
    .unwrap();

    assert_eq!(report.available_frames, 4);
    assert_eq!(report.metrics.routes.total_frames, 2);
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 0);
    assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 2);
    assert_eq!(report.metrics.route_passthrough_frames(), 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 2);
}

fn encode_restart_test_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; width as usize * height as usize * 3];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 3) as usize;
            pixels[idx] = 32u8.saturating_add((x % 96) as u8);
            pixels[idx + 1] = 48u8.saturating_add((y % 96) as u8);
            pixels[idx + 2] = 96u8.saturating_add(((x + y) % 96) as u8);
        }
    }
    signinum_jpeg::encode_jpeg_baseline(
        JpegSamples::Rgb8 {
            data: &pixels,
            width,
            height,
        },
        signinum_jpeg::JpegEncodeOptions {
            quality: 90,
            subsampling: JpegSubsampling::Ybr422,
            restart_interval: Some(8),
            backend: JpegBackend::Cpu,
        },
    )
    .unwrap()
    .data
}

fn write_ndpi_restart_jpeg(path: &std::path::Path, width: u32, height: u32) {
    let encoded = encode_restart_test_jpeg(width, height);
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
