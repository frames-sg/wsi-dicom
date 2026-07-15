use super::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum ComposeAddressWidth {
    U32,
    U64,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
pub(super) struct ComposeAddressPlan {
    pub(super) request: MetalComposeTileRequest,
    pub(super) params: MetalComposeStripsParams,
    pub(super) dst_stride: usize,
    pub(super) dst_bytes: usize,
    pub(super) address_width: ComposeAddressWidth,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl ComposeAddressPlan {
    pub(super) fn new(
        request: MetalComposeTileRequest,
        packed: &PackedMetalStrips,
        first_col: u32,
        first_row: u32,
        bytes_per_pixel: u32,
    ) -> Result<Self, Error> {
        if bytes_per_pixel == 0 {
            return Err(Error::Unsupported {
                reason: "Metal composed bytes-per-pixel must be nonzero".into(),
            });
        }
        validate_compose_request_bounds(
            &request,
            first_col,
            first_row,
            packed.tiles_across,
            packed.tiles_down,
            packed.tile_width,
            packed.tile_height,
        )?;
        let dst_stride = usize::try_from(request.output_width)
            .ok()
            .and_then(|width| width.checked_mul(bytes_per_pixel as usize))
            .ok_or_else(|| Error::Unsupported {
                reason: "Metal composed tile stride overflow".into(),
            })?;
        let dst_bytes = dst_stride
            .checked_mul(usize::try_from(request.output_height).map_err(|_| {
                Error::Unsupported {
                    reason: "Metal composed tile height exceeds addressable memory".into(),
                }
            })?)
            .ok_or_else(|| Error::Unsupported {
                reason: "Metal composed tile byte length overflow".into(),
            })?;
        let src_slot_stride =
            u32::try_from(packed.slot_stride).map_err(|_| Error::Unsupported {
                reason: "Metal WholeLevel source slot stride exceeds the shader ABI".into(),
            })?;
        let src_tile_slot_bytes =
            u32::try_from(packed.tile_slot_bytes).map_err(|_| Error::Unsupported {
                reason: "Metal WholeLevel source tile slot byte length exceeds the shader ABI"
                    .into(),
            })?;
        let dst_stride_u32 = u32::try_from(dst_stride).map_err(|_| Error::Unsupported {
            reason: "Metal composed tile pitch exceeds the shader ABI".into(),
        })?;
        let params = MetalComposeStripsParams {
            src_origin_x: request.src_origin_x,
            src_origin_y: request.src_origin_y,
            valid_width: request.valid_width,
            valid_height: request.valid_height,
            output_width: request.output_width,
            output_height: request.output_height,
            bytes_per_pixel,
            src_tile_width: packed.tile_width,
            src_tile_height: packed.tile_height,
            src_slot_stride,
            src_tile_slot_bytes,
            src_first_col: first_col,
            src_first_row: first_row,
            src_tiles_across: packed.tiles_across,
            dst_stride: dst_stride_u32,
        };
        let max_dst = max_destination_byte(&params)?;
        let dst_bytes_u64 = u64::try_from(dst_bytes).map_err(|_| Error::Unsupported {
            reason: "Metal composed destination allocation exceeds u64".into(),
        })?;
        if max_dst >= dst_bytes_u64 {
            return Err(Error::Unsupported {
                reason: "Metal composed destination shader span exceeds its allocation".into(),
            });
        }
        let max_src = max_source_byte(&params)?;
        if let Some(max_src) = max_src {
            let packed_len =
                u64::try_from(packed.image.byte_len()).map_err(|_| Error::Unsupported {
                    reason: "Metal packed source allocation exceeds u64".into(),
                })?;
            if max_src >= packed_len {
                return Err(Error::Unsupported {
                    reason: "Metal composed source shader span exceeds the packed allocation"
                        .into(),
                });
            }
        }
        let address_width = select_address_width(max_src, max_dst);
        Ok(Self {
            request,
            params,
            dst_stride,
            dst_bytes,
            address_width,
        })
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn select_address_width(
    max_source_byte: Option<u64>,
    max_destination_byte: u64,
) -> ComposeAddressWidth {
    if max_destination_byte <= u64::from(u32::MAX)
        && max_source_byte.is_none_or(|source| source <= u64::from(u32::MAX))
    {
        ComposeAddressWidth::U32
    } else {
        ComposeAddressWidth::U64
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn max_destination_byte(params: &MetalComposeStripsParams) -> Result<u64, Error> {
    if params.output_width == 0 || params.output_height == 0 || params.bytes_per_pixel == 0 {
        return Err(Error::Unsupported {
            reason: "Metal composed output dimensions must be nonzero".into(),
        });
    }
    u64::from(params.output_height - 1)
        .checked_mul(u64::from(params.dst_stride))
        .and_then(|row| {
            row.checked_add(
                u64::from(params.output_width - 1)
                    .checked_mul(u64::from(params.bytes_per_pixel))?,
            )
        })
        .and_then(|pixel| pixel.checked_add(u64::from(params.bytes_per_pixel - 1)))
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed destination address calculation overflow".into(),
        })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn max_source_byte(params: &MetalComposeStripsParams) -> Result<Option<u64>, Error> {
    if params.valid_width == 0 || params.valid_height == 0 {
        return Ok(None);
    }
    if params.src_tile_width == 0 || params.src_tile_height == 0 || params.bytes_per_pixel == 0 {
        return Err(Error::Unsupported {
            reason: "Metal composed source geometry must be nonzero".into(),
        });
    }
    let global_x = params
        .src_origin_x
        .checked_add(params.valid_width - 1)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source x address overflow".into(),
        })?;
    let global_y = params
        .src_origin_y
        .checked_add(params.valid_height - 1)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source y address overflow".into(),
        })?;
    let source_col = global_x / params.src_tile_width;
    let source_row = global_y / params.src_tile_height;
    let packed_col = source_col
        .checked_sub(params.src_first_col)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source column precedes the packed grid".into(),
        })?;
    let packed_row = source_row
        .checked_sub(params.src_first_row)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source row precedes the packed grid".into(),
        })?;
    let in_tile_x = global_x % params.src_tile_width;
    let in_tile_y = global_y % params.src_tile_height;
    let tile_idx = u64::from(packed_row)
        .checked_mul(u64::from(params.src_tiles_across))
        .and_then(|row| row.checked_add(u64::from(packed_col)))
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source tile-grid multiplication overflow".into(),
        })?;
    tile_idx
        .checked_mul(u64::from(params.src_tile_slot_bytes))
        .and_then(|base| {
            base.checked_add(u64::from(in_tile_y).checked_mul(u64::from(params.src_slot_stride))?)
        })
        .and_then(|row| {
            row.checked_add(u64::from(in_tile_x).checked_mul(u64::from(params.bytes_per_pixel))?)
        })
        .and_then(|pixel| pixel.checked_add(u64::from(params.bytes_per_pixel - 1)))
        .map(Some)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source address calculation overflow".into(),
        })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn validate_compose_request_bounds(
    request: &MetalComposeTileRequest,
    first_col: u32,
    first_row: u32,
    tiles_across: u32,
    tiles_down: u32,
    tile_width: u32,
    tile_height: u32,
) -> Result<(), Error> {
    if request.valid_width > request.output_width || request.valid_height > request.output_height {
        return Err(Error::Unsupported {
            reason: "Metal composed tile valid geometry exceeds its output geometry".into(),
        });
    }
    if request.valid_width == 0 || request.valid_height == 0 {
        return Ok(());
    }

    let source_min_x = first_col
        .checked_mul(tile_width)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source x origin overflow".into(),
        })?;
    let source_min_y = first_row
        .checked_mul(tile_height)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source y origin overflow".into(),
        })?;
    let source_max_x = first_col
        .checked_add(tiles_across)
        .and_then(|columns| columns.checked_mul(tile_width))
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source x extent overflow".into(),
        })?;
    let source_max_y = first_row
        .checked_add(tiles_down)
        .and_then(|rows| rows.checked_mul(tile_height))
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed source y extent overflow".into(),
        })?;
    let request_max_x = request
        .src_origin_x
        .checked_add(request.valid_width)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed request x extent overflow".into(),
        })?;
    let request_max_y = request
        .src_origin_y
        .checked_add(request.valid_height)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal composed request y extent overflow".into(),
        })?;
    if request.src_origin_x < source_min_x
        || request.src_origin_y < source_min_y
        || request_max_x > source_max_x
        || request_max_y > source_max_y
    {
        return Err(Error::Unsupported {
            reason: "Metal composed request reads outside the packed source tile grid".into(),
        });
    }
    Ok(())
}
