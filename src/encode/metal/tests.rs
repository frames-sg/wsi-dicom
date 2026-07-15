use super::*;
use crate::test_support::{find_command_for_test, read_binary_ppm_for_test};
use crate::{CodecValidation, EncodeBackendPreference, TransferSyntax};
use j2k::{
    j2k_lossless_decomposition_levels_for_options, J2kBlockCodingMode, J2kLosslessEncodeOptions,
    J2kLosslessSamples, J2kProgressionOrder,
};
use j2k_core::PixelFormat as J2kPixelFormat;
use wsi_rs::output::metal::MetalDeviceTile;
use wsi_rs::PixelFormat as WsiPixelFormat;

fn rgb8_pixels_for_test(width: u32, height: u32, multiplier: u32) -> Vec<u8> {
    (0..width * height * 3)
        .map(|idx| ((idx * multiplier) & 0xFF) as u8)
        .collect()
}

fn metal_rgb8_tile_for_test(pixels: &[u8], width: u32, height: u32) -> MetalDeviceTile {
    let session = j2k_metal::MetalBackendSession::system_default().expect("Metal session");
    let buffer = session.device().new_buffer_with_data(
        pixels.as_ptr().cast(),
        pixels.len() as u64,
        ::metal::MTLResourceOptions::StorageModeShared,
    );
    crate::metal_interop::test_tile_from_completed_buffer(
        buffer,
        0,
        width,
        height,
        width as usize * 3,
        WsiPixelFormat::Rgb8,
    )
}

fn first_metal_frame_for_test(encoded: Vec<Option<EncodedDicomJ2kFrame>>) -> EncodedDicomJ2kFrame {
    encoded
        .into_iter()
        .next()
        .expect("one frame")
        .expect("Metal frame")
}

fn assert_rgb8_j2k_frame_matches_pixels_for_test(
    frame: EncodedDicomJ2kFrame,
    pixels: &[u8],
    width: u32,
) {
    assert!(frame.codestream_is_metal_buffer_backed());
    let codestream = frame.codestream_bytes().expect("codestream bytes");
    assert!(codestream.starts_with(&[0xFF, 0x4F]));
    let mut decoded = vec![0u8; pixels.len()];
    j2k::J2kDecoder::new(codestream.as_ref())
        .expect("parse J2K")
        .decode_into(&mut decoded, width as usize * 3, J2kPixelFormat::Rgb8)
        .expect("decode J2K");
    assert_eq!(decoded, pixels);
}

#[test]
fn auto_j2k_encoder_can_be_demoted_after_cpu_input_probe_wins() {
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLosslessRpcl,
        CodecValidation::Disabled,
    );

    let cpu_peer = encoder.cpu_only_peer();
    assert_eq!(cpu_peer.preference(), EncodeBackendPreference::CpuOnly);

    encoder.force_cpu_only_for_auto();
    assert_eq!(encoder.preference(), EncodeBackendPreference::CpuOnly);

    let mut preferred = DicomJ2kEncoder::new(
        EncodeBackendPreference::PreferDevice,
        TransferSyntax::Htj2kLosslessRpcl,
        CodecValidation::Disabled,
    );
    preferred.force_cpu_only_for_auto();
    assert_eq!(
        preferred.preference(),
        EncodeBackendPreference::PreferDevice
    );
}

#[test]
fn metal_tile_encode_returns_buffer_backed_codestream_for_padded_tiles() {
    let pixels = rgb8_pixels_for_test(8, 8, 29);
    let tile = metal_rgb8_tile_for_test(&pixels, 8, 8);
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Jpeg2000Lossless,
        CodecValidation::RoundTrip,
    );

    let encoded = encoder
        .encode_metal_tiles(&[tile], 8, 8)
        .expect("Metal DICOM tile encode")
        .frames;
    let frame = first_metal_frame_for_test(encoded);

    assert_rgb8_j2k_frame_matches_pixels_for_test(frame, &pixels, 8);
}

