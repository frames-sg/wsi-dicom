use super::*;
use wsi_rs::TileRequest;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) static DEVICE_DECODE_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn test_jpeg_baseline_fallback_frame(col: u64) -> JpegBaselineFallbackFrame {
    JpegBaselineFallbackFrame {
        x: col * 4,
        y: 0,
        width: 4,
        height: 4,
    }
}

pub(super) fn test_rgb8_pixel_profile() -> PixelProfile {
    PixelProfile {
        components: 3,
        bits_allocated: 8,
        photometric_interpretation: "RGB",
    }
}

pub(super) fn test_lossless_j2k_planned_frame(col: u64) -> LosslessJ2kPlannedFrame {
    LosslessJ2kPlannedFrame {
        row: 0,
        col,
        x: col * 4,
        y: 0,
        width: 4,
        height: 4,
        source_j2k_dimensions: None,
        source_j2k_syntax: None,
        source_j2k_profile: None,
        source_j2k: None,
        source_jpeg: None,
        source_jpeg_retiled: false,
        source_jpeg_retile_duration: Duration::ZERO,
        source_jpeg_retile_rejection: None,
        source_jpeg_direct_rejected: false,
        source_raw_probe_failed: false,
        passthrough: None,
    }
}

pub(super) fn assert_j2k_facade_roundtrip(samples: J2kLosslessSamples<'_>, codestream: &[u8]) {
    let mut decoder = j2k::J2kDecoder::new(codestream).expect("parse encoded J2K");
    let bytes_per_sample = if samples.bit_depth <= 8 {
        1usize
    } else {
        2usize
    };
    let stride = samples.width as usize * samples.components as usize * bytes_per_sample;
    let mut decoded = vec![0; stride * samples.height as usize];
    let fmt = match (samples.components, samples.bit_depth) {
        (1, 8) => j2k::PixelFormat::Gray8,
        (3, 8) => j2k::PixelFormat::Rgb8,
        (1, 16) => j2k::PixelFormat::Gray16,
        (3, 16) => j2k::PixelFormat::Rgb16,
        _ => panic!(
            "unsupported test sample profile: components={} bit_depth={}",
            samples.components, samples.bit_depth
        ),
    };
    decoder
        .decode_into(&mut decoded, stride, fmt)
        .expect("decode encoded J2K");

    assert_eq!(decoded, samples.data);
}

pub(super) fn write_general_j2k_ycbcr_passthrough_tiff_for_test(
    source_path: &std::path::Path,
    sample_multiplier: u32,
) -> Vec<u8> {
    let bytes = (0..12u32)
        .map(|value| ((value * sample_multiplier) & 0xFF) as u8)
        .collect::<Vec<_>>();
    let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
    let codestream = encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();
    write_tiled_jp2k_ycbcr_tiff(source_path, 2, 2, 2, 2, std::slice::from_ref(&codestream));
    codestream
}

