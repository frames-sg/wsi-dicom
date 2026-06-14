use statumen::{ColorSpace, CpuTile, CpuTileData, CpuTileLayout, SampleType};

use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelProfile {
    pub(crate) components: u8,
    pub(crate) bits_allocated: u16,
    pub(crate) photometric_interpretation: &'static str,
}

#[derive(Debug)]
pub(crate) struct PreparedTile {
    pub(crate) bytes: Vec<u8>,
    pub(crate) profile: PixelProfile,
}

#[cfg(any(test, feature = "bench-internals"))]
pub(crate) fn prepare_tile_samples(
    tile: &CpuTile,
    output_width: u32,
    output_height: u32,
) -> Result<PreparedTile, Error> {
    prepare_tile_samples_with_limit(tile, output_width, output_height, usize::MAX)
}

pub(crate) fn prepare_tile_samples_with_limit(
    tile: &CpuTile,
    output_width: u32,
    output_height: u32,
    max_prepared_bytes: usize,
) -> Result<PreparedTile, Error> {
    if tile.layout != CpuTileLayout::Interleaved {
        return Err(Error::UnsupportedPixelData {
            reason: "only interleaved CPU tiles are supported".into(),
        });
    }
    let profile = pixel_profile(tile)?;
    if tile.width > output_width || tile.height > output_height {
        return Err(Error::UnsupportedPixelData {
            reason: format!(
                "source tile {}x{} exceeds requested output tile {}x{}",
                tile.width, tile.height, output_width, output_height
            ),
        });
    }
    if let Some(bytes) = exact_size_u8_tile_bytes(tile, output_width, output_height, profile) {
        if bytes.len() > max_prepared_bytes {
            return Err(Error::UnsupportedPixelData {
                reason: format!(
                    "prepared tile buffer requires {} bytes, exceeding configured limit {max_prepared_bytes}",
                    bytes.len()
                ),
            });
        }
        return Ok(PreparedTile {
            bytes: bytes.to_vec(),
            profile,
        });
    }
    let sample_size = usize::from(profile.bits_allocated / 8);
    let out_len = prepared_tile_len(output_width, output_height, profile.components, sample_size)?;
    if out_len > max_prepared_bytes {
        return Err(Error::UnsupportedPixelData {
            reason: format!(
                "prepared tile buffer requires {out_len} bytes, exceeding configured limit {max_prepared_bytes}"
            ),
        });
    }
    let mut out = vec![0u8; out_len];
    match &tile.data {
        CpuTileData::U8(bytes) => copy_u8_tile(tile, bytes, profile, output_width, &mut out)?,
        CpuTileData::U16(samples) => {
            copy_u16_tile(tile, samples, profile, output_width, &mut out)?;
        }
        CpuTileData::F32(_) => {
            return Err(Error::UnsupportedPixelData {
                reason: "Float32 requires an explicit windowing/conversion policy".into(),
            });
        }
    }
    Ok(PreparedTile {
        bytes: out,
        profile,
    })
}

fn prepared_tile_len(
    width: u32,
    height: u32,
    components: u8,
    sample_size: usize,
) -> Result<usize, Error> {
    let width = usize::try_from(width).map_err(|_| Error::UnsupportedPixelData {
        reason: "tile width exceeds platform addressable memory".into(),
    })?;
    let height = usize::try_from(height).map_err(|_| Error::UnsupportedPixelData {
        reason: "tile height exceeds platform addressable memory".into(),
    })?;
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(usize::from(components)))
        .and_then(|samples| samples.checked_mul(sample_size))
        .ok_or_else(|| Error::UnsupportedPixelData {
            reason: "prepared tile buffer length overflow".into(),
        })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn pixel_profile_from_device_format(
    format: signinum_j2k::PixelFormat,
) -> Result<PixelProfile, Error> {
    match format {
        signinum_j2k::PixelFormat::Gray8 => Ok(PixelProfile {
            components: 1,
            bits_allocated: 8,
            photometric_interpretation: "MONOCHROME2",
        }),
        signinum_j2k::PixelFormat::Rgb8 => Ok(PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "RGB",
        }),
        signinum_j2k::PixelFormat::Gray16 => Ok(PixelProfile {
            components: 1,
            bits_allocated: 16,
            photometric_interpretation: "MONOCHROME2",
        }),
        signinum_j2k::PixelFormat::Rgb16 => Ok(PixelProfile {
            components: 3,
            bits_allocated: 16,
            photometric_interpretation: "RGB",
        }),
        signinum_j2k::PixelFormat::Rgba8 | signinum_j2k::PixelFormat::Rgba16 => {
            Err(Error::UnsupportedPixelData {
                reason: "Metal RGBA tiles require an explicit alpha composite policy".into(),
            })
        }
        _ => Err(Error::UnsupportedPixelData {
            reason: "unsupported Metal tile pixel format".into(),
        }),
    }
}

