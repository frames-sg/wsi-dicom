use super::*;
use j2k_core::DeviceSubmission as _;

#[cfg(all(feature = "metal", target_os = "macos"))]
struct MetalPackTileDispatch<'a> {
    image: &'a j2k_metal_support::ResidentMetalImage,
    source_offset: u64,
    source_pitch: u64,
    destination_offset: u64,
    destination_pitch: u64,
    row_bytes: u64,
    height: u64,
}

impl MetalStripComposer {
    pub(in crate::export) fn pack_tiles(
        &self,
        tiles: &[wsi_rs::output::metal::MetalDeviceTile],
        layout: WholeLevelStripLayout,
        first_col: i64,
        first_row: i64,
        tiles_across: usize,
    ) -> Result<PackedMetalStrips, Error> {
        let first = tiles.first().ok_or_else(|| Error::Unsupported {
            reason: "Metal WholeLevel composition requires at least one source tile".into(),
        })?;
        let format = first.format;
        let j2k_format = J2kPixelFormat::from(format);
        let bytes_per_pixel = format.bytes_per_pixel();
        let slot_stride = (layout.width as usize)
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| Error::Unsupported {
                reason: "Metal WholeLevel source slot stride overflow".into(),
            })?;
        let tile_height_usize = usize::try_from(layout.height).map_err(|_| Error::Unsupported {
            reason: "Metal WholeLevel source tile height exceeds platform addressable memory"
                .into(),
        })?;
        let tile_slot_bytes =
            slot_stride
                .checked_mul(tile_height_usize)
                .ok_or_else(|| Error::Unsupported {
                    reason: "Metal WholeLevel source tile slot byte length overflow".into(),
                })?;
        let total_bytes =
            tile_slot_bytes
                .checked_mul(tiles.len())
                .ok_or_else(|| Error::Unsupported {
                    reason: "Metal packed WholeLevel tile byte length overflow".into(),
                })?;
        let tiles_across_u32 = u32::try_from(tiles_across).map_err(|_| Error::Unsupported {
            reason: "Metal WholeLevel source tile columns exceed u32".into(),
        })?;
        if tiles_across == 0 || !tiles.len().is_multiple_of(tiles_across) {
            return Err(Error::Unsupported {
                reason: "Metal WholeLevel source tile grid is not rectangular".into(),
            });
        }
        let tiles_down =
            u32::try_from(tiles.len() / tiles_across).map_err(|_| Error::Unsupported {
                reason: "Metal WholeLevel source tile rows exceed u32".into(),
            })?;
        u64::try_from(total_bytes).map_err(|_| Error::Unsupported {
            reason: "Metal packed WholeLevel byte length exceeds u64".into(),
        })?;
        let destination_pitch = u64::try_from(slot_stride).map_err(|_| Error::Unsupported {
            reason: "Metal packed WholeLevel destination pitch exceeds u64".into(),
        })?;
        let mut validated_tiles = Vec::with_capacity(tiles.len());
        for (idx, tile) in tiles.iter().enumerate() {
            if tile.format != format {
                return Err(Error::Unsupported {
                    reason: "Metal WholeLevel composition requires uniform source tile format"
                        .into(),
                });
            }
            if tile.width == 0
                || tile.height == 0
                || tile.width > layout.width
                || tile.height > layout.height
            {
                return Err(Error::Unsupported {
                    reason: format!(
                        "Metal WholeLevel source tile geometry exceeds virtual tile: got {}x{}, expected <= {}x{}",
                        tile.width, tile.height, layout.width, layout.height
                    ),
                });
            }
            let row_bytes = (tile.width as usize)
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| Error::Unsupported {
                    reason: "Metal WholeLevel source tile row byte length overflow".into(),
                })?;
            if tile.pitch_bytes < row_bytes {
                return Err(Error::Unsupported {
                    reason: "Metal WholeLevel source tile pitch is smaller than row bytes".into(),
                });
            }
            let image = crate::metal_interop::device_tile_image(tile)?;
            image.validate_device(&self.device).map_err(|source| {
                crate::metal_interop::support_error("Metal strip pack input device", source)
            })?;
            let slot_offset =
                idx.checked_mul(tile_slot_bytes)
                    .ok_or_else(|| Error::Unsupported {
                        reason: "Metal packed WholeLevel destination offset overflow".into(),
                    })?;
            let source_end = image
                .byte_offset()
                .checked_add(image.byte_len())
                .ok_or_else(|| Error::Unsupported {
                    reason: "Metal WholeLevel source image byte range overflow".into(),
                })?;
            u64::try_from(source_end).map_err(|_| Error::Unsupported {
                reason: "Metal WholeLevel source image byte range exceeds u64".into(),
            })?;
            let destination_end =
                slot_offset
                    .checked_add(tile_slot_bytes)
                    .ok_or_else(|| Error::Unsupported {
                        reason: "Metal WholeLevel destination byte range overflow".into(),
                    })?;
            if destination_end > total_bytes {
                return Err(Error::Unsupported {
                    reason: "Metal WholeLevel destination byte range exceeds packed output".into(),
                });
            }
            validated_tiles.push(MetalPackTileDispatch {
                image,
                source_offset: u64::try_from(image.byte_offset()).map_err(|_| {
                    Error::Unsupported {
                        reason: "Metal WholeLevel source offset exceeds u64".into(),
                    }
                })?,
                source_pitch: u64::try_from(tile.pitch_bytes).map_err(|_| Error::Unsupported {
                    reason: "Metal WholeLevel source pitch exceeds u64".into(),
                })?,
                destination_offset: u64::try_from(slot_offset).map_err(|_| Error::Unsupported {
                    reason: "Metal WholeLevel destination offset exceeds u64".into(),
                })?,
                destination_pitch,
                row_bytes: u64::try_from(row_bytes).map_err(|_| Error::Unsupported {
                    reason: "Metal WholeLevel source row byte length exceeds u64".into(),
                })?,
                height: u64::from(tile.height),
            });
        }

        let packed =
            j2k_metal_support::checked_shared_buffer_for_len::<u8>(&self.device, total_bytes)
                .map_err(|source| {
                    crate::metal_interop::support_error("Metal packed strip allocation", source)
                })?;
        let command_buffer =
            j2k_metal_support::checked_command_buffer(&self.queue).map_err(|source| {
                crate::metal_interop::support_error("Metal strip pack command", source)
            })?;
        if metal_profile_stages_enabled() {
            command_buffer.set_label("wsi-dicom input tile pack");
        }
        let blit = command_buffer.new_blit_command_encoder();
        if metal_profile_stages_enabled() {
            blit.set_label("WSI input tile pack");
        }

        for tile in &validated_tiles {
            crate::metal_interop::copy_resident_rows(
                blit,
                tile.image,
                tile.source_offset,
                tile.source_pitch,
                &packed,
                tile.destination_offset,
                tile.destination_pitch,
                tile.row_bytes,
                tile.height,
            );
        }

        blit.end_encoding();
        let input_keepalives = validated_tiles
            .into_iter()
            .map(|tile| tile.image.clone())
            .collect();
        let packed_height = layout
            .height
            .checked_mul(u32::try_from(tiles.len()).map_err(|_| Error::Unsupported {
                reason: "Metal packed WholeLevel tile count exceeds u32".into(),
            })?)
            .ok_or_else(|| Error::Unsupported {
                reason: "Metal packed WholeLevel image height overflow".into(),
            })?;
        let packed_layout = j2k_metal_support::MetalImageLayout::new(
            0,
            (layout.width, packed_height),
            slot_stride,
            j2k_format,
        )
        .map_err(|source| {
            crate::metal_interop::support_error("Metal packed strip layout", source)
        })?;
        let submitted = crate::metal_interop::submit_images(
            &self.device,
            command_buffer,
            vec![(packed, packed_layout)],
            input_keepalives,
        )?;
        let mut images = submitted.wait().map_err(|source| {
            crate::metal_interop::support_error("Metal strip pack completion", source)
        })?;
        let image = images.pop().ok_or_else(|| Error::Encode {
            message: "Metal strip pack returned no resident output".into(),
        })?;

        Ok(PackedMetalStrips {
            image,
            first_col,
            first_row,
            tiles_across: tiles_across_u32,
            tiles_down,
            tile_width: layout.width,
            tile_height: layout.height,
            slot_stride,
            tile_slot_bytes,
            format: j2k_format,
        })
    }
}