#[test]
#[allow(deprecated)]
fn metal_tile_encode_rejects_legacy_raw_buffer_storage() {
    let pixels = rgb8_pixels_for_test(8, 8, 31);
    let mut tile = metal_rgb8_tile_for_test(&pixels, 8, 8);
    let session = j2k_metal::MetalBackendSession::system_default().expect("Metal session");
    tile.storage = wsi_rs::output::metal::MetalDeviceStorage::Buffer {
        buffer: j2k_metal_support::checked_shared_buffer_with_slice(session.device(), &pixels)
            .expect("legacy test upload"),
        byte_offset: 0,
    };
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Jpeg2000Lossless,
        CodecValidation::RoundTrip,
    );

    let error = match encoder.encode_metal_tiles(&[tile], 8, 8) {
        Ok(_) => panic!("legacy raw storage must be rejected before submission"),
        Err(error) => error,
    };

    assert!(matches!(&error, crate::Error::Unsupported { .. }));
    assert!(error.to_string().contains("legacy raw Metal buffer"));
}

#[test]
fn metal_tile_encode_rejects_mutated_resident_metadata() {
    let pixels = rgb8_pixels_for_test(8, 8, 37);
    let mut tile = metal_rgb8_tile_for_test(&pixels, 8, 8);
    tile.width += 1;
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Jpeg2000Lossless,
        CodecValidation::RoundTrip,
    );

    let error = match encoder.encode_metal_tiles(&[tile], 8, 8) {
        Ok(_) => panic!("resident metadata mismatch must be propagated"),
        Err(error) => error,
    };

    assert!(matches!(&error, crate::Error::Unsupported { .. }));
    assert!(error.to_string().contains("metadata"));
}

#[test]
fn submitted_metal_tile_batch_wait_returns_buffer_backed_codestream() {
    let pixels = rgb8_pixels_for_test(8, 8, 29);
    let tile = metal_rgb8_tile_for_test(&pixels, 8, 8);
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Jpeg2000Lossless,
        CodecValidation::RoundTrip,
    );

    let submitted = encoder
        .submit_metal_tiles_owned(vec![tile], 8, 8)
        .expect("submit Metal DICOM tile encode");
    let encoded = submitted
        .wait()
        .expect("wait submitted Metal encode")
        .frames;
    let frame = first_metal_frame_for_test(encoded);

    assert_rgb8_j2k_frame_matches_pixels_for_test(frame, &pixels, 8);
}

#[test]
fn metal_tile_encode_returns_buffer_backed_codestream_for_edge_tiles() {
    let pixels: Vec<u8> = (0..7 * 5 * 3)
        .map(|idx| ((idx * 31) & 0xFF) as u8)
        .collect();
    let session = j2k_metal::MetalBackendSession::system_default().expect("Metal session");
    let buffer = session.device().new_buffer_with_data(
        pixels.as_ptr().cast(),
        pixels.len() as u64,
        ::metal::MTLResourceOptions::StorageModeShared,
    );
    let tile = crate::metal_interop::test_tile_from_completed_buffer(
        buffer,
        0,
        7,
        5,
        7 * 3,
        WsiPixelFormat::Rgb8,
    );
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Jpeg2000Lossless,
        CodecValidation::RoundTrip,
    );

    let encoded = encoder
        .encode_metal_tiles(&[tile], 8, 8)
        .expect("Metal DICOM edge tile encode")
        .frames;
    let frame = encoded
        .into_iter()
        .next()
        .expect("one frame")
        .expect("Metal frame");

    assert!(frame.codestream_is_metal_buffer_backed());
    let codestream = frame.codestream_bytes().expect("codestream bytes");
    assert!(codestream.starts_with(&[0xFF, 0x4F]));
    let mut decoded = vec![0u8; 8 * 8 * 3];
    j2k::J2kDecoder::new(codestream.as_ref())
        .expect("parse J2K")
        .decode_into(&mut decoded, 8 * 3, J2kPixelFormat::Rgb8)
        .expect("decode J2K");
    for y in 0..8usize {
        for x in 0..8usize {
            let dst = (y * 8 + x) * 3;
            if x < 7 && y < 5 {
                let src = (y * 7 + x) * 3;
                assert_eq!(&decoded[dst..dst + 3], &pixels[src..src + 3]);
            } else {
                assert_eq!(&decoded[dst..dst + 3], &[0, 0, 0]);
            }
        }
    }
}

