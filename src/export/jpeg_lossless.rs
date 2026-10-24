//! Prepared-frame encoding for JPEG Lossless SV1.

use std::time::{Duration, Instant};

use j2k_jpeg::{JpegBackend, JpegLosslessEncodeOptions, JpegLosslessSamples};

use crate::error::Error;
use crate::options::CodecValidation;
use crate::tile::PixelProfile;

/// Encode one prepared frame as SV1 JPEG Lossless at the profile's bit depth,
/// returning the bytes with encode and validation durations.
pub(super) fn encode_jpeg_lossless_frame(
    bytes: &[u8],
    profile: PixelProfile,
    width: u32,
    height: u32,
    codec_validation: CodecValidation,
) -> Result<(Vec<u8>, Duration, Duration), Error> {
    let wide;
    let samples = match (
        profile.components,
        profile.bits_allocated,
        profile.photometric_interpretation,
    ) {
        (1, 8, "MONOCHROME2") => JpegLosslessSamples::Gray8 {
            data: bytes,
            width,
            height,
        },
        (3, 8, "RGB") => JpegLosslessSamples::Rgb8 {
            data: bytes,
            width,
            height,
        },
        (1, 16, "MONOCHROME2") => {
            wide = little_endian_u16_samples(bytes)?;
            JpegLosslessSamples::Gray16 {
                data: &wide,
                width,
                height,
            }
        }
        (3, 16, "RGB") => {
            wide = little_endian_u16_samples(bytes)?;
            JpegLosslessSamples::Rgb16 {
                data: &wide,
                width,
                height,
            }
        }
        (components, bits, photometric) => {
            return Err(Error::UnsupportedPixelData {
                reason: format!(
                    "JPEG Lossless supports 8- or 16-bit MONOCHROME2 or RGB, got {components} component(s) at {bits} bits as {photometric}"
                ),
            });
        }
    };
    // The writer declares Bits Stored equal to Bits Allocated.
    let mut options = JpegLosslessEncodeOptions::new(profile.bits_allocated as u8);
    options.backend = JpegBackend::Cpu;
    let encode_started = Instant::now();
    let encoded =
        j2k_jpeg::encode_jpeg_lossless(samples, options).map_err(|source| Error::Encode {
            message: format!("JPEG Lossless encode failed: {source}"),
        })?;
    let encode_duration = encode_started.elapsed();
    let validation_started = Instant::now();
    if codec_validation == CodecValidation::RoundTrip {
        validate_jpeg_lossless_round_trip(&encoded.data, samples)?;
    }
    Ok((encoded.data, encode_duration, validation_started.elapsed()))
}

fn little_endian_u16_samples(bytes: &[u8]) -> Result<Vec<u16>, Error> {
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(Error::UnsupportedPixelData {
            reason: "16-bit frame byte length is odd".into(),
        });
    }
    Ok(pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect())
}

fn validate_jpeg_lossless_round_trip(
    encoded: &[u8],
    samples: JpegLosslessSamples<'_>,
) -> Result<(), Error> {
    let validation_error = |reason: String| Error::Encode {
        message: format!("JPEG Lossless round-trip validation failed: {reason}"),
    };
    let (decoded, _) = j2k_jpeg::Decoder::new(encoded)
        .map_err(|err| validation_error(err.to_string()))?
        .decode_request(j2k_jpeg::DecodeRequest::full(samples.pixel_format()))
        .map_err(|err| validation_error(err.to_string()))?;
    let decoded = decoded.as_slice();
    let identical = match samples {
        JpegLosslessSamples::Gray8 { data, .. } | JpegLosslessSamples::Rgb8 { data, .. } => {
            decoded == data
        }
        JpegLosslessSamples::Gray16 { data, .. } | JpegLosslessSamples::Rgb16 { data, .. } => {
            let (pairs, remainder) = decoded.as_chunks::<2>();
            remainder.is_empty()
                && pairs.len() == data.len()
                && pairs
                    .iter()
                    .zip(data)
                    .all(|(pair, &sample)| u16::from_ne_bytes(*pair) == sample)
        }
    };
    if !identical {
        return Err(validation_error(
            "decoded samples differ from the encoded frame".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::frame_region::PreparedCpuRegion;
    use super::*;

    fn prepared(bytes: Vec<u8>, components: u8, bits_allocated: u16) -> PreparedCpuRegion {
        PreparedCpuRegion {
            bytes: Arc::new(bytes),
            profile: PixelProfile {
                components,
                bits_allocated,
                photometric_interpretation: if components == 1 {
                    "MONOCHROME2"
                } else {
                    "RGB"
                },
            },
            input_decode_duration: Duration::ZERO,
            compose_duration: Duration::ZERO,
        }
    }

    #[test]
    fn frames_round_trip_exactly_at_8_and_16_bits() {
        let size = 8u32;
        let pixels = (size * size) as usize;
        let rgb8 = (0..pixels * 3).map(|i| (i * 37 % 256) as u8).collect();
        let gray16: Vec<u8> = (0..pixels)
            .flat_map(|i| ((i as u16).wrapping_mul(2_731)).to_le_bytes())
            .collect();
        let rgb16: Vec<u8> = (0..pixels * 3)
            .flat_map(|i| ((i as u16).wrapping_mul(40_503)).to_le_bytes())
            .collect();
        for tile in [
            prepared(rgb8, 3, 8),
            prepared(vec![7; pixels], 1, 8),
            prepared(gray16, 1, 16),
            prepared(rgb16, 3, 16),
        ] {
            let (data, _, _) = encode_jpeg_lossless_frame(
                &tile.bytes,
                tile.profile,
                size,
                size,
                CodecValidation::RoundTrip,
            )
            .unwrap();
            assert_eq!(&data[..2], [0xFF, 0xD8]);
            assert!(
                data.windows(2).any(|marker| marker == [0xFF, 0xC3]),
                "frame must be a lossless SOF3 JPEG"
            );
        }
    }

    #[test]
    fn unsupported_profiles_are_rejected_before_encode() {
        let mut tile = prepared(vec![0; 8 * 8 * 3], 3, 8);
        tile.profile.photometric_interpretation = "YBR_FULL_422";
        let error =
            encode_jpeg_lossless_frame(&tile.bytes, tile.profile, 8, 8, CodecValidation::Disabled)
                .unwrap_err();
        assert!(
            matches!(error, Error::UnsupportedPixelData { .. }),
            "{error}"
        );
    }

    #[test]
    fn odd_length_16_bit_frames_are_rejected() {
        assert!(little_endian_u16_samples(&[1, 2, 3]).is_err());
        assert_eq!(little_endian_u16_samples(&[1, 2]).unwrap(), [0x0201]);
    }

    #[test]
    fn round_trip_validation_reports_mismatched_samples() {
        let data = vec![9u8; 64];
        let encoded = j2k_jpeg::encode_jpeg_lossless(
            JpegLosslessSamples::Gray8 {
                data: &data,
                width: 8,
                height: 8,
            },
            JpegLosslessEncodeOptions::new(8),
        )
        .unwrap();
        let other = vec![10u8; 64];
        let error = validate_jpeg_lossless_round_trip(
            &encoded.data,
            JpegLosslessSamples::Gray8 {
                data: &other,
                width: 8,
                height: 8,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("round-trip"), "{error}");
    }
}
