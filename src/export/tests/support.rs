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
        let Ok(raw) = slide.read_raw_compressed_tile(&TileRequest {
            scene: 0,
            series: 0,
            level: level_idx as u32,
            plane: PlaneSelection { z: 0, c: 0, t: 0 },
            col: 0,
            row: 0,
        }) else {
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
        let Ok(raw) = slide.read_raw_compressed_tile(&TileRequest {
            scene: 0,
            series: 0,
            level: level_idx as u32,
            plane: PlaneSelection { z: 0, c: 0, t: 0 },
            col: 0,
            row: 0,
        }) else {
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
    std::env::set_var("STATUMEN_JP2K_DEVICE_DECODE", "1");

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
        frame.codestream_bytes().expect("codestream bytes"),
    );

    let cpu_region = slide
        .read_region(&RegionRequest {
            scene: SceneId(0),
            series: SeriesId(0),
            level: LevelIdx(0),
            plane: PlaneIdx(PlaneSelection { z: 0, c: 0, t: 0 }),
            origin_px: (0, 0),
            size_px: (tile_size, tile_size),
        })
        .unwrap();
    let expected = prepare_tile_samples(&cpu_region, tile_size, tile_size).unwrap();
    let actual = decode_j2k_frame_for_test(
        frame.codestream_bytes().expect("codestream bytes"),
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
    let buffer = device.new_buffer_with_data(
        bytes.as_ptr().cast(),
        bytes.len() as u64,
        metal::MTLResourceOptions::StorageModeShared,
    );
    wsi_rs::output::metal::MetalDeviceTile {
        width,
        height,
        pitch_bytes: width as usize * format.bytes_per_pixel(),
        format,
        storage: wsi_rs::output::metal::MetalDeviceStorage::Buffer {
            buffer,
            byte_offset: 0,
        },
    }
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
    let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 3);
    for y in 0..height {
        for x in 0..width {
            pixels.push((x * 37 + y * 11) as u8);
            pixels.push((x * 17 + y * 29) as u8);
            pixels.push((x * 7 + y * 43) as u8);
        }
    }
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
    let mut object = InMemDicomObject::new_empty();
    object.put(DataElement::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE,
    ));
    object.put(DataElement::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        sop_instance_uid,
    ));
    object.put(DataElement::new(
        tags::SERIES_INSTANCE_UID,
        VR::UI,
        "1.2.826.0.1.3680043.10.999",
    ));
    object.put(DataElement::new(
        tags::IMAGE_TYPE,
        VR::CS,
        "ORIGINAL\\PRIMARY\\VOLUME\\NONE",
    ));
    object.put(DataElement::new(
        tags::ROWS,
        VR::US,
        PrimitiveValue::from(height as u16),
    ));
    object.put(DataElement::new(
        tags::COLUMNS,
        VR::US,
        PrimitiveValue::from(width as u16),
    ));
    object.put(DataElement::new(
        tags::TOTAL_PIXEL_MATRIX_ROWS,
        VR::UL,
        PrimitiveValue::from(height),
    ));
    object.put(DataElement::new(
        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
        VR::UL,
        PrimitiveValue::from(width),
    ));
    object.put(DataElement::new(
        tags::PIXEL_SPACING,
        VR::DS,
        "0.0005\\0.0005",
    ));
    object.put(DataElement::new(
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        PrimitiveValue::from(1u32),
    ));
    object.put(DataElement::new(
        tags::SAMPLES_PER_PIXEL,
        VR::US,
        PrimitiveValue::from(3u16),
    ));
    object.put(DataElement::new(
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        "RGB",
    ));
    object.put(DataElement::new(
        tags::PLANAR_CONFIGURATION,
        VR::US,
        PrimitiveValue::from(0u16),
    ));
    object.put(DataElement::new(
        tags::BITS_ALLOCATED,
        VR::US,
        PrimitiveValue::from(8u16),
    ));
    object.put(DataElement::new(
        tags::BITS_STORED,
        VR::US,
        PrimitiveValue::from(8u16),
    ));
    object.put(DataElement::new(
        tags::HIGH_BIT,
        VR::US,
        PrimitiveValue::from(7u16),
    ));
    object.put(DataElement::new(
        tags::PIXEL_REPRESENTATION,
        VR::US,
        PrimitiveValue::from(0u16),
    ));
    object.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::from(pixels),
    ));
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE)
                .media_storage_sop_instance_uid(sop_instance_uid)
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
        )
        .unwrap()
        .write_to_file(path)
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

pub(super) fn run_dicom_validator_for_test(
    name: &str,
    command: &str,
    args: &[&str],
    paths: &[&std::path::Path],
) {
    let output = std::process::Command::new(command)
        .args(args)
        .args(paths)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let has_error = stdout
        .lines()
        .chain(stderr.lines())
        .any(|line| line.trim_start().starts_with("Error"));

    assert!(
        output.status.success() && !has_error,
        "{name} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        stdout,
        stderr
    );
}
