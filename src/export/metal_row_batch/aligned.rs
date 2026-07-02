use super::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
struct MetalAlignedGridRead {
    tile_count: usize,
    tile_entries: Option<Vec<MetalTileEntry>>,
    input_decode_duration: Duration,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn aligned_tile_location(
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
) -> JpegBaselineFrameLocation {
    JpegBaselineFrameLocation {
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn aligned_tile_row_requests(
    location: JpegBaselineFrameLocation,
    row: u64,
    start_col: u64,
    tile_count: usize,
) -> Result<Vec<TileRequest>, Error> {
    let row_i64 = i64::try_from(row).map_err(|_| Error::Unsupported {
        reason: "tile row exceeds i64".into(),
    })?;
    let start_col_i64 = i64::try_from(start_col).map_err(|_| Error::Unsupported {
        reason: "tile column exceeds i64".into(),
    })?;
    let mut requests = Vec::with_capacity(tile_count);
    for offset in 0..tile_count {
        let col = start_col_i64
            .checked_add(i64::try_from(offset).map_err(|_| Error::Unsupported {
                reason: "tile batch offset exceeds i64".into(),
            })?)
            .ok_or_else(|| Error::Unsupported {
                reason: "tile column overflow".into(),
            })?;
        requests.push(location.tile_request(col, row_i64));
    }
    Ok(requests)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn aligned_tile_grid_requests(
    location: JpegBaselineFrameLocation,
    start_row: u64,
    tiles_across: usize,
    row_count: usize,
) -> Result<Vec<TileRequest>, Error> {
    let tile_count = tiles_across
        .checked_mul(row_count)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal aligned tile grid batch size overflow".into(),
        })?;
    let start_row_i64 = i64::try_from(start_row).map_err(|_| Error::Unsupported {
        reason: "tile row exceeds i64".into(),
    })?;
    let mut requests = Vec::with_capacity(tile_count);
    for row_offset in 0..row_count {
        let row_i64 = start_row_i64
            .checked_add(i64::try_from(row_offset).map_err(|_| Error::Unsupported {
                reason: "tile row batch offset exceeds i64".into(),
            })?)
            .ok_or_else(|| Error::Unsupported {
                reason: "tile row overflow".into(),
            })?;
        for col in 0..tiles_across {
            let col = i64::try_from(col).map_err(|_| Error::Unsupported {
                reason: "tile column exceeds i64".into(),
            })?;
            requests.push(location.tile_request(col, row_i64));
        }
    }
    Ok(requests)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn read_aligned_tile_grid_entries(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    location: JpegBaselineFrameLocation,
    start_row: u64,
    tiles_across: usize,
    row_count: usize,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
) -> Result<MetalAlignedGridRead, Error> {
    let requests = aligned_tile_grid_requests(location, start_row, tiles_across, row_count)?;
    let tile_count = requests.len();

    let input_decode_started = Instant::now();
    let pixels = match slide.read_tiles(&requests, metal_input.source_tile_output_preference()?) {
        Ok(pixels) => pixels,
        Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
            return Err(Error::SlideRead {
                message: format!("Metal input tile grid decode failed: {err}"),
            });
        }
        Err(_) => {
            return Ok(MetalAlignedGridRead {
                tile_count,
                tile_entries: None,
                input_decode_duration: Duration::ZERO,
            });
        }
    };
    let input_decode_duration = input_decode_started.elapsed();

    if pixels.len() != tile_count {
        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(Error::SlideRead {
                message: format!(
                    "Metal input tile grid returned {} tile(s), expected {tile_count}",
                    pixels.len()
                ),
            });
        }
        return Ok(MetalAlignedGridRead {
            tile_count,
            tile_entries: None,
            input_decode_duration: Duration::ZERO,
        });
    }

    let mut tile_entries = Vec::with_capacity(tile_count);
    for (idx, pixels) in pixels.into_iter().enumerate() {
        let row_offset = idx / tiles_across;
        let col = idx % tiles_across;
        let x = u64::try_from(col)
            .ok()
            .and_then(|col| col.checked_mul(u64::from(tile_size)))
            .ok_or_else(|| Error::Unsupported {
                reason: "tile x offset overflow".into(),
            })?;
        let y = start_row
            .checked_add(u64::try_from(row_offset).map_err(|_| Error::Unsupported {
                reason: "tile row batch offset exceeds u64".into(),
            })?)
            .and_then(|row| row.checked_mul(u64::from(tile_size)))
            .ok_or_else(|| Error::Unsupported {
                reason: "tile y offset overflow".into(),
            })?;
        let width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
        let height = (matrix_rows - y).min(u64::from(tile_size)) as u32;

        let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason:
                        "requested Metal input tile grid decode returned CPU pixels; set WSI_RS_JPEG_DEVICE_DECODE=1 or WSI_RS_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                            .into(),
                });
            }
            tile_entries.push(None);
            continue;
        };

        if tile.width != width || tile.height != height {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason: format!(
                        "Metal input tile grid geometry changed: expected {}x{}, got {}x{}",
                        width, height, tile.width, tile.height
                    ),
                });
            }
            tile_entries.push(None);
            continue;
        }

        let profile = pixel_profile_from_wsi_device_format(tile.format)?;
        tile_entries.push(Some((tile, profile)));
    }

    Ok(MetalAlignedGridRead {
        tile_count,
        tile_entries: Some(tile_entries),
        input_decode_duration,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(in crate::export) fn try_encode_metal_aligned_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &wsi_rs::Level,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    row: u64,
    start_col: u64,
    tile_count: usize,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
) -> Result<MetalEncodedTileRun, Error> {
    if !output_tile_maps_to_wsi_rs_tile(level, tile_size) {
        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(Error::Unsupported {
                reason:
                    "requested Metal input tile decode requires the DICOM tile grid to align with wsi-rs source tiles"
                        .into(),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let requests = aligned_tile_row_requests(
        aligned_tile_location(scene_idx, series_idx, level_idx, z, c, t),
        row,
        start_col,
        tile_count,
    )?;

    let input_decode_started = Instant::now();
    let pixels = match slide.read_tiles(&requests, metal_input.source_tile_output_preference()?) {
        Ok(pixels) => pixels,
        Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
            return Err(Error::SlideRead {
                message: format!("Metal input tile batch decode failed: {err}"),
            });
        }
        Err(_) => return Ok(empty_metal_tile_run(tile_count)),
    };
    let input_decode_duration = input_decode_started.elapsed();

    if pixels.len() != tile_count {
        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(Error::SlideRead {
                message: format!(
                    "Metal input tile batch returned {} tile(s), expected {}",
                    pixels.len(),
                    tile_count
                ),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let mut tile_entries = Vec::with_capacity(tile_count);
    for (offset, pixels) in pixels.into_iter().enumerate() {
        let col = start_col
            .checked_add(u64::try_from(offset).map_err(|_| Error::Unsupported {
                reason: "tile batch offset exceeds u64".into(),
            })?)
            .ok_or_else(|| Error::Unsupported {
                reason: "tile column overflow".into(),
            })?;
        let x = col
            .checked_mul(u64::from(tile_size))
            .ok_or_else(|| Error::Unsupported {
                reason: "tile x offset overflow".into(),
            })?;
        let y = row
            .checked_mul(u64::from(tile_size))
            .ok_or_else(|| Error::Unsupported {
                reason: "tile y offset overflow".into(),
            })?;
        let width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
        let height = (matrix_rows - y).min(u64::from(tile_size)) as u32;

        let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason:
                        "requested Metal input tile decode returned CPU pixels; set WSI_RS_JPEG_DEVICE_DECODE=1 or WSI_RS_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                            .into(),
                });
            }
            tile_entries.push(None);
            continue;
        };

        if tile.width != width || tile.height != height {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason: format!(
                        "Metal input tile geometry changed: expected {}x{}, got {}x{}",
                        width, height, tile.width, tile.height
                    ),
                });
            }
            tile_entries.push(None);
            continue;
        }

        let profile = pixel_profile_from_wsi_device_format(tile.format)?;
        tile_entries.push(Some((tile, profile)));
    }

    let encoded_entries = encode_metal_tile_entries(
        j2k_encoder,
        tile_entries,
        tile_size,
        tile_size,
        metal_input.preference,
        "requested JPEG 2000 Metal tile encode did not dispatch all required stages",
    )?;

    Ok(MetalEncodedTileRun {
        tiles: encoded_entries.tiles,
        input_decode_duration,
        compose_duration: Duration::ZERO,
        input_decode_batches: 1,
        compose_batches: 0,
        encode_batches: encoded_entries.encode_batches,
        gpu_encode_stats: encoded_entries.gpu_encode_stats,
        row_batch_rows: 1,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_encode_metal_aligned_tile_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    start_row: u64,
    tiles_across: usize,
    row_count: usize,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
) -> Result<MetalEncodedTileRun, Error> {
    let read = read_aligned_tile_grid_entries(
        slide,
        metal_input,
        aligned_tile_location(scene_idx, series_idx, level_idx, z, c, t),
        start_row,
        tiles_across,
        row_count,
        matrix_columns,
        matrix_rows,
        tile_size,
    )?;
    let Some(tile_entries) = read.tile_entries else {
        return Ok(empty_metal_tile_run(read.tile_count));
    };

    let encoded_entries = encode_metal_tile_entries(
        j2k_encoder,
        tile_entries,
        tile_size,
        tile_size,
        metal_input.preference,
        "requested JPEG 2000 Metal tile grid encode did not dispatch all required stages",
    )?;

    Ok(MetalEncodedTileRun {
        tiles: encoded_entries.tiles,
        input_decode_duration: read.input_decode_duration,
        compose_duration: Duration::ZERO,
        input_decode_batches: 1,
        compose_batches: 0,
        encode_batches: encoded_entries.encode_batches,
        gpu_encode_stats: encoded_entries.gpu_encode_stats,
        row_batch_rows: row_count,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_submit_metal_aligned_tile_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    start_row: u64,
    tiles_across: usize,
    row_count: usize,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
) -> Result<PendingMetalEncodedTileRun, Error> {
    let read = read_aligned_tile_grid_entries(
        slide,
        metal_input,
        aligned_tile_location(scene_idx, series_idx, level_idx, z, c, t),
        start_row,
        tiles_across,
        row_count,
        matrix_columns,
        matrix_rows,
        tile_size,
    )?;
    let Some(tile_entries) = read.tile_entries else {
        return empty_pending_metal_tile_run(
            j2k_encoder,
            read.tile_count,
            tile_size,
            tile_size,
            row_count,
            metal_input,
        );
    };

    let (batch_tiles, tile_profiles) = split_metal_tile_entries(tile_entries);
    let encode_batches = metal_j2k_encode_batch_count(&batch_tiles, tile_size, tile_size);
    let submission = j2k_encoder.submit_metal_tiles_owned(batch_tiles, tile_size, tile_size)?;

    Ok(PendingMetalEncodedTileRun {
        tile_profiles,
        submission,
        input_decode_duration: read.input_decode_duration,
        compose_duration: Duration::ZERO,
        input_decode_batches: 1,
        compose_batches: 0,
        encode_batches,
        row_batch_rows: row_count,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
        preference: metal_input.preference,
        missing_encode_message:
            "requested JPEG 2000 Metal tile grid encode did not dispatch all required stages",
    })
}
