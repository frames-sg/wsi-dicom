use j2k_jpeg::{EncodedJpeg, JpegBackend};
use j2k_jpeg_metal::{encode_jpeg_baseline_batch_from_metal_buffers, JpegBaselineMetalEncodeTile};

use crate::tile::{pixel_profile_from_wsi_device_format, PixelProfile};
use crate::Error;

use crate::export::jpeg_baseline_output_profile;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(in crate::export) fn encode_jpeg_baseline_metal_device_tile_batch(
    tiles: &[wsi_rs::output::metal::MetalDeviceTile],
    frame_columns: u32,
    frame_rows: u32,
    jpeg_quality: u8,
    session: &j2k_jpeg_metal::MetalBackendSession,
) -> Result<Vec<(EncodedJpeg, PixelProfile)>, Error> {
    let first = tiles.first().ok_or_else(|| Error::Unsupported {
        reason: "JPEG Baseline Metal tile batch is empty".into(),
    })?;
    let source_profile = pixel_profile_from_wsi_device_format(first.format)?;
    let (profile, subsampling) = jpeg_baseline_output_profile(source_profile)?;
    let mut requests = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if pixel_profile_from_wsi_device_format(tile.format)? != source_profile {
            return Err(Error::UnsupportedPixelData {
                reason: "JPEG Baseline Metal tile batch changed pixel profile".into(),
            });
        }
        let image = crate::metal_interop::device_tile_image(tile)?;
        requests.push(JpegBaselineMetalEncodeTile::from_resident(
            image,
            (frame_columns, frame_rows),
        ));
    }
    let encoded = encode_jpeg_baseline_batch_from_metal_buffers(
        &requests,
        j2k_jpeg::JpegEncodeOptions {
            quality: jpeg_quality,
            subsampling,
            restart_interval: None,
            backend: JpegBackend::Metal,
        },
        session,
    )
    .map_err(|source| Error::Encode {
        message: format!("JPEG Baseline Metal encode failed: {source}"),
    })?;
    Ok(encoded
        .into_iter()
        .map(|encoded| (encoded, profile))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tile(
        device: &metal::DeviceRef,
        pixels: &[u8],
    ) -> wsi_rs::output::metal::MetalDeviceTile {
        crate::metal_interop::test_tile_from_shared_bytes(
            device,
            pixels,
            8,
            8,
            j2k_core::PixelFormat::Rgb8,
        )
    }

    #[test]
    #[allow(deprecated)]
    fn encode_rejects_legacy_raw_buffer_storage() {
        let Some(device) = metal::Device::system_default() else {
            return;
        };
        let pixels = vec![41_u8; 8 * 8 * 3];
        let mut tile = test_tile(&device, &pixels);
        tile.storage = wsi_rs::output::metal::MetalDeviceStorage::Buffer {
            buffer: j2k_metal_support::checked_shared_buffer_with_slice(&device, &pixels)
                .expect("legacy test upload"),
            byte_offset: 0,
        };
        let session = j2k_jpeg_metal::MetalBackendSession::new(device);

        let error = encode_jpeg_baseline_metal_device_tile_batch(&[tile], 8, 8, 85, &session)
            .expect_err("legacy raw storage must be rejected before JPEG submission");

        assert!(matches!(&error, Error::Unsupported { .. }));
        assert!(error.to_string().contains("legacy raw Metal buffer"));
    }

    #[test]
    fn encode_rejects_mutated_resident_metadata() {
        let Some(device) = metal::Device::system_default() else {
            return;
        };
        let pixels = vec![43_u8; 8 * 8 * 3];
        let mut tile = test_tile(&device, &pixels);
        tile.pitch_bytes += 1;
        let session = j2k_jpeg_metal::MetalBackendSession::new(device);

        let error = encode_jpeg_baseline_metal_device_tile_batch(&[tile], 8, 8, 85, &session)
            .expect_err("resident metadata mismatch must be propagated");

        assert!(matches!(&error, Error::Unsupported { .. }));
        assert!(error.to_string().contains("metadata"));
    }
}