#[test]
fn metal_tile_encode_returns_buffer_backed_codestream_for_htj2k_tiles() {
    let pixels: Vec<u8> = (0..8 * 8).map(|idx| ((idx * 37) & 0xFF) as u8).collect();
    let session = j2k_metal::MetalBackendSession::system_default().expect("Metal session");
    let buffer = session.device().new_buffer_with_data(
        pixels.as_ptr().cast(),
        pixels.len() as u64,
        ::metal::MTLResourceOptions::StorageModeShared,
    );
    let tile = crate::metal_interop::test_tile_from_completed_buffer(
        buffer,
        0,
        8,
        8,
        8,
        WsiPixelFormat::Gray8,
    );
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Htj2kLossless,
        CodecValidation::RoundTrip,
    );

    let encoded = encoder
        .encode_metal_tiles(&[tile], 8, 8)
        .expect("Metal DICOM HTJ2K tile encode")
        .frames;
    let frame = encoded
        .into_iter()
        .next()
        .expect("one frame")
        .expect("Metal frame");

    assert!(frame.codestream_is_metal_buffer_backed());
    let codestream = frame.codestream_bytes().expect("codestream bytes");
    assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
    let cod_marker = codestream
        .windows(2)
        .position(|window| window == [0xFF, 0x52])
        .expect("COD marker");
    assert_eq!(codestream[cod_marker + 12], 0x40);
    let mut decoded = vec![0u8; pixels.len()];
    j2k::J2kDecoder::new(codestream.as_ref())
        .expect("parse HTJ2K")
        .decode_into(&mut decoded, 8, J2kPixelFormat::Gray8)
        .expect("decode HTJ2K");
    assert_eq!(decoded, pixels);
}

#[test]
fn metal_tile_encode_returns_buffer_backed_codestream_for_wsi_sized_htj2k_rpcl_tiles() {
    let pixels: Vec<u8> = (0..256 * 256 * 3)
        .map(|idx| ((idx * 41) & 0xFF) as u8)
        .collect();
    let session = j2k_metal::MetalBackendSession::system_default().expect("Metal session");
    let buffer = session.device().new_buffer_with_data(
        pixels.as_ptr().cast(),
        pixels.len() as u64,
        ::metal::MTLResourceOptions::StorageModeShared,
    );
    let tile = crate::metal_interop::test_tile_from_completed_buffer(
        buffer,
        0,
        256,
        256,
        256 * 3,
        WsiPixelFormat::Rgb8,
    );
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Htj2kLosslessRpcl,
        CodecValidation::RoundTrip,
    )
    .with_j2k_decomposition_levels(Some(1));

    let encoded = encoder
        .encode_metal_tiles(&[tile], 256, 256)
        .expect("Metal DICOM HTJ2K RPCL tile encode")
        .frames;
    let frame = encoded
        .into_iter()
        .next()
        .expect("one frame")
        .expect("Metal frame");

    assert!(frame.codestream_is_metal_buffer_backed());
    let codestream = frame.codestream_bytes().expect("codestream bytes");
    assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
    let cod_marker = codestream
        .windows(2)
        .position(|window| window == [0xFF, 0x52])
        .expect("COD marker");
    assert_eq!(codestream[cod_marker + 5], 0x02);
    assert_eq!(j2k_cod_decomposition_levels(codestream.as_ref()), 1);
    assert_eq!(codestream[cod_marker + 12], 0x40);
    let mut decoded = vec![0u8; pixels.len()];
    j2k::J2kDecoder::new(codestream.as_ref())
        .expect("parse HTJ2K")
        .decode_into(&mut decoded, 256 * 3, J2kPixelFormat::Rgb8)
        .expect("decode HTJ2K");
    assert_eq!(decoded, pixels);
}