fn pixel_profile(tile: &CpuTile) -> Result<PixelProfile, Error> {
    let bits_allocated = match tile.data.sample_type() {
        SampleType::Uint8 => 8,
        SampleType::Uint16 => 16,
        SampleType::Float32 => {
            return Err(Error::UnsupportedPixelData {
                reason: "Float32 requires an explicit windowing/conversion policy".into(),
            });
        }
    };
    match (&tile.color_space, tile.channels) {
        (ColorSpace::Grayscale, 1) | (_, 1) => Ok(PixelProfile {
            components: 1,
            bits_allocated,
            photometric_interpretation: "MONOCHROME2",
        }),
        (ColorSpace::Rgb, 3) | (_, 3) => Ok(PixelProfile {
            components: 3,
            bits_allocated,
            photometric_interpretation: "RGB",
        }),
        (ColorSpace::Rgba, 4) => Ok(PixelProfile {
            components: 3,
            bits_allocated,
            photometric_interpretation: "RGB",
        }),
        _ => Err(Error::UnsupportedPixelData {
            reason: format!(
                "unsupported color space {:?} with {} channels",
                tile.color_space, tile.channels
            ),
        }),
    }
}

fn exact_size_u8_tile_bytes(
    tile: &CpuTile,
    output_width: u32,
    output_height: u32,
    profile: PixelProfile,
) -> Option<&[u8]> {
    if tile.width != output_width
        || tile.height != output_height
        || profile.bits_allocated != 8
        || tile.channels != u16::from(profile.components)
        || !matches!(profile.components, 1 | 3)
    {
        return None;
    }
    let CpuTileData::U8(bytes) = &tile.data else {
        return None;
    };
    let expected_len = usize::try_from(tile.width)
        .ok()
        .and_then(|width| {
            usize::try_from(tile.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(usize::from(tile.channels)))?;
    (bytes.len() == expected_len).then_some(bytes.as_slice())
}

fn copy_u8_tile(
    tile: &CpuTile,
    bytes: &[u8],
    profile: PixelProfile,
    output_width: u32,
    out: &mut [u8],
) -> Result<(), Error> {
    let src_components = usize::from(tile.channels);
    let dst_components = usize::from(profile.components);
    let tile_width = usize::try_from(tile.width).map_err(|_| Error::UnsupportedPixelData {
        reason: "tile width exceeds platform addressable memory".into(),
    })?;
    let tile_height = usize::try_from(tile.height).map_err(|_| Error::UnsupportedPixelData {
        reason: "tile height exceeds platform addressable memory".into(),
    })?;
    let output_width = usize::try_from(output_width).map_err(|_| Error::UnsupportedPixelData {
        reason: "output tile width exceeds platform addressable memory".into(),
    })?;
    let expected_src = tile_width
        .checked_mul(tile_height)
        .and_then(|pixels| pixels.checked_mul(src_components))
        .ok_or_else(|| Error::UnsupportedPixelData {
            reason: "source tile buffer length overflow".into(),
        })?;
    if bytes.len() < expected_src {
        return Err(Error::UnsupportedPixelData {
            reason: format!(
                "source tile buffer is shorter than expected: {} < {expected_src}",
                bytes.len()
            ),
        });
    }
    for y in 0..tile_height {
        for x in 0..tile_width {
            let src = y
                .checked_mul(tile_width)
                .and_then(|row| row.checked_add(x))
                .and_then(|pixel| pixel.checked_mul(src_components))
                .ok_or_else(|| Error::UnsupportedPixelData {
                    reason: "source tile index overflow".into(),
                })?;
            let dst = y
                .checked_mul(output_width)
                .and_then(|row| row.checked_add(x))
                .and_then(|pixel| pixel.checked_mul(dst_components))
                .ok_or_else(|| Error::UnsupportedPixelData {
                    reason: "prepared tile index overflow".into(),
                })?;
            if src_components == 4 {
                if bytes[src + 3] != u8::MAX {
                    return Err(Error::UnsupportedPixelData {
                        reason: "non-opaque alpha requires an explicit composite policy".into(),
                    });
                }
                out[dst..dst + 3].copy_from_slice(&bytes[src..src + 3]);
            } else {
                out[dst..dst + dst_components].copy_from_slice(&bytes[src..src + dst_components]);
            }
        }
    }
    Ok(())
}

fn copy_u16_tile(
    tile: &CpuTile,
    samples: &[u16],
    profile: PixelProfile,
    output_width: u32,
    out: &mut [u8],
) -> Result<(), Error> {
    let src_components = usize::from(tile.channels);
    let dst_components = usize::from(profile.components);
    let tile_width = usize::try_from(tile.width).map_err(|_| Error::UnsupportedPixelData {
        reason: "tile width exceeds platform addressable memory".into(),
    })?;
    let tile_height = usize::try_from(tile.height).map_err(|_| Error::UnsupportedPixelData {
        reason: "tile height exceeds platform addressable memory".into(),
    })?;
    let output_width = usize::try_from(output_width).map_err(|_| Error::UnsupportedPixelData {
        reason: "output tile width exceeds platform addressable memory".into(),
    })?;
    let expected_src = tile_width
        .checked_mul(tile_height)
        .and_then(|pixels| pixels.checked_mul(src_components))
        .ok_or_else(|| Error::UnsupportedPixelData {
            reason: "source tile buffer length overflow".into(),
        })?;
    if samples.len() < expected_src {
        return Err(Error::UnsupportedPixelData {
            reason: format!(
                "source tile sample buffer is shorter than expected: {} < {expected_src}",
                samples.len()
            ),
        });
    }
    for y in 0..tile_height {
        for x in 0..tile_width {
            let src = y
                .checked_mul(tile_width)
                .and_then(|row| row.checked_add(x))
                .and_then(|pixel| pixel.checked_mul(src_components))
                .ok_or_else(|| Error::UnsupportedPixelData {
                    reason: "source tile sample index overflow".into(),
                })?;
            let dst = y
                .checked_mul(output_width)
                .and_then(|row| row.checked_add(x))
                .and_then(|pixel| pixel.checked_mul(dst_components))
                .and_then(|offset| offset.checked_mul(2))
                .ok_or_else(|| Error::UnsupportedPixelData {
                    reason: "prepared tile byte index overflow".into(),
                })?;
            if src_components == 4 && samples[src + 3] != u16::MAX {
                return Err(Error::UnsupportedPixelData {
                    reason: "non-opaque alpha requires an explicit composite policy".into(),
                });
            }
            for c in 0..dst_components {
                out[dst + c * 2..dst + c * 2 + 2].copy_from_slice(&samples[src + c].to_le_bytes());
            }
        }
    }
    Ok(())
}

pub(crate) fn optical_path_groups(channels: u32) -> Vec<u32> {
    if channels == 0 {
        Vec::new()
    } else if channels == 1 {
        vec![0]
    } else {
        (0..channels).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu_tile(
        width: u32,
        height: u32,
        channels: u16,
        color_space: ColorSpace,
        layout: CpuTileLayout,
        data: CpuTileData,
    ) -> CpuTile {
        CpuTile::new(width, height, channels, color_space, layout, data).unwrap()
    }

    #[test]
    fn prepare_tile_samples_copies_u8_rgb_and_pads_requested_output() {
        let tile = cpu_tile(
            2,
            1,
            3,
            ColorSpace::Rgb,
            CpuTileLayout::Interleaved,
            CpuTileData::u8(vec![1, 2, 3, 4, 5, 6]),
        );

        let prepared = prepare_tile_samples(&tile, 3, 2).unwrap();

        assert_eq!(prepared.profile.components, 3);
        assert_eq!(prepared.profile.bits_allocated, 8);
        assert_eq!(prepared.profile.photometric_interpretation, "RGB");
        assert_eq!(prepared.bytes.len(), 18);
        assert_eq!(&prepared.bytes[..6], &[1, 2, 3, 4, 5, 6]);
        assert!(prepared.bytes[6..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn exact_size_u8_tile_bytes_fast_path_accepts_only_direct_copy_layouts() {
        let rgb = cpu_tile(
            2,
            1,
            3,
            ColorSpace::Rgb,
            CpuTileLayout::Interleaved,
            CpuTileData::u8(vec![1, 2, 3, 4, 5, 6]),
        );
        let gray = cpu_tile(
            2,
            1,
            1,
            ColorSpace::Grayscale,
            CpuTileLayout::Interleaved,
            CpuTileData::u8(vec![7, 8]),
        );
        let rgba = cpu_tile(
            1,
            1,
            4,
            ColorSpace::Rgba,
            CpuTileLayout::Interleaved,
            CpuTileData::u8(vec![1, 2, 3, u8::MAX]),
        );

        assert_eq!(
            exact_size_u8_tile_bytes(
                &rgb,
                2,
                1,
                PixelProfile {
                    components: 3,
                    bits_allocated: 8,
                    photometric_interpretation: "RGB",
                }
            ),
            Some(&[1, 2, 3, 4, 5, 6][..])
        );
        assert_eq!(
            exact_size_u8_tile_bytes(
                &gray,
                2,
                1,
                PixelProfile {
                    components: 1,
                    bits_allocated: 8,
                    photometric_interpretation: "MONOCHROME2",
                }
            ),
            Some(&[7, 8][..])
        );
        assert_eq!(
            exact_size_u8_tile_bytes(
                &rgba,
                1,
                1,
                PixelProfile {
                    components: 3,
                    bits_allocated: 8,
                    photometric_interpretation: "RGB",
                }
            ),
            None
        );
    }

    #[test]
    fn prepare_tile_samples_copies_u16_rgba_when_alpha_is_opaque() {
        let tile = cpu_tile(
            1,
            1,
            4,
            ColorSpace::Rgba,
            CpuTileLayout::Interleaved,
            CpuTileData::u16(vec![0x0102, 0x0304, 0x0506, u16::MAX]),
        );

        let prepared = prepare_tile_samples(&tile, 1, 1).unwrap();

        assert_eq!(prepared.profile.components, 3);
        assert_eq!(prepared.profile.bits_allocated, 16);
        assert_eq!(prepared.bytes, vec![0x02, 0x01, 0x04, 0x03, 0x06, 0x05]);
    }

    #[test]
    fn prepare_tile_samples_rejects_unsupported_tile_layouts_and_alpha() {
        let planar = cpu_tile(
            1,
            1,
            1,
            ColorSpace::Grayscale,
            CpuTileLayout::Planar,
            CpuTileData::u8(vec![7]),
        );
        let err = match prepare_tile_samples(&planar, 1, 1) {
            Ok(_) => panic!("planar tile should be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("interleaved"));

        let translucent = cpu_tile(
            1,
            1,
            4,
            ColorSpace::Rgba,
            CpuTileLayout::Interleaved,
            CpuTileData::u8(vec![1, 2, 3, 127]),
        );
        let err = match prepare_tile_samples(&translucent, 1, 1) {
            Ok(_) => panic!("translucent tile should be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("alpha"));

        let float_tile = cpu_tile(
            1,
            1,
            1,
            ColorSpace::Grayscale,
            CpuTileLayout::Interleaved,
            CpuTileData::f32(vec![0.5]),
        );
        let err = match prepare_tile_samples(&float_tile, 1, 1) {
            Ok(_) => panic!("float tile should be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("Float32"));
    }

    #[test]
    fn prepare_tile_samples_rejects_configured_output_size_limit() {
        let tile = cpu_tile(
            2,
            2,
            3,
            ColorSpace::Rgb,
            CpuTileLayout::Interleaved,
            CpuTileData::u8(vec![0; 12]),
        );

        let err = prepare_tile_samples_with_limit(&tile, 2, 2, 11)
            .expect_err("prepared tile should exceed configured byte limit");

        assert!(err.to_string().contains("exceeding configured limit"));
    }

    #[test]
    fn optical_path_groups_match_channel_count() {
        assert_eq!(optical_path_groups(0), Vec::<u32>::new());
        assert_eq!(optical_path_groups(1), vec![0]);
        assert_eq!(optical_path_groups(4), vec![0, 1, 2, 3]);
    }
}