pub(super) fn export_general_j2k_passthrough_for_test(
    source_path: std::path::PathBuf,
    output_dir: std::path::PathBuf,
) -> ExportReport {
    export_dicom(ExportRequest {
        source_path,
        output_dir,
        options: ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::Jpeg2000,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: true,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap()
}

pub(super) fn assert_single_j2k_passthrough_avoids_gpu_for_test(report: &ExportReport) {
    assert_eq!(report.metrics.routes.total_frames, 1);
    assert_eq!(report.metrics.routes.j2k_passthrough_frames, 1);
    assert_eq!(report.metrics.routes.cpu_input_frames, 0);
    assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
    assert_eq!(report.metrics.routes.gpu_encode_frames, 0);
    assert_eq!(report.metrics.routes.gpu_input_decode_batches, 0);
    assert_eq!(report.metrics.routes.gpu_compose_batches, 0);
    assert_eq!(report.metrics.routes.gpu_encode_batches, 0);
    assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);
}

pub(super) struct J2kEdgeFallbackCodestreamsForTest {
    pub(super) interior: Vec<u8>,
    pub(super) edge: Vec<u8>,
}

pub(super) fn j2k_edge_fallback_codestreams_for_test() -> J2kEdgeFallbackCodestreamsForTest {
    let interior_bytes: Vec<u8> = (0..2 * 2 * 3)
        .map(|value| ((value * 7) & 0xFF) as u8)
        .collect();
    let interior_samples =
        J2kLosslessSamples::new(&interior_bytes, 2, 2, 3, 8, false).expect("valid samples");
    let interior = encode_dicom_lossless(
        interior_samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();

    let edge_bytes: Vec<u8> = (0..6).map(|value| ((value * 11) & 0xFF) as u8).collect();
    let edge_samples =
        J2kLosslessSamples::new(&edge_bytes, 1, 2, 3, 8, false).expect("valid edge samples");
    let edge = encode_dicom_lossless(
        edge_samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap();

    J2kEdgeFallbackCodestreamsForTest { interior, edge }
}

pub(super) struct Htj2kRpclDicomSourceForTest {
    pub(super) path: std::path::PathBuf,
    pub(super) fragments: Vec<Vec<u8>>,
}

pub(super) fn write_htj2k_rpcl_dicom_source_for_test(
    work_dir: &std::path::Path,
    sop_instance_uid: &str,
    width: u32,
    height: u32,
) -> Htj2kRpclDicomSourceForTest {
    let raw_source = work_dir.join("source.dcm");
    write_source_dicom_with_dimensions(&raw_source, sop_instance_uid, width, height);

    let source_report = export_dicom(ExportRequest {
        source_path: raw_source,
        output_dir: work_dir.join("source-dicom"),
        options: ExportOptions {
            tile_size: 2,
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
    let path = source_report.instances[0].path.clone();
    let object = dicom_object::open_file(&path).unwrap();
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap()
        .iter()
        .map(|fragment| dicom_fragment_payload_without_padding(fragment).to_vec())
        .collect::<Vec<_>>();

    Htj2kRpclDicomSourceForTest { path, fragments }
}

pub(super) fn export_htj2k_rpcl_dicom_passthrough_for_test(
    source: &Htj2kRpclDicomSourceForTest,
    output_dir: std::path::PathBuf,
) -> ExportReport {
    export_dicom(ExportRequest {
        source_path: source.path.clone(),
        output_dir,
        options: ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap()
}

pub(super) struct ExternalJ2kDecoderFrameForTest {
    pub(super) expected_pixels: Vec<u8>,
    pub(super) codestream_path: std::path::PathBuf,
    pub(super) ppm_path: std::path::PathBuf,
}

pub(super) fn write_external_j2k_decoder_frame_for_test(
    work_dir: &std::path::Path,
    sop_instance_uid: &str,
    transfer_syntax: TransferSyntax,
) -> ExternalJ2kDecoderFrameForTest {
    let source = work_dir.join("source.dcm");
    let expected_pixels = vec![
        255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
    ];
    write_source_dicom_with_pixels(&source, sop_instance_uid, 3, 2, expected_pixels.clone());

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: work_dir.join("out"),
        options: ExportOptions {
            tile_size: 3,
            transfer_syntax,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
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

    let codestream_path = work_dir.join("frame.j2k");
    let ppm_path = work_dir.join("frame.ppm");
    std::fs::write(
        &codestream_path,
        dicom_fragment_payload_without_padding(&fragments[0]),
    )
    .unwrap();

    ExternalJ2kDecoderFrameForTest {
        expected_pixels,
        codestream_path,
        ppm_path,
    }
}

pub(super) fn assert_external_decoder_ppm_matches_source_for_test(
    ppm_path: &std::path::Path,
    expected_pixels: &[u8],
) {
    let decoded = read_binary_ppm_for_test(ppm_path);

    assert_eq!(decoded.0, 3);
    assert_eq!(decoded.1, 3);
    assert_eq!(&decoded.2[..expected_pixels.len()], expected_pixels);
    assert_eq!(&decoded.2[expected_pixels.len()..], &[0; 9]);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn auto_route_candidate(complete: bool, micros: u64) -> AutoLosslessJ2kRouteCandidate {
    AutoLosslessJ2kRouteCandidate {
        complete,
        duration: Duration::from_micros(micros),
    }
}

pub(super) fn ndpi_jpeg_passthrough_level(
    slide: &Slide,
    tile_size: u32,
) -> (usize, JpegBaselineFrameGeometry) {
    let levels = &slide.dataset().scenes[0].series[0].levels;
    let mut best = None;
    for (level_idx, level) in levels.iter().enumerate() {
        let Ok(geometry) = jpeg_baseline_frame_geometry(level, tile_size) else {
            continue;
        };
        let Ok(frame_count) = geometry
            .tiles_across
            .checked_mul(geometry.tiles_down)
            .ok_or(())
        else {
            continue;
        };
        let Ok(raw) = slide.read_raw_compressed_tile(&TileRequest::new(
            0usize,
            0usize,
            level_idx as u32,
            0,
            0,
        )) else {
            continue;
        };
        if !raw_jpeg_matches_frame_geometry(&raw, geometry.frame_columns, geometry.frame_rows) {
            continue;
        }
        let Ok(profile) = pixel_profile_from_raw_jpeg_tile(&raw) else {
            continue;
        };
        if !raw_jpeg_profile_can_passthrough(
            profile,
            raw_rgb_passthrough_has_no_geometry_fallback(level, geometry),
        ) {
            continue;
        }
        if best
            .map(|(_, _, best_frame_count)| frame_count < best_frame_count)
            .unwrap_or(true)
        {
            best = Some((level_idx, geometry, frame_count));
        }
    }
    best.map(|(level_idx, geometry, _)| (level_idx, geometry))
        .expect("NDPI fixture did not expose any full JPEG Baseline passthrough level")
}

pub(super) fn ndpi_jpeg_passthrough_levels(
    slide: &Slide,
    tile_size: u32,
) -> Vec<(usize, JpegBaselineFrameGeometry)> {
    let mut levels = Vec::new();
    for (level_idx, level) in slide.dataset().scenes[0].series[0]
        .levels
        .iter()
        .enumerate()
    {
        let Ok(geometry) = jpeg_baseline_frame_geometry(level, tile_size) else {
            continue;
        };
        let Ok(raw) = slide.read_raw_compressed_tile(&TileRequest::new(
            0usize,
            0usize,
            level_idx as u32,
            0,
            0,
        )) else {
            continue;
        };
        if !raw_jpeg_matches_frame_geometry(&raw, geometry.frame_columns, geometry.frame_rows) {
            continue;
        }
        let Ok(profile) = pixel_profile_from_raw_jpeg_tile(&raw) else {
            continue;
        };
        if raw_jpeg_profile_can_passthrough(
            profile,
            raw_rgb_passthrough_has_no_geometry_fallback(level, geometry),
        ) {
            levels.push((level_idx, geometry));
        }
    }
    levels
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn assert_aperio_jp2k_metal_input_tile_matches_cpu(tile_size: u32) {
    let Some(source) = std::env::var_os("WSI_DICOM_APERIO_JP2K_FIXTURE").map(PathBuf::from) else {
        return;
    };
    std::env::set_var("WSI_RS_JP2K_DEVICE_DECODE", "1");

    let slide = Slide::open(&source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let TileLayout::Regular {
        tile_width,
        tile_height,
        ..
    } = level.tile_layout
    else {
        panic!("fixture first level must use a regular tiled source layout");
    };
    if tile_size > tile_width || tile_size > tile_height {
        assert!(tile_width < tile_size || tile_height < tile_size);
    }
    let mut metal_input = MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Htj2kLosslessRpcl,
        CodecValidation::RoundTrip,
    );

    let mut encoded = try_encode_metal_input_tile_run(
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
        1,
        level.dimensions.0,
        level.dimensions.1,
        tile_size,
    )
    .unwrap();

    assert_eq!(encoded.tiles.len(), 1);
    assert!(encoded.input_decode_duration > Duration::ZERO);
    if tile_size > tile_width || tile_size > tile_height {
        assert!(encoded.compose_duration > Duration::ZERO);
    } else {
        assert_eq!(encoded.compose_duration, Duration::ZERO);
    }
    let (frame, profile) = encoded.tiles.remove(0).expect("resident Metal frame");
    assert!(frame.used_device_encode);
    assert!(frame.used_device_validation);
    assert!(frame.codestream_is_metal_buffer_backed());
    assert_transfer_syntax_codestream(
        TransferSyntax::Htj2kLosslessRpcl,
        frame.codestream_bytes().expect("codestream bytes").as_ref(),
    );

    let cpu_region = slide
        .read_region(&RegionRequest::new(
            0usize,
            0usize,
            0u32,
            (0, 0),
            (tile_size, tile_size),
        ))
        .unwrap();
    let expected = prepare_tile_samples(&cpu_region, tile_size, tile_size).unwrap();
    let actual = decode_j2k_frame_for_test(
        frame.codestream_bytes().expect("codestream bytes").as_ref(),
        tile_size,
        tile_size,
        profile.components,
        profile.bits_allocated,
    );
    if actual != expected.bytes {
        let max_abs_diff = actual
            .iter()
            .zip(expected.bytes.iter())
            .map(|(actual, expected)| actual.abs_diff(*expected))
            .max()
            .unwrap_or(0);
        let mismatches = actual
            .iter()
            .zip(expected.bytes.iter())
            .filter(|(actual, expected)| actual != expected)
            .count();
        let first_mismatch = actual
            .iter()
            .zip(expected.bytes.iter())
            .position(|(actual, expected)| actual != expected)
            .expect("mismatch exists");
        let pixel = first_mismatch / usize::from(profile.components);
        let x = pixel % tile_size as usize;
        let y = pixel / tile_size as usize;
        let channel = first_mismatch % usize::from(profile.components);
        panic!(
                "Metal input tile mismatch for tile_size={tile_size} at x={x}, y={y}, channel={channel}: actual={}, expected={}, max_abs_diff={max_abs_diff}, mismatches={mismatches}, len={}",
                actual[first_mismatch],
                expected.bytes[first_mismatch],
                actual.len()
            );
    }
}

pub(super) fn assert_transfer_syntax_codestream(
    transfer_syntax: TransferSyntax,
    codestream: &[u8],
) {
    match transfer_syntax {
        TransferSyntax::Jpeg2000Lossless => {}
        TransferSyntax::Htj2kLossless => {
            assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
        }
        TransferSyntax::Htj2kLosslessRpcl => {
            let cod_offset = codestream
                .windows(2)
                .position(|window| window == [0xFF, 0x52])
                .expect("COD marker");
            assert_eq!(codestream[cod_offset + 5], 0x02);
            assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
            assert!(codestream.windows(2).any(|window| window == [0xFF, 0x55]));
        }
        TransferSyntax::JpegBaseline8Bit
        | TransferSyntax::Jpeg2000
        | TransferSyntax::Htj2k
        | TransferSyntax::ExplicitVrLittleEndian => {
            panic!("non-JPEG 2000 transfer syntax in lossless J2K fixture test");
        }
    }
}

pub(super) fn decode_j2k_frame_for_test(
    codestream: &[u8],
    width: u32,
    height: u32,
    components: u8,
    bits_allocated: u16,
) -> Vec<u8> {
    let fmt = match (components, bits_allocated) {
        (1, 8) => j2k::PixelFormat::Gray8,
        (3, 8) => j2k::PixelFormat::Rgb8,
        (1, 16) => j2k::PixelFormat::Gray16,
        (3, 16) => j2k::PixelFormat::Rgb16,
        other => panic!("unsupported frame profile: {other:?}"),
    };
    let bytes_per_sample = if bits_allocated <= 8 { 1usize } else { 2usize };
    let stride = width as usize * components as usize * bytes_per_sample;
    let mut decoder = j2k::J2kDecoder::new(codestream).unwrap_or_else(|err| {
        if codestream.last() == Some(&0) {
            j2k::J2kDecoder::new(&codestream[..codestream.len() - 1])
                .unwrap_or_else(|_| panic!("parse frame: {err}"))
        } else {
            panic!("parse frame: {err}");
        }
    });
    let mut decoded = vec![0; stride * height as usize];
    decoder.decode_into(&mut decoded, stride, fmt).unwrap();
    decoded
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn metal_test_tile(
    device: &metal::Device,
    bytes: &[u8],
    width: u32,
    height: u32,
    format: J2kPixelFormat,
) -> wsi_rs::output::metal::MetalDeviceTile {
    crate::metal_interop::test_tile_from_shared_bytes(device, bytes, width, height, format)
}

pub(super) fn write_source_dicom(path: &std::path::Path) {
    write_source_dicom_with_pixels(
        path,
        "1.2.826.0.1.3680043.10.999.1",
        3,
        2,
        vec![
            255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
        ],
    );
}

pub(super) fn write_source_dicom_with_dimensions(
    path: &std::path::Path,
    sop_instance_uid: &str,
    width: u32,
    height: u32,
) {
    let pixels = crate::synthetic_source::deterministic_rgb_pixels(width, height);
    write_source_dicom_with_pixels(path, sop_instance_uid, width, height, pixels);
}

pub(super) fn write_source_dicom_with_pixels(
    path: &std::path::Path,
    sop_instance_uid: &str,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) {
    assert_eq!(pixels.len(), (width as usize) * (height as usize) * 3);
    crate::synthetic_source::write_rgb_source_dicom(
        path,
        sop_instance_uid,
        "1.2.826.0.1.3680043.10.999",
        width,
        height,
        pixels,
    )
    .unwrap();
}

pub(super) fn j2k_view_dimensions(codestream: &[u8]) -> (u32, u32) {
    let view = J2kView::parse(codestream).expect("parse J2K view");
    view.info().dimensions
}

pub(super) fn j2k_passthrough_transfer_syntax(codestream: &[u8]) -> CompressedTransferSyntax {
    J2kView::parse(codestream)
        .expect("parse J2K view")
        .passthrough_candidate()
        .expect("passthrough candidate")
        .transfer_syntax()
}

pub(super) fn j2k_cod_decomposition_levels(codestream: &[u8]) -> u8 {
    let cod_offset = codestream
        .windows(2)
        .position(|window| window == [0xFF, 0x52])
        .expect("COD marker");
    codestream[cod_offset + 9]
}

pub(super) fn j2k_cod_mct(codestream: &[u8]) -> u8 {
    let cod_offset = codestream
        .windows(2)
        .position(|window| window == [0xFF, 0x52])
        .expect("COD marker");
    codestream[cod_offset + 8]
}

pub(super) fn patch_j2k_cod_wavelet_transform(codestream: &mut [u8], transform: u8) {
    let cod_offset = codestream
        .windows(2)
        .position(|window| window == [0xFF, 0x52])
        .expect("COD marker");
    codestream[cod_offset + 13] = transform;
}

pub(super) fn first_pixel_data_fragment_payload_len(path: &std::path::Path) -> usize {
    let object = dicom_object::open_file(path).unwrap();
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    dicom_fragment_jpeg_payload(&fragments[0]).len()
}

pub(super) fn dicom_fragment_jpeg_payload(fragment: &[u8]) -> &[u8] {
    if fragment.len() >= 3
        && fragment.last() == Some(&0)
        && fragment[fragment.len() - 3..fragment.len() - 1] == [0xFF, 0xD9]
    {
        &fragment[..fragment.len() - 1]
    } else {
        fragment
    }
}

pub(super) fn run_dicom_validators_for_test(path: &std::path::Path) {
    let mut ran = false;
    if let Some(dciodvfy) = find_command_for_test("dciodvfy") {
        run_dicom_validator_for_test("dciodvfy", &dciodvfy, &["-new"], &[path]);
        ran = true;
    } else {
        eprintln!("skipping dciodvfy validation: dciodvfy not found");
    }
    if let Some(dcentvfy) = find_command_for_test("dcentvfy") {
        run_dicom_validator_for_test("dcentvfy", &dcentvfy, &[], &[path]);
        ran = true;
    } else {
        eprintln!("skipping dcentvfy validation: dcentvfy not found");
    }
    if !ran {
        eprintln!("skipping external DICOM validator smoke: no DICOM validators found");
    }
}

pub(super) fn run_htj2k_dicom_validators_for_test(path: &std::path::Path) {
    let object = dicom_object::open_file(path).expect("open exported HTJ2K DICOM");
    let transfer_syntax = object.meta().transfer_syntax.trim_end_matches('\0');
    assert!(
        matches!(
            transfer_syntax,
            "1.2.840.10008.1.2.4.201" | "1.2.840.10008.1.2.4.202" | "1.2.840.10008.1.2.4.203"
        ),
        "expected an HTJ2K transfer syntax, got {transfer_syntax}"
    );

    let mut ran = false;
    if let Some(dciodvfy) = find_command_for_test("dciodvfy") {
        let output = run_dicom_validator_command_for_test(&dciodvfy, &["-new"], &[path]);
        if validator_output_succeeded(&output) {
            ran = true;
        } else if dciodvfy_lacks_htj2k_transfer_syntax_support(&output) {
            eprintln!(
                "dciodvfy is installed but does not recognize the HTJ2K transfer syntax; \
                 retaining dcentvfy structural validation and the separate Grok decode gate"
            );
        } else {
            assert_validator_output_succeeded("dciodvfy", &output);
        }
    } else {
        eprintln!("skipping dciodvfy validation: dciodvfy not found");
    }
    if let Some(dcentvfy) = find_command_for_test("dcentvfy") {
        run_dicom_validator_for_test("dcentvfy", &dcentvfy, &[], &[path]);
        ran = true;
    } else {
        eprintln!("skipping dcentvfy validation: dcentvfy not found");
    }
    if !ran {
        eprintln!("skipping external HTJ2K DICOM validator smoke: no capable validator found");
    }
}

pub(super) fn run_dicom_validator_for_test(
    name: &str,
    command: &str,
    args: &[&str],
    paths: &[&std::path::Path],
) {
    let output = run_dicom_validator_command_for_test(command, args, paths);
    assert_validator_output_succeeded(name, &output);
}

fn run_dicom_validator_command_for_test(
    command: &str,
    args: &[&str],
    paths: &[&std::path::Path],
) -> std::process::Output {
    std::process::Command::new(command)
        .args(args)
        .args(paths)
        .output()
        .unwrap()
}

fn validator_output_succeeded(output: &std::process::Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let has_error = stdout
        .lines()
        .chain(stderr.lines())
        .any(|line| line.trim_start().starts_with("Error"));
    output.status.success() && !has_error
}

fn assert_validator_output_succeeded(name: &str, output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        validator_output_succeeded(output),
        "{name} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        stdout,
        stderr
    );
}

fn dciodvfy_lacks_htj2k_transfer_syntax_support(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    dciodvfy_errors_are_unsupported_htj2k_cascade(&stderr)
}

fn dciodvfy_errors_are_unsupported_htj2k_cascade(stderr: &str) -> bool {
    let errors: Vec<_> = stderr
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("Error"))
        .collect();

    errors.len() == 3
        && errors.iter().any(|line| {
            line.contains(
                "Undefined value length of other byte/word element is illegal in non-encapsulated transfer syntax",
            )
        })
        && errors
            .iter()
            .any(|line| line.contains("Dicom dataset read failed"))
        && errors.iter().any(|line| {
            line.contains("</PixelData(7fe0,0010)>")
                && line.contains("Missing attribute for Type 1C Conditional")
        })
}

#[test]
fn dciodvfy_htj2k_capability_classifier_accepts_only_the_known_parse_cascade() {
    let unsupported = "\
Error - Undefined value length of other byte/word element is illegal in non-encapsulated transfer syntax\n\
Error - Dicom dataset read failed\n\
Error - </PixelData(7fe0,0010)> - Missing attribute for Type 1C Conditional - Module=<ImagePixel>\n";

    assert!(dciodvfy_errors_are_unsupported_htj2k_cascade(unsupported));
}

#[test]
fn dciodvfy_htj2k_capability_classifier_rejects_additional_validation_errors() {
    let invalid = "\
Error - Undefined value length of other byte/word element is illegal in non-encapsulated transfer syntax\n\
Error - Dicom dataset read failed\n\
Error - </PixelData(7fe0,0010)> - Missing attribute for Type 1C Conditional - Module=<ImagePixel>\n\
Error - </PatientID(0010,0020)> - Missing attribute for Type 2\n";

    assert!(!dciodvfy_errors_are_unsupported_htj2k_cascade(invalid));
}