#[test]
fn metal_tile_encode_preserves_default_htj2k_rpcl_decomposition_profile() {
    let pixels: Vec<u8> = (0..512 * 512 * 3)
        .map(|idx| ((idx * 47 + idx / 17) & 0xFF) as u8)
        .collect();
    let samples =
        J2kLosslessSamples::new(&pixels, 512, 512, 3, 8, false).expect("valid RGB samples");
    let expected_levels = j2k_lossless_decomposition_levels_for_options(
        samples,
        J2kLosslessEncodeOptions::new(
            J2kLosslessEncodeOptions::default().backend,
            J2kBlockCodingMode::HighThroughput,
            J2kProgressionOrder::Rpcl,
            J2kLosslessEncodeOptions::default().max_decomposition_levels,
            J2kLosslessEncodeOptions::default().reversible_transform,
            J2kLosslessEncodeOptions::default().validation,
        ),
    );
    assert_eq!(expected_levels, 3);

    let session = j2k_metal::MetalBackendSession::system_default().expect("Metal session");
    let buffer = session.device().new_buffer_with_data(
        pixels.as_ptr().cast(),
        pixels.len() as u64,
        ::metal::MTLResourceOptions::StorageModeShared,
    );
    let tile = crate::metal_interop::test_tile_from_completed_buffer(
        buffer,
        0,
        512,
        512,
        512 * 3,
        WsiPixelFormat::Rgb8,
    );
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Htj2kLosslessRpcl,
        CodecValidation::RoundTrip,
    );

    let encoded = encoder
        .encode_metal_tiles(&[tile], 512, 512)
        .expect("Metal DICOM HTJ2K RPCL tile encode")
        .frames;
    let frame = encoded
        .into_iter()
        .next()
        .expect("one frame")
        .expect("Metal frame");

    assert!(frame.used_device_encode);
    let codestream = frame.codestream_bytes().expect("codestream bytes");
    assert_eq!(
        j2k_cod_decomposition_levels(codestream.as_ref()),
        expected_levels
    );
    let mut decoded = vec![0u8; pixels.len()];
    j2k::J2kDecoder::new(codestream.as_ref())
        .expect("parse HTJ2K")
        .decode_into(&mut decoded, 512 * 3, J2kPixelFormat::Rgb8)
        .expect("decode HTJ2K");
    assert_eq!(decoded, pixels);
}

#[test]
fn metal_host_fallback_parallel_chunk_size_bounds_serial_work() {
    assert_eq!(metal_host_fallback_parallel_chunk_size(0, None, 8), 1);
    assert_eq!(metal_host_fallback_parallel_chunk_size(64, None, 8), 8);
    assert_eq!(metal_host_fallback_parallel_chunk_size(64, Some(16), 8), 4);
    assert_eq!(metal_host_fallback_parallel_chunk_size(10, Some(64), 8), 1);
    assert_eq!(metal_host_fallback_parallel_chunk_size(64, Some(1), 8), 64);
}

