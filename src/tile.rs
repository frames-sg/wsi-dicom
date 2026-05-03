use statumen::{ColorSpace, CpuTile, CpuTileData, CpuTileLayout, SampleType};

use crate::WsiDicomError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelProfile {
    pub(crate) components: u8,
    pub(crate) bits_allocated: u16,
    pub(crate) photometric_interpretation: &'static str,
}

pub(crate) struct PreparedTile {
    pub(crate) bytes: Vec<u8>,
    pub(crate) profile: PixelProfile,
}

pub(crate) fn prepare_tile_samples(
    tile: &CpuTile,
    output_width: u32,
    output_height: u32,
) -> Result<PreparedTile, WsiDicomError> {
    if tile.layout != CpuTileLayout::Interleaved {
        return Err(WsiDicomError::UnsupportedPixelData {
            reason: "only interleaved CPU tiles are supported".into(),
        });
    }
    let profile = pixel_profile(tile)?;
    let sample_size = usize::from(profile.bits_allocated / 8);
    let out_len =
        output_width as usize * output_height as usize * profile.components as usize * sample_size;
    let mut out = vec![0u8; out_len];
    match &tile.data {
        CpuTileData::U8(bytes) => copy_u8_tile(tile, bytes, profile, output_width, &mut out)?,
        CpuTileData::U16(samples) => {
            copy_u16_tile(tile, samples, profile, output_width, &mut out)?;
        }
        CpuTileData::F32(_) => {
            return Err(WsiDicomError::UnsupportedPixelData {
                reason: "Float32 requires an explicit windowing/conversion policy".into(),
            });
        }
    }
    Ok(PreparedTile {
        bytes: out,
        profile,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn pixel_profile_from_device_format(
    format: signinum_j2k::PixelFormat,
) -> Result<PixelProfile, WsiDicomError> {
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
            Err(WsiDicomError::UnsupportedPixelData {
                reason: "Metal RGBA tiles require an explicit alpha composite policy".into(),
            })
        }
        _ => Err(WsiDicomError::UnsupportedPixelData {
            reason: "unsupported Metal tile pixel format".into(),
        }),
    }
}

fn pixel_profile(tile: &CpuTile) -> Result<PixelProfile, WsiDicomError> {
    let bits_allocated = match tile.data.sample_type() {
        SampleType::Uint8 => 8,
        SampleType::Uint16 => 16,
        SampleType::Float32 => {
            return Err(WsiDicomError::UnsupportedPixelData {
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
        _ => Err(WsiDicomError::UnsupportedPixelData {
            reason: format!(
                "unsupported color space {:?} with {} channels",
                tile.color_space, tile.channels
            ),
        }),
    }
}

fn copy_u8_tile(
    tile: &CpuTile,
    bytes: &[u8],
    profile: PixelProfile,
    output_width: u32,
    out: &mut [u8],
) -> Result<(), WsiDicomError> {
    let src_components = tile.channels as usize;
    let dst_components = profile.components as usize;
    for y in 0..tile.height as usize {
        for x in 0..tile.width as usize {
            let src = (y * tile.width as usize + x) * src_components;
            let dst = (y * output_width as usize + x) * dst_components;
            if src_components == 4 {
                if bytes[src + 3] != u8::MAX {
                    return Err(WsiDicomError::UnsupportedPixelData {
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
) -> Result<(), WsiDicomError> {
    let src_components = tile.channels as usize;
    let dst_components = profile.components as usize;
    for y in 0..tile.height as usize {
        for x in 0..tile.width as usize {
            let src = (y * tile.width as usize + x) * src_components;
            let dst = (y * output_width as usize + x) * dst_components * 2;
            if src_components == 4 && samples[src + 3] != u16::MAX {
                return Err(WsiDicomError::UnsupportedPixelData {
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
