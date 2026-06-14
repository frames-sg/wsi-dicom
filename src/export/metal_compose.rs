use super::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct PackedMetalStrips {
    pub(super) buffer: metal::Buffer,
    pub(super) first_col: i64,
    pub(super) first_row: i64,
    pub(super) tiles_across: u32,
    pub(super) tile_width: u32,
    pub(super) tile_height: u32,
    pub(super) slot_stride: usize,
    pub(super) tile_slot_bytes: usize,
    pub(super) format: SigninumPixelFormat,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
pub(super) struct MetalComposeTileRequest {
    pub(super) src_origin_x: u32,
    pub(super) src_origin_y: u32,
    pub(super) valid_width: u32,
    pub(super) valid_height: u32,
    pub(super) output_width: u32,
    pub(super) output_height: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalComposeTileDispatch {
    pub(super) request: MetalComposeTileRequest,
    pub(super) params: MetalComposeStripsParams,
    pub(super) dst_buffer: metal::Buffer,
    pub(super) dst_stride: usize,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalStripComposer {
    pub(super) device: metal::Device,
    pub(super) queue: metal::CommandQueue,
    pub(super) pipeline: metal::ComputePipelineState,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn metal_profile_stages_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("SIGNINUM_J2K_METAL_PROFILE_STAGES"),
            Ok(value) if value == "1"
        )
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalStripComposer {
    pub(super) fn new(device: metal::Device) -> Result<Self, Error> {
        let options = metal::CompileOptions::new();
        let library = device
            .new_library_with_source(WSI_COMPOSE_STRIPS_METAL, &options)
            .map_err(|message| Error::Encode {
                message: format!("Metal strip compose shader failed to compile: {message}"),
            })?;
        let function = library
            .get_function("wsi_compose_strips", None)
            .map_err(|message| Error::Encode {
                message: format!("Metal strip compose function unavailable: {message}"),
            })?;
        let pipeline = device
            .new_compute_pipeline_state_with_function(&function)
            .map_err(|message| Error::Encode {
                message: format!("Metal strip compose pipeline unavailable: {message}"),
            })?;
        let queue = device.new_command_queue();
        Ok(Self {
            device,
            queue,
            pipeline,
        })
    }

    pub(super) fn pack_tiles(
        &self,
        tiles: &[statumen::output::metal::MetalDeviceTile],
        layout: WholeLevelStripLayout,
        first_col: i64,
        first_row: i64,
        tiles_across: usize,
    ) -> Result<PackedMetalStrips, Error> {
        let first = tiles.first().ok_or_else(|| Error::Unsupported {
            reason: "Metal WholeLevel composition requires at least one source tile".into(),
        })?;
        let format = first.format;
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
        let total_bytes_u64 = u64::try_from(total_bytes).map_err(|_| Error::Unsupported {
            reason: "Metal packed WholeLevel tile byte length exceeds u64".into(),
        })?;
        let packed = self.device.new_buffer(
            total_bytes_u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        let command_buffer = self.queue.new_command_buffer();
        if metal_profile_stages_enabled() {
            command_buffer.set_label("wsi-dicom input tile pack");
        }
        let blit = command_buffer.new_blit_command_encoder();
        if metal_profile_stages_enabled() {
            blit.set_label("WSI input tile pack");
        }

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
            let statumen::output::metal::MetalDeviceStorage::Buffer {
                buffer,
                byte_offset,
            } = &tile.storage;
            let slot_offset =
                idx.checked_mul(tile_slot_bytes)
                    .ok_or_else(|| Error::Unsupported {
                        reason: "Metal packed WholeLevel destination offset overflow".into(),
                    })?;
            for source_row in 0..tile.height as usize {
                let source_offset = byte_offset
                    .checked_add(source_row.checked_mul(tile.pitch_bytes).ok_or_else(|| {
                        Error::Unsupported {
                            reason: "Metal WholeLevel source row offset overflow".into(),
                        }
                    })?)
                    .ok_or_else(|| Error::Unsupported {
                        reason: "Metal WholeLevel source row offset overflow".into(),
                    })?;
                let destination_offset = slot_offset
                    .checked_add(source_row.checked_mul(slot_stride).ok_or_else(|| {
                        Error::Unsupported {
                            reason: "Metal WholeLevel destination row offset overflow".into(),
                        }
                    })?)
                    .ok_or_else(|| Error::Unsupported {
                        reason: "Metal WholeLevel destination row offset overflow".into(),
                    })?;
                blit.copy_from_buffer(
                    buffer,
                    u64::try_from(source_offset).map_err(|_| Error::Unsupported {
                        reason: "Metal WholeLevel source row offset exceeds u64".into(),
                    })?,
                    &packed,
                    u64::try_from(destination_offset).map_err(|_| Error::Unsupported {
                        reason: "Metal WholeLevel destination row offset exceeds u64".into(),
                    })?,
                    u64::try_from(row_bytes).map_err(|_| Error::Unsupported {
                        reason: "Metal WholeLevel source row byte length exceeds u64".into(),
                    })?,
                );
            }
        }

        blit.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        Ok(PackedMetalStrips {
            buffer: packed,
            first_col,
            first_row,
            tiles_across: tiles_across_u32,
            tile_width: layout.width,
            tile_height: layout.height,
            slot_stride,
            tile_slot_bytes,
            format,
        })
    }

    pub(super) fn compose_tiles(
        &self,
        packed: &PackedMetalStrips,
        requests: &[MetalComposeTileRequest],
    ) -> Result<Vec<statumen::output::metal::MetalDeviceTile>, Error> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        let first_col = u32::try_from(packed.first_col).map_err(|_| Error::Unsupported {
            reason: "Metal WholeLevel first source tile column exceeds u32".into(),
        })?;
        let first_row = u32::try_from(packed.first_row).map_err(|_| Error::Unsupported {
            reason: "Metal WholeLevel first source tile row exceeds u32".into(),
        })?;
        let bytes_per_pixel = packed.format.bytes_per_pixel();
        let bytes_per_pixel_u32 =
            u32::try_from(bytes_per_pixel).map_err(|_| Error::Unsupported {
                reason: "Metal composed tile bytes-per-pixel exceeds u32".into(),
            })?;
        let src_slot_stride =
            u32::try_from(packed.slot_stride).map_err(|_| Error::Unsupported {
                reason: "Metal WholeLevel source slot stride exceeds u32".into(),
            })?;
        let src_tile_slot_bytes =
            u32::try_from(packed.tile_slot_bytes).map_err(|_| Error::Unsupported {
                reason: "Metal WholeLevel source tile slot byte length exceeds u32".into(),
            })?;
        let mut dispatches = Vec::with_capacity(requests.len());
        for request in requests {
            let dst_stride = (request.output_width as usize)
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| Error::Unsupported {
                    reason: "Metal composed tile stride overflow".into(),
                })?;
            let dst_bytes = dst_stride
                .checked_mul(request.output_height as usize)
                .ok_or_else(|| Error::Unsupported {
                    reason: "Metal composed tile byte length overflow".into(),
                })?;
            let dst_bytes_u64 = u64::try_from(dst_bytes).map_err(|_| Error::Unsupported {
                reason: "Metal composed tile byte length exceeds u64".into(),
            })?;
            let dst_buffer = self
                .device
                .new_buffer(dst_bytes_u64, metal::MTLResourceOptions::StorageModeShared);
            let params = MetalComposeStripsParams {
                src_origin_x: request.src_origin_x,
                src_origin_y: request.src_origin_y,
                valid_width: request.valid_width,
                valid_height: request.valid_height,
                output_width: request.output_width,
                output_height: request.output_height,
                bytes_per_pixel: bytes_per_pixel_u32,
                src_tile_width: packed.tile_width,
                src_tile_height: packed.tile_height,
                src_slot_stride,
                src_tile_slot_bytes,
                src_first_col: first_col,
                src_first_row: first_row,
                src_tiles_across: packed.tiles_across,
                dst_stride: u32::try_from(dst_stride).map_err(|_| Error::Unsupported {
                    reason: "Metal composed tile pitch exceeds u32".into(),
                })?,
            };
            dispatches.push(MetalComposeTileDispatch {
                request: *request,
                params,
                dst_buffer,
                dst_stride,
            });
        }

        let command_buffer = self.queue.new_command_buffer();
        if metal_profile_stages_enabled() {
            command_buffer.set_label("wsi-dicom compose tiles");
        }
        let encoder = command_buffer.new_compute_command_encoder();
        if metal_profile_stages_enabled() {
            encoder.set_label("WSI compose tiles");
        }
        encoder.set_compute_pipeline_state(&self.pipeline);
        encoder.set_buffer(0, Some(&packed.buffer), 0);
        let width = self.pipeline.thread_execution_width().max(1);
        let max_threads = self.pipeline.max_total_threads_per_threadgroup().max(width);
        let height = (max_threads / width).max(1);
        for dispatch in &dispatches {
            encoder.set_buffer(1, Some(&dispatch.dst_buffer), 0);
            encoder.set_bytes(
                2,
                core::mem::size_of::<MetalComposeStripsParams>() as u64,
                (&raw const dispatch.params).cast(),
            );
            encoder.dispatch_threads(
                metal::MTLSize {
                    width: u64::from(dispatch.request.output_width),
                    height: u64::from(dispatch.request.output_height),
                    depth: 1,
                },
                metal::MTLSize {
                    width,
                    height,
                    depth: 1,
                },
            );
        }
        encoder.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        Ok(dispatches
            .into_iter()
            .map(|dispatch| statumen::output::metal::MetalDeviceTile {
                width: dispatch.request.output_width,
                height: dispatch.request.output_height,
                pitch_bytes: dispatch.dst_stride,
                format: packed.format,
                storage: statumen::output::metal::MetalDeviceStorage::Buffer {
                    buffer: dispatch.dst_buffer,
                    byte_offset: 0,
                },
            })
            .collect())
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct MetalComposeStripsParams {
    pub(super) src_origin_x: u32,
    pub(super) src_origin_y: u32,
    pub(super) valid_width: u32,
    pub(super) valid_height: u32,
    pub(super) output_width: u32,
    pub(super) output_height: u32,
    pub(super) bytes_per_pixel: u32,
    pub(super) src_tile_width: u32,
    pub(super) src_tile_height: u32,
    pub(super) src_slot_stride: u32,
    pub(super) src_tile_slot_bytes: u32,
    pub(super) src_first_col: u32,
    pub(super) src_first_row: u32,
    pub(super) src_tiles_across: u32,
    pub(super) dst_stride: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
const WSI_COMPOSE_STRIPS_METAL: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct MetalComposeStripsParams {
    uint src_origin_x;
    uint src_origin_y;
    uint valid_width;
    uint valid_height;
    uint output_width;
    uint output_height;
    uint bytes_per_pixel;
    uint src_tile_width;
    uint src_tile_height;
    uint src_slot_stride;
    uint src_tile_slot_bytes;
    uint src_first_col;
    uint src_first_row;
    uint src_tiles_across;
    uint dst_stride;
};

kernel void wsi_compose_strips(
    device const uchar *src [[buffer(0)]],
    device uchar *dst [[buffer(1)]],
    constant MetalComposeStripsParams &params [[buffer(2)]],
    uint2 gid [[thread_position_in_grid]]
) {
    if (gid.x >= params.output_width || gid.y >= params.output_height) {
        return;
    }

    const uint dst_idx = gid.y * params.dst_stride + gid.x * params.bytes_per_pixel;
    const bool inside = gid.x < params.valid_width && gid.y < params.valid_height;
    if (!inside) {
        for (uint byte_idx = 0u; byte_idx < params.bytes_per_pixel; ++byte_idx) {
            dst[dst_idx + byte_idx] = uchar(0);
        }
        return;
    }

    const uint global_x = params.src_origin_x + gid.x;
    const uint global_y = params.src_origin_y + gid.y;
    const uint source_col = global_x / params.src_tile_width;
    const uint source_row = global_y / params.src_tile_height;
    const uint in_tile_x = global_x - source_col * params.src_tile_width;
    const uint in_tile_y = global_y - source_row * params.src_tile_height;
    const uint packed_col = source_col - params.src_first_col;
    const uint packed_row = source_row - params.src_first_row;
    const uint tile_idx = packed_row * params.src_tiles_across + packed_col;
    const uint src_idx = tile_idx * params.src_tile_slot_bytes
        + in_tile_y * params.src_slot_stride
        + in_tile_x * params.bytes_per_pixel;
    for (uint byte_idx = 0u; byte_idx < params.bytes_per_pixel; ++byte_idx) {
        dst[dst_idx + byte_idx] = src[src_idx + byte_idx];
    }
}
"#;

#[cfg(all(test, feature = "metal", target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn metal_compose_params_layout_matches_shader_struct() {
        let rust_fields = [
            "src_origin_x",
            "src_origin_y",
            "valid_width",
            "valid_height",
            "output_width",
            "output_height",
            "bytes_per_pixel",
            "src_tile_width",
            "src_tile_height",
            "src_slot_stride",
            "src_tile_slot_bytes",
            "src_first_col",
            "src_first_row",
            "src_tiles_across",
            "dst_stride",
        ];
        let shader_fields = WSI_COMPOSE_STRIPS_METAL
            .lines()
            .map(str::trim)
            .filter_map(|line| line.strip_prefix("uint "))
            .map(|field| field.trim_end_matches(';'))
            .collect::<Vec<_>>();

        assert_eq!(shader_fields, rust_fields);
        assert_eq!(
            core::mem::size_of::<MetalComposeStripsParams>(),
            rust_fields.len() * core::mem::size_of::<u32>()
        );
        assert_eq!(
            core::mem::align_of::<MetalComposeStripsParams>(),
            core::mem::align_of::<u32>()
        );
    }
}