#[test]
fn metal_edge_rgb8_htj2k_rpcl_codestream_decodes_with_reference_codec_when_available() {
    let Some(grk_decompress) = find_command_for_test("grk_decompress") else {
        eprintln!("skipping resident Metal HTJ2K edge parity smoke: grk_decompress not found");
        return;
    };
    let pixels: Vec<u8> = (0..7 * 5 * 3)
        .map(|idx| ((idx * 43 + 17) & 0xFF) as u8)
        .collect();
    let session = j2k_metal::MetalBackendSession::system_default().expect("Metal session");
    let buffer = session.device().new_buffer_with_data(
        pixels.as_ptr().cast(),
        pixels.len() as u64,
        ::metal::MTLResourceOptions::StorageModeShared,
    );
    let tile = crate::metal_interop::test_tile_from_completed_buffer(
        buffer,
        0,
        7,
        5,
        7 * 3,
        WsiPixelFormat::Rgb8,
    );
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::RequireDevice,
        TransferSyntax::Htj2kLosslessRpcl,
        CodecValidation::RoundTrip,
    );

    let encoded = encoder
        .encode_metal_tiles(&[tile], 8, 8)
        .expect("resident Metal DICOM HTJ2K RPCL edge tile encode")
        .frames;
    let frame = encoded
        .into_iter()
        .next()
        .expect("one frame")
        .expect("Metal frame");

    assert!(frame.codestream_is_metal_buffer_backed());
    let codestream = frame.codestream_bytes().expect("codestream bytes");
    assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
    let cod_marker = codestream
        .windows(2)
        .position(|window| window == [0xFF, 0x52])
        .expect("COD marker");
    assert_eq!(codestream[cod_marker + 5], 0x02);
    assert_eq!(codestream[cod_marker + 12], 0x40);

    let tmp = tempfile::tempdir().expect("tempdir");
    let codestream_path = tmp.path().join("edge-rgb8.j2k");
    let ppm_path = tmp.path().join("edge-rgb8.ppm");
    std::fs::write(&codestream_path, codestream).expect("write codestream");
    let status = std::process::Command::new(grk_decompress)
        .args(["-i"])
        .arg(&codestream_path)
        .args(["-o"])
        .arg(&ppm_path)
        .status()
        .expect("run grk_decompress");
    assert!(status.success(), "grk_decompress failed with {status}");

    let (width, height, decoded) = read_binary_ppm_for_test(&ppm_path);
    assert_eq!((width, height), (8, 8));
    for y in 0..8usize {
        for x in 0..8usize {
            let dst = (y * 8 + x) * 3;
            if x < 7 && y < 5 {
                let src = (y * 7 + x) * 3;
                assert_eq!(&decoded[dst..dst + 3], &pixels[src..src + 3]);
            } else {
                assert_eq!(&decoded[dst..dst + 3], &[0, 0, 0]);
            }
        }
    }
}

#[test]
fn prefer_device_metal_tile_encode_returns_buffer_backed_codestream_for_wsi_sized_htj2k_rpcl_tiles()
{
    let pixels: Vec<u8> = (0..256 * 256 * 3)
        .map(|idx| ((idx * 41) & 0xFF) as u8)
        .collect();
    let session = j2k_metal::MetalBackendSession::system_default().expect("Metal session");
    let buffer = session.device().new_buffer_with_data(
        pixels.as_ptr().cast(),
        pixels.len() as u64,
        ::metal::MTLResourceOptions::StorageModeShared,
    );
    let tile = crate::metal_interop::test_tile_from_completed_buffer(
        buffer,
        0,
        256,
        256,
        256 * 3,
        WsiPixelFormat::Rgb8,
    );
    let mut encoder = DicomJ2kEncoder::new(
        EncodeBackendPreference::PreferDevice,
        TransferSyntax::Htj2kLosslessRpcl,
        CodecValidation::RoundTrip,
    )
    .with_j2k_decomposition_levels(Some(1));

    let encoded = encoder
        .encode_metal_tiles(&[tile], 256, 256)
        .expect("PreferDevice Metal DICOM HTJ2K RPCL tile encode")
        .frames;

    assert_eq!(encoded.len(), 1);
    assert!(encoded[0]
        .as_ref()
        .expect("Metal frame")
        .codestream_is_metal_buffer_backed());
}

fn j2k_cod_decomposition_levels(codestream: &[u8]) -> u8 {
    let cod_marker = codestream
        .windows(2)
        .position(|window| window == [0xFF, 0x52])
        .expect("COD marker");
    codestream[cod_marker + 9]
}
