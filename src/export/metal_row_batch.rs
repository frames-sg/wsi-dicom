use super::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_encode_metal_input_tile_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &statumen::Level,
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
    first_row_key: MetalEncodedRowRunKey,
) -> Result<Option<MetalEncodedTileRun>, WsiDicomError> {
    let tiles_across = matrix_columns.div_ceil(u64::from(tile_size));
    if start_col != 0 || u64::try_from(tile_count).ok() != Some(tiles_across) {
        return Ok(None);
    }
    let row_count = metal_row_batch_rows(
        row,
        matrix_rows.div_ceil(u64::from(tile_size)),
        tile_count,
        metal_input.row_batch_rows,
        metal_input.row_batch_target_tiles,
    )?;
    if row_count <= 1 {
        return Ok(None);
    }

    let grid_run = if output_tile_maps_to_statumen_tile(level, tile_size) {
        try_encode_metal_aligned_tile_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            row,
            tile_count,
            row_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        )?
    } else if let Some(source_layout) = regular_tiled_source_layout(level) {
        try_encode_metal_whole_level_strip_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            source_layout,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            row,
            tile_count,
            row_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        )?
    } else if let Some(strip_layout) = whole_level_strip_layout(level) {
        try_encode_metal_whole_level_strip_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            strip_layout,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            row,
            tile_count,
            row_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        )?
    } else {
        return Ok(None);
    };

    Ok(Some(cache_split_metal_grid_run(
        metal_input,
        first_row_key,
        grid_run,
        tile_count,
        row_count,
    )?))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_encode_metal_input_tile_grid_pipeline_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &statumen::Level,
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
    first_row_key: MetalEncodedRowRunKey,
) -> Result<Option<MetalEncodedTileRun>, WsiDicomError> {
    let tiles_across = matrix_columns.div_ceil(u64::from(tile_size));
    if start_col != 0 || u64::try_from(tile_count).ok() != Some(tiles_across) {
        return Ok(None);
    }
    if metal_input.pipeline_depth <= 1 {
        return try_encode_metal_input_tile_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            level,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            row,
            start_col,
            tile_count,
            matrix_columns,
            matrix_rows,
            tile_size,
            first_row_key,
        );
    }

    let tiles_down = matrix_rows.div_ceil(u64::from(tile_size));
    if metal_input
        .next_grid_pipeline_row
        .is_none_or(|next| next < row)
    {
        metal_input.next_grid_pipeline_row = Some(row);
    }

    while metal_input.pending_encoded_grid_runs.len() < metal_input.pipeline_depth {
        let Some(submit_row) = metal_input.next_grid_pipeline_row else {
            break;
        };
        if submit_row >= tiles_down {
            break;
        }
        let row_count = metal_row_batch_rows(
            submit_row,
            tiles_down,
            tile_count,
            metal_input.row_batch_rows,
            metal_input.row_batch_target_tiles,
        )?;
        if row_count <= 1 {
            break;
        }
        let submit_key = MetalEncodedRowRunKey {
            scene: scene_idx,
            series: series_idx,
            level: level_idx,
            z,
            c,
            t,
            row: submit_row,
            start_col,
            tile_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        };
        if metal_input
            .pending_encoded_grid_runs
            .contains_key(&submit_key)
            || metal_input.encoded_row_runs.contains_key(&submit_key)
        {
            metal_input.next_grid_pipeline_row =
                Some(next_metal_grid_pipeline_row(submit_row, row_count)?);
            continue;
        }
        let Some(run) = try_submit_metal_input_tile_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            level,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            submit_row,
            tile_count,
            row_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        )?
        else {
            break;
        };
        metal_input.pending_encoded_grid_runs.insert(
            submit_key,
            PendingMetalEncodedGridRun {
                run,
                first_row_key: submit_key,
                tiles_per_row: tile_count,
                row_count,
            },
        );
        metal_input.next_grid_pipeline_row =
            Some(next_metal_grid_pipeline_row(submit_row, row_count)?);
    }

    let Some(pending) = metal_input.pending_encoded_grid_runs.remove(&first_row_key) else {
        return Ok(None);
    };
    let grid_run = pending.run.wait()?;
    Ok(Some(cache_split_metal_grid_run(
        metal_input,
        pending.first_row_key,
        grid_run,
        pending.tiles_per_row,
        pending.row_count,
    )?))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn next_metal_grid_pipeline_row(row: u64, row_count: usize) -> Result<u64, WsiDicomError> {
    row.checked_add(
        u64::try_from(row_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "Metal row batch pipeline row count exceeds u64".into(),
        })?,
    )
    .ok_or_else(|| WsiDicomError::Unsupported {
        reason: "Metal row batch pipeline row overflow".into(),
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_row_batch_rows(
    row: u64,
    tiles_down: u64,
    tiles_across: usize,
    configured_rows: Option<usize>,
    configured_target_tiles: Option<usize>,
) -> Result<usize, WsiDicomError> {
    if tiles_across == 0 || row >= tiles_down {
        return Ok(1);
    }
    let requested = if let Some(rows) = configured_rows {
        rows
    } else if let Some(target_tiles) = configured_target_tiles {
        target_tiles.div_ceil(tiles_across)
    } else if let Some(rows) = configured_metal_row_batch_rows()? {
        rows
    } else {
        DEFAULT_METAL_ROW_BATCH_TARGET_TILES.div_ceil(tiles_across)
    }
    .max(1);
    let remaining_rows =
        usize::try_from(tiles_down - row).map_err(|_| WsiDicomError::Unsupported {
            reason: "remaining tile rows exceed platform addressable memory".into(),
        })?;
    Ok(requested.min(remaining_rows))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn configured_metal_row_batch_rows() -> Result<Option<usize>, WsiDicomError> {
    let value = match std::env::var(WSI_DICOM_METAL_ROW_BATCH_ROWS_ENV) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Err(err) => {
            return Err(WsiDicomError::InvalidOptions {
                reason: format!("{WSI_DICOM_METAL_ROW_BATCH_ROWS_ENV} is not valid UTF-8: {err}"),
            });
        }
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let rows = trimmed
        .parse::<usize>()
        .map_err(|_| WsiDicomError::InvalidOptions {
            reason: format!("{WSI_DICOM_METAL_ROW_BATCH_ROWS_ENV} must be a positive integer"),
        })?;
    if rows == 0 {
        return Err(WsiDicomError::InvalidOptions {
            reason: format!("{WSI_DICOM_METAL_ROW_BATCH_ROWS_ENV} must be greater than zero"),
        });
    }
    Ok(Some(rows))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn cache_split_metal_grid_run(
    metal_input: &mut MetalInputTileReader,
    first_row_key: MetalEncodedRowRunKey,
    mut grid_run: MetalEncodedTileRun,
    tiles_per_row: usize,
    row_count: usize,
) -> Result<MetalEncodedTileRun, WsiDicomError> {
    let expected_tiles =
        tiles_per_row
            .checked_mul(row_count)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal row batch tile count overflow".into(),
            })?;
    if grid_run.tiles.len() != expected_tiles {
        return Err(WsiDicomError::Encode {
            message: format!(
                "Metal row batch produced {} tile(s), expected {expected_tiles}",
                grid_run.tiles.len()
            ),
        });
    }

    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        let row_tiles = grid_run.tiles.drain(..tiles_per_row).collect::<Vec<_>>();
        rows.push(MetalEncodedTileRun {
            tiles: row_tiles,
            input_decode_duration: Duration::ZERO,
            compose_duration: Duration::ZERO,
            input_decode_batches: 0,
            compose_batches: 0,
            encode_batches: 0,
            gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats::default(),
            row_batch_rows: 0,
            row_batch_target_tiles: None,
        });
    }

    let mut first = rows.remove(0);
    first.input_decode_duration = grid_run.input_decode_duration;
    first.compose_duration = grid_run.compose_duration;
    first.input_decode_batches = grid_run.input_decode_batches;
    first.compose_batches = grid_run.compose_batches;
    first.encode_batches = grid_run.encode_batches;
    first.gpu_encode_stats = grid_run.gpu_encode_stats;
    first.row_batch_rows = row_count;
    first.row_batch_target_tiles = grid_run.row_batch_target_tiles;

    for (offset, run) in rows.into_iter().enumerate() {
        let mut key = first_row_key;
        key.row = key
            .row
            .checked_add(
                u64::try_from(offset + 1).map_err(|_| WsiDicomError::Unsupported {
                    reason: "Metal row batch cache offset exceeds u64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal row batch cache row overflow".into(),
            })?;
        metal_input.encoded_row_runs.insert(key, run);
    }

    Ok(first)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn try_submit_metal_input_tile_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &statumen::Level,
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
) -> Result<Option<PendingMetalEncodedTileRun>, WsiDicomError> {
    if output_tile_maps_to_statumen_tile(level, tile_size) {
        return try_submit_metal_aligned_tile_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            start_row,
            tiles_across,
            row_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        )
        .map(Some);
    }
    if let Some(source_layout) = regular_tiled_source_layout(level) {
        return try_submit_metal_whole_level_strip_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            source_layout,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            start_row,
            tiles_across,
            row_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        )
        .map(Some);
    }
    if let Some(strip_layout) = whole_level_strip_layout(level) {
        return try_submit_metal_whole_level_strip_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            strip_layout,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            start_row,
            tiles_across,
            row_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        )
        .map(Some);
    }
    Ok(None)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn empty_pending_metal_tile_run(
    j2k_encoder: &mut DicomJ2kEncoder,
    tile_count: usize,
    output_width: u32,
    output_height: u32,
    row_batch_rows: usize,
    metal_input: &MetalInputTileReader,
) -> Result<PendingMetalEncodedTileRun, WsiDicomError> {
    Ok(PendingMetalEncodedTileRun {
        tile_profiles: (0..tile_count).map(|_| None).collect(),
        submission: j2k_encoder.submit_metal_tiles_owned(
            Vec::new(),
            output_width,
            output_height,
        )?,
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
        input_decode_batches: 0,
        compose_batches: 0,
        encode_batches: 0,
        row_batch_rows,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
        preference: metal_input.preference,
        missing_encode_message:
            "requested JPEG 2000 Metal tile encode did not dispatch all required stages",
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_encode_metal_aligned_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &statumen::Level,
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
) -> Result<MetalEncodedTileRun, WsiDicomError> {
    if !output_tile_maps_to_statumen_tile(level, tile_size) {
        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::Unsupported {
                reason:
                    "requested Metal input tile decode requires the DICOM tile grid to align with statumen source tiles"
                        .into(),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let row_i64 = i64::try_from(row).map_err(|_| WsiDicomError::Unsupported {
        reason: "tile row exceeds i64".into(),
    })?;
    let start_col_i64 = i64::try_from(start_col).map_err(|_| WsiDicomError::Unsupported {
        reason: "tile column exceeds i64".into(),
    })?;
    let mut requests = Vec::with_capacity(tile_count);
    for offset in 0..tile_count {
        let col = start_col_i64
            .checked_add(
                i64::try_from(offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile batch offset exceeds i64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile column overflow".into(),
            })?;
        requests.push(TileRequest {
            scene: scene_idx,
            series: series_idx,
            level: level_idx,
            plane: PlaneSelection { z, c, t },
            col,
            row: row_i64,
        });
    }

    let input_decode_started = Instant::now();
    let pixels = match slide.read_tiles(&requests, metal_input.source_tile_output_preference()?) {
        Ok(pixels) => pixels,
        Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
            return Err(WsiDicomError::SlideRead {
                message: format!("Metal input tile batch decode failed: {err}"),
            });
        }
        Err(_) => return Ok(empty_metal_tile_run(tile_count)),
    };
    let input_decode_duration = input_decode_started.elapsed();

    if pixels.len() != tile_count {
        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::SlideRead {
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
            .checked_add(
                u64::try_from(offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile batch offset exceeds u64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile column overflow".into(),
            })?;
        let x =
            col.checked_mul(u64::from(tile_size))
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "tile x offset overflow".into(),
                })?;
        let y =
            row.checked_mul(u64::from(tile_size))
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "tile y offset overflow".into(),
                })?;
        let width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
        let height = (matrix_rows - y).min(u64::from(tile_size)) as u32;

        let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested Metal input tile decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                            .into(),
                });
            }
            tile_entries.push(None);
            continue;
        };

        if tile.width != width || tile.height != height {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::Unsupported {
                    reason: format!(
                        "Metal input tile geometry changed: expected {}x{}, got {}x{}",
                        width, height, tile.width, tile.height
                    ),
                });
            }
            tile_entries.push(None);
            continue;
        }

        let profile = pixel_profile_from_device_format(tile.format)?;
        tile_entries.push(Some((tile, profile)));
    }

    let batch_tiles: Vec<_> = tile_entries
        .iter()
        .filter_map(|entry| entry.as_ref().map(|(tile, _)| tile.clone()))
        .collect();
    let encode_batches = metal_j2k_encode_batch_count(&batch_tiles, tile_size, tile_size);
    let batch_encoded = j2k_encoder.encode_metal_tiles(&batch_tiles, tile_size, tile_size)?;
    let gpu_encode_stats = batch_encoded.gpu_encode_stats;
    let mut batch_encoded = batch_encoded.frames.into_iter();
    let mut encoded = Vec::with_capacity(tile_count);
    for entry in tile_entries {
        let Some((_tile, profile)) = entry else {
            encoded.push(None);
            continue;
        };
        match batch_encoded
            .next()
            .expect("Metal batch encode result count matches input tile count")
        {
            Some(codestream) => encoded.push(Some((codestream, profile))),
            None if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested JPEG 2000 Metal tile encode did not dispatch all required stages"
                            .into(),
                });
            }
            None => encoded.push(None),
        }
    }

    Ok(MetalEncodedTileRun {
        tiles: encoded,
        input_decode_duration,
        compose_duration: Duration::ZERO,
        input_decode_batches: 1,
        compose_batches: 0,
        encode_batches,
        gpu_encode_stats,
        row_batch_rows: 1,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn try_encode_metal_aligned_tile_grid_run(
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
) -> Result<MetalEncodedTileRun, WsiDicomError> {
    let tile_count =
        tiles_across
            .checked_mul(row_count)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal aligned tile grid batch size overflow".into(),
            })?;
    let start_row_i64 = i64::try_from(start_row).map_err(|_| WsiDicomError::Unsupported {
        reason: "tile row exceeds i64".into(),
    })?;
    let mut requests = Vec::with_capacity(tile_count);
    for row_offset in 0..row_count {
        let row_i64 = start_row_i64
            .checked_add(
                i64::try_from(row_offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile row batch offset exceeds i64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile row overflow".into(),
            })?;
        for col in 0..tiles_across {
            requests.push(TileRequest {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                plane: PlaneSelection { z, c, t },
                col: i64::try_from(col).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile column exceeds i64".into(),
                })?,
                row: row_i64,
            });
        }
    }

    let input_decode_started = Instant::now();
    let pixels = match slide.read_tiles(&requests, metal_input.source_tile_output_preference()?) {
        Ok(pixels) => pixels,
        Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
            return Err(WsiDicomError::SlideRead {
                message: format!("Metal input tile grid decode failed: {err}"),
            });
        }
        Err(_) => return Ok(empty_metal_tile_run(tile_count)),
    };
    let input_decode_duration = input_decode_started.elapsed();

    if pixels.len() != tile_count {
        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::SlideRead {
                message: format!(
                    "Metal input tile grid returned {} tile(s), expected {tile_count}",
                    pixels.len()
                ),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let mut tile_entries = Vec::with_capacity(tile_count);
    for (idx, pixels) in pixels.into_iter().enumerate() {
        let row_offset = idx / tiles_across;
        let col = idx % tiles_across;
        let x = u64::try_from(col)
            .ok()
            .and_then(|col| col.checked_mul(u64::from(tile_size)))
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile x offset overflow".into(),
            })?;
        let y = start_row
            .checked_add(
                u64::try_from(row_offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile row batch offset exceeds u64".into(),
                })?,
            )
            .and_then(|row| row.checked_mul(u64::from(tile_size)))
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile y offset overflow".into(),
            })?;
        let width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
        let height = (matrix_rows - y).min(u64::from(tile_size)) as u32;

        let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested Metal input tile grid decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                            .into(),
                });
            }
            tile_entries.push(None);
            continue;
        };

        if tile.width != width || tile.height != height {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::Unsupported {
                    reason: format!(
                        "Metal input tile grid geometry changed: expected {}x{}, got {}x{}",
                        width, height, tile.width, tile.height
                    ),
                });
            }
            tile_entries.push(None);
            continue;
        }

        let profile = pixel_profile_from_device_format(tile.format)?;
        tile_entries.push(Some((tile, profile)));
    }

    let batch_tiles: Vec<_> = tile_entries
        .iter()
        .filter_map(|entry| entry.as_ref().map(|(tile, _)| tile.clone()))
        .collect();
    let encode_batches = metal_j2k_encode_batch_count(&batch_tiles, tile_size, tile_size);
    let batch_encoded = j2k_encoder.encode_metal_tiles(&batch_tiles, tile_size, tile_size)?;
    let gpu_encode_stats = batch_encoded.gpu_encode_stats;
    let mut batch_encoded = batch_encoded.frames.into_iter();
    let mut encoded = Vec::with_capacity(tile_count);
    for entry in tile_entries {
        let Some((_tile, profile)) = entry else {
            encoded.push(None);
            continue;
        };
        match batch_encoded
            .next()
            .expect("Metal batch encode result count matches input tile count")
        {
            Some(codestream) => encoded.push(Some((codestream, profile))),
            None if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested JPEG 2000 Metal tile grid encode did not dispatch all required stages"
                            .into(),
                });
            }
            None => encoded.push(None),
        }
    }

    Ok(MetalEncodedTileRun {
        tiles: encoded,
        input_decode_duration,
        compose_duration: Duration::ZERO,
        input_decode_batches: 1,
        compose_batches: 0,
        encode_batches,
        gpu_encode_stats,
        row_batch_rows: row_count,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn try_submit_metal_aligned_tile_grid_run(
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
) -> Result<PendingMetalEncodedTileRun, WsiDicomError> {
    let tile_count =
        tiles_across
            .checked_mul(row_count)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal aligned tile grid batch size overflow".into(),
            })?;
    let start_row_i64 = i64::try_from(start_row).map_err(|_| WsiDicomError::Unsupported {
        reason: "tile row exceeds i64".into(),
    })?;
    let mut requests = Vec::with_capacity(tile_count);
    for row_offset in 0..row_count {
        let row_i64 = start_row_i64
            .checked_add(
                i64::try_from(row_offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile row batch offset exceeds i64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile row overflow".into(),
            })?;
        for col in 0..tiles_across {
            requests.push(TileRequest {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                plane: PlaneSelection { z, c, t },
                col: i64::try_from(col).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile column exceeds i64".into(),
                })?,
                row: row_i64,
            });
        }
    }

    let input_decode_started = Instant::now();
    let pixels = match slide.read_tiles(&requests, metal_input.source_tile_output_preference()?) {
        Ok(pixels) => pixels,
        Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
            return Err(WsiDicomError::SlideRead {
                message: format!("Metal input tile grid decode failed: {err}"),
            });
        }
        Err(_) => {
            return empty_pending_metal_tile_run(
                j2k_encoder,
                tile_count,
                tile_size,
                tile_size,
                row_count,
                metal_input,
            );
        }
    };
    let input_decode_duration = input_decode_started.elapsed();

    if pixels.len() != tile_count {
        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::SlideRead {
                message: format!(
                    "Metal input tile grid returned {} tile(s), expected {tile_count}",
                    pixels.len()
                ),
            });
        }
        return empty_pending_metal_tile_run(
            j2k_encoder,
            tile_count,
            tile_size,
            tile_size,
            row_count,
            metal_input,
        );
    }

    let mut tile_entries = Vec::with_capacity(tile_count);
    for (idx, pixels) in pixels.into_iter().enumerate() {
        let row_offset = idx / tiles_across;
        let col = idx % tiles_across;
        let x = u64::try_from(col)
            .ok()
            .and_then(|col| col.checked_mul(u64::from(tile_size)))
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile x offset overflow".into(),
            })?;
        let y = start_row
            .checked_add(
                u64::try_from(row_offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile row batch offset exceeds u64".into(),
                })?,
            )
            .and_then(|row| row.checked_mul(u64::from(tile_size)))
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile y offset overflow".into(),
            })?;
        let width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
        let height = (matrix_rows - y).min(u64::from(tile_size)) as u32;

        let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested Metal input tile grid decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                            .into(),
                });
            }
            tile_entries.push(None);
            continue;
        };

        if tile.width != width || tile.height != height {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::Unsupported {
                    reason: format!(
                        "Metal input tile grid geometry changed: expected {}x{}, got {}x{}",
                        width, height, tile.width, tile.height
                    ),
                });
            }
            tile_entries.push(None);
            continue;
        }

        let profile = pixel_profile_from_device_format(tile.format)?;
        tile_entries.push(Some((tile, profile)));
    }

    let batch_tiles: Vec<_> = tile_entries
        .iter()
        .filter_map(|entry| entry.as_ref().map(|(tile, _)| tile.clone()))
        .collect();
    let tile_profiles = tile_entries
        .into_iter()
        .map(|entry| entry.map(|(_, profile)| profile))
        .collect();
    let encode_batches = metal_j2k_encode_batch_count(&batch_tiles, tile_size, tile_size);
    let submission = j2k_encoder.submit_metal_tiles_owned(batch_tiles, tile_size, tile_size)?;

    Ok(PendingMetalEncodedTileRun {
        tile_profiles,
        submission,
        input_decode_duration,
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

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
pub(super) struct WholeLevelStripLayout {
    pub(super) width: u32,
    pub(super) height: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_encode_metal_whole_level_strip_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    strip_layout: WholeLevelStripLayout,
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
) -> Result<MetalEncodedTileRun, WsiDicomError> {
    let preference = metal_input.preference;
    let tile_size_u64 = u64::from(tile_size);
    let x_start =
        start_col
            .checked_mul(tile_size_u64)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile x offset overflow".into(),
            })?;
    let y = row
        .checked_mul(tile_size_u64)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "tile y offset overflow".into(),
        })?;
    let requested_batch_width = u64::try_from(tile_count)
        .map_err(|_| WsiDicomError::Unsupported {
            reason: "tile batch size exceeds u64".into(),
        })?
        .checked_mul(tile_size_u64)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "tile batch width overflow".into(),
        })?;
    let batch_width = matrix_columns
        .saturating_sub(x_start)
        .min(requested_batch_width);
    let valid_height = (matrix_rows - y).min(tile_size_u64) as u32;
    let source_tile_width = u64::from(strip_layout.width);
    let source_tile_height = u64::from(strip_layout.height);
    let first_source_col = x_start / source_tile_width;
    let first_source_row = y / source_tile_height;
    let source_col_count = x_start
        .checked_add(batch_width)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile x end overflow".into(),
        })?
        .div_ceil(source_tile_width)
        .saturating_sub(first_source_col);
    let source_row_count = y
        .checked_add(u64::from(valid_height))
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile y end overflow".into(),
        })?
        .div_ceil(source_tile_height)
        .saturating_sub(first_source_row);
    let first_source_col_i64 =
        i64::try_from(first_source_col).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile column exceeds i64".into(),
        })?;
    let first_source_row_i64 =
        i64::try_from(first_source_row).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile row exceeds i64".into(),
        })?;
    let source_col_count_usize =
        usize::try_from(source_col_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile column count exceeds platform addressable memory".into(),
        })?;
    let source_row_count_usize =
        usize::try_from(source_row_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile row count exceeds platform addressable memory".into(),
        })?;
    let source_tile_count = source_col_count_usize
        .checked_mul(source_row_count_usize)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile batch size overflow".into(),
        })?;
    let mut source_keys = Vec::with_capacity(source_tile_count);
    for source_row_offset in 0..source_row_count_usize {
        let source_row = first_source_row_i64
            .checked_add(i64::try_from(source_row_offset).map_err(|_| {
                WsiDicomError::Unsupported {
                    reason: "source tile row offset exceeds i64".into(),
                }
            })?)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "source tile row overflow".into(),
            })?;
        for source_col_offset in 0..source_col_count_usize {
            let source_col = first_source_col_i64
                .checked_add(i64::try_from(source_col_offset).map_err(|_| {
                    WsiDicomError::Unsupported {
                        reason: "source tile column offset exceeds i64".into(),
                    }
                })?)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "source tile column overflow".into(),
                })?;
            source_keys.push(MetalSourceTileKey {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                z,
                c,
                t,
                col: source_col,
                row: source_row,
            });
        }
    }

    if source_keys.is_empty() {
        if preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile source batch is empty".into(),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let mut source_tiles = vec![None; source_tile_count];
    let mut missing_requests = Vec::new();
    let mut missing_keys = Vec::new();
    let mut missing_indices = Vec::new();
    for (index, key) in source_keys.iter().copied().enumerate() {
        if let Some(tile) = metal_input.whole_level_cache.get(key) {
            source_tiles[index] = Some(tile);
        } else {
            missing_requests.push(TileRequest {
                scene: key.scene,
                series: key.series,
                level: key.level,
                plane: PlaneSelection {
                    z: key.z,
                    c: key.c,
                    t: key.t,
                },
                col: key.col,
                row: key.row,
            });
            missing_keys.push(key);
            missing_indices.push(index);
        }
    }

    let mut input_decode_duration = Duration::ZERO;
    if !missing_requests.is_empty() {
        let input_decode_started = Instant::now();
        let pixels = match slide.read_tiles(
            &missing_requests,
            metal_input.source_tile_output_preference()?,
        ) {
            Ok(pixels) => pixels,
            Err(err) if preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::SlideRead {
                    message: format!("Metal WholeLevel tile batch decode failed: {err}"),
                });
            }
            Err(_) => return Ok(empty_metal_tile_run(tile_count)),
        };
        input_decode_duration = input_decode_started.elapsed();
        if pixels.len() != missing_requests.len() {
            if preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::SlideRead {
                    message: format!(
                        "Metal WholeLevel tile batch returned {} tile(s), expected {}",
                        pixels.len(),
                        missing_requests.len()
                    ),
                });
            }
            return Ok(empty_metal_tile_run(tile_count));
        }
        for ((index, key), pixels) in missing_indices
            .into_iter()
            .zip(missing_keys.into_iter())
            .zip(pixels.into_iter())
        {
            let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason:
                            "requested Metal WholeLevel tile decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                                .into(),
                    });
                }
                return Ok(empty_metal_tile_run(tile_count));
            };
            if tile.width == 0
                || tile.height == 0
                || tile.width > strip_layout.width
                || tile.height > strip_layout.height
            {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason: format!(
                            "Metal WholeLevel tile geometry changed: expected <= {}x{}, got {}x{}",
                            strip_layout.width, strip_layout.height, tile.width, tile.height
                        ),
                    });
                }
                return Ok(empty_metal_tile_run(tile_count));
            }
            metal_input.whole_level_cache.insert(key, tile.clone());
            source_tiles[index] = Some(tile);
        }
    }
    let source_tiles: Vec<_> = source_tiles
        .into_iter()
        .map(|tile| {
            tile.ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel source tile cache returned incomplete row window".into(),
            })
        })
        .collect::<Result<_, _>>()?;

    let compose_started = Instant::now();
    let composer = metal_input.strip_composer()?;
    let packed = composer.pack_tiles(
        &source_tiles,
        strip_layout,
        first_source_col_i64,
        first_source_row_i64,
        source_col_count_usize,
    )?;
    let profile = pixel_profile_from_device_format(packed.format)?;
    let src_origin_y = u32::try_from(y).map_err(|_| WsiDicomError::Unsupported {
        reason: "Metal WholeLevel tile source y offset exceeds u32".into(),
    })?;
    let mut compose_requests = Vec::with_capacity(tile_count);
    for offset in 0..tile_count {
        let col = start_col
            .checked_add(
                u64::try_from(offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile batch offset exceeds u64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile column overflow".into(),
            })?;
        let x = col
            .checked_mul(tile_size_u64)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile x offset overflow".into(),
            })?;
        let valid_width = (matrix_columns - x).min(tile_size_u64) as u32;
        let src_origin_x = u32::try_from(x).map_err(|_| WsiDicomError::Unsupported {
            reason: "Metal WholeLevel tile source x offset exceeds u32".into(),
        })?;
        compose_requests.push(MetalComposeTileRequest {
            src_origin_x,
            src_origin_y,
            valid_width,
            valid_height,
            output_width: tile_size,
            output_height: tile_size,
        });
    }
    let composed_tiles = composer.compose_tiles(&packed, &compose_requests)?;
    let compose_duration = compose_started.elapsed();

    let mut encoded = Vec::with_capacity(tile_count);
    let encode_batches = metal_j2k_encode_batch_count(&composed_tiles, tile_size, tile_size);
    let batch_encoded = j2k_encoder.encode_metal_tiles(&composed_tiles, tile_size, tile_size)?;
    let gpu_encode_stats = batch_encoded.gpu_encode_stats;
    for frame in batch_encoded.frames {
        match frame {
            Some(codestream) => encoded.push(Some((codestream, profile))),
            None if preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested JPEG 2000 Metal tile encode did not dispatch all required stages"
                            .into(),
                });
            }
            None => encoded.push(None),
        }
    }

    Ok(MetalEncodedTileRun {
        tiles: encoded,
        input_decode_duration,
        compose_duration,
        input_decode_batches: u64::from(input_decode_duration > Duration::ZERO),
        compose_batches: 1,
        encode_batches,
        gpu_encode_stats,
        row_batch_rows: 1,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn try_encode_metal_whole_level_strip_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    strip_layout: WholeLevelStripLayout,
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
) -> Result<MetalEncodedTileRun, WsiDicomError> {
    let preference = metal_input.preference;
    let tile_count =
        tiles_across
            .checked_mul(row_count)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile grid batch size overflow".into(),
            })?;
    let tile_size_u64 = u64::from(tile_size);
    let x_start = 0u64;
    let y_start =
        start_row
            .checked_mul(tile_size_u64)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile y offset overflow".into(),
            })?;
    let requested_batch_width = u64::try_from(tiles_across)
        .map_err(|_| WsiDicomError::Unsupported {
            reason: "tile grid column count exceeds u64".into(),
        })?
        .checked_mul(tile_size_u64)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "tile grid batch width overflow".into(),
        })?;
    let requested_batch_height = u64::try_from(row_count)
        .map_err(|_| WsiDicomError::Unsupported {
            reason: "tile grid row count exceeds u64".into(),
        })?
        .checked_mul(tile_size_u64)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "tile grid batch height overflow".into(),
        })?;
    let batch_width = matrix_columns
        .saturating_sub(x_start)
        .min(requested_batch_width);
    let batch_height = matrix_rows
        .saturating_sub(y_start)
        .min(requested_batch_height);
    let source_tile_width = u64::from(strip_layout.width);
    let source_tile_height = u64::from(strip_layout.height);
    let first_source_col = x_start / source_tile_width;
    let first_source_row = y_start / source_tile_height;
    let source_col_count = x_start
        .checked_add(batch_width)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile x end overflow".into(),
        })?
        .div_ceil(source_tile_width)
        .saturating_sub(first_source_col);
    let source_row_count = y_start
        .checked_add(batch_height)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile y end overflow".into(),
        })?
        .div_ceil(source_tile_height)
        .saturating_sub(first_source_row);
    let first_source_col_i64 =
        i64::try_from(first_source_col).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile column exceeds i64".into(),
        })?;
    let first_source_row_i64 =
        i64::try_from(first_source_row).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile row exceeds i64".into(),
        })?;
    let source_col_count_usize =
        usize::try_from(source_col_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile column count exceeds platform addressable memory".into(),
        })?;
    let source_row_count_usize =
        usize::try_from(source_row_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile row count exceeds platform addressable memory".into(),
        })?;
    let source_tile_count = source_col_count_usize
        .checked_mul(source_row_count_usize)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile batch size overflow".into(),
        })?;
    if source_tile_count == 0 {
        if preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile grid source batch is empty".into(),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let mut source_keys = Vec::with_capacity(source_tile_count);
    for source_row_offset in 0..source_row_count_usize {
        let source_row = first_source_row_i64
            .checked_add(i64::try_from(source_row_offset).map_err(|_| {
                WsiDicomError::Unsupported {
                    reason: "source tile row offset exceeds i64".into(),
                }
            })?)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "source tile row overflow".into(),
            })?;
        for source_col_offset in 0..source_col_count_usize {
            let source_col = first_source_col_i64
                .checked_add(i64::try_from(source_col_offset).map_err(|_| {
                    WsiDicomError::Unsupported {
                        reason: "source tile column offset exceeds i64".into(),
                    }
                })?)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "source tile column overflow".into(),
                })?;
            source_keys.push(MetalSourceTileKey {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                z,
                c,
                t,
                col: source_col,
                row: source_row,
            });
        }
    }

    let mut source_tiles = vec![None; source_tile_count];
    let mut missing_requests = Vec::new();
    let mut missing_keys = Vec::new();
    let mut missing_indices = Vec::new();
    for (index, key) in source_keys.iter().copied().enumerate() {
        if let Some(tile) = metal_input.whole_level_cache.get(key) {
            source_tiles[index] = Some(tile);
        } else {
            missing_requests.push(TileRequest {
                scene: key.scene,
                series: key.series,
                level: key.level,
                plane: PlaneSelection {
                    z: key.z,
                    c: key.c,
                    t: key.t,
                },
                col: key.col,
                row: key.row,
            });
            missing_keys.push(key);
            missing_indices.push(index);
        }
    }

    let mut input_decode_duration = Duration::ZERO;
    if !missing_requests.is_empty() {
        let input_decode_started = Instant::now();
        let pixels = match slide.read_tiles(
            &missing_requests,
            metal_input.source_tile_output_preference()?,
        ) {
            Ok(pixels) => pixels,
            Err(err) if preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::SlideRead {
                    message: format!("Metal WholeLevel tile grid decode failed: {err}"),
                });
            }
            Err(_) => return Ok(empty_metal_tile_run(tile_count)),
        };
        input_decode_duration = input_decode_started.elapsed();
        if pixels.len() != missing_requests.len() {
            if preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::SlideRead {
                    message: format!(
                        "Metal WholeLevel tile grid returned {} source tile(s), expected {}",
                        pixels.len(),
                        missing_requests.len()
                    ),
                });
            }
            return Ok(empty_metal_tile_run(tile_count));
        }
        for ((index, key), pixels) in missing_indices
            .into_iter()
            .zip(missing_keys.into_iter())
            .zip(pixels.into_iter())
        {
            let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason:
                            "requested Metal WholeLevel tile grid decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                                .into(),
                    });
                }
                return Ok(empty_metal_tile_run(tile_count));
            };
            if tile.width == 0
                || tile.height == 0
                || tile.width > strip_layout.width
                || tile.height > strip_layout.height
            {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason: format!(
                            "Metal WholeLevel tile grid geometry changed: expected <= {}x{}, got {}x{}",
                            strip_layout.width, strip_layout.height, tile.width, tile.height
                        ),
                    });
                }
                return Ok(empty_metal_tile_run(tile_count));
            }
            metal_input.whole_level_cache.insert(key, tile.clone());
            source_tiles[index] = Some(tile);
        }
    }
    let source_tiles: Vec<_> = source_tiles
        .into_iter()
        .map(|tile| {
            tile.ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile grid cache returned incomplete source window".into(),
            })
        })
        .collect::<Result<_, _>>()?;

    let compose_started = Instant::now();
    let composer = metal_input.strip_composer()?;
    let packed = composer.pack_tiles(
        &source_tiles,
        strip_layout,
        first_source_col_i64,
        first_source_row_i64,
        source_col_count_usize,
    )?;
    let profile = pixel_profile_from_device_format(packed.format)?;
    let mut compose_requests = Vec::with_capacity(tile_count);
    for row_offset in 0..row_count {
        let output_row = start_row
            .checked_add(
                u64::try_from(row_offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "Metal WholeLevel output row offset exceeds u64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel output row overflow".into(),
            })?;
        let y =
            output_row
                .checked_mul(tile_size_u64)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "tile y offset overflow".into(),
                })?;
        let valid_height = (matrix_rows - y).min(tile_size_u64) as u32;
        let src_origin_y = u32::try_from(y).map_err(|_| WsiDicomError::Unsupported {
            reason: "Metal WholeLevel tile source y offset exceeds u32".into(),
        })?;
        for col in 0..tiles_across {
            let x = u64::try_from(col)
                .map_err(|_| WsiDicomError::Unsupported {
                    reason: "Metal WholeLevel output column exceeds u64".into(),
                })?
                .checked_mul(tile_size_u64)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "tile x offset overflow".into(),
                })?;
            let valid_width = (matrix_columns - x).min(tile_size_u64) as u32;
            let src_origin_x = u32::try_from(x).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile source x offset exceeds u32".into(),
            })?;
            compose_requests.push(MetalComposeTileRequest {
                src_origin_x,
                src_origin_y,
                valid_width,
                valid_height,
                output_width: tile_size,
                output_height: tile_size,
            });
        }
    }
    let composed_tiles = composer.compose_tiles(&packed, &compose_requests)?;
    let compose_duration = compose_started.elapsed();

    let mut encoded = Vec::with_capacity(tile_count);
    let encode_batches = metal_j2k_encode_batch_count(&composed_tiles, tile_size, tile_size);
    let batch_encoded = j2k_encoder.encode_metal_tiles(&composed_tiles, tile_size, tile_size)?;
    let gpu_encode_stats = batch_encoded.gpu_encode_stats;
    for frame in batch_encoded.frames {
        match frame {
            Some(codestream) => encoded.push(Some((codestream, profile))),
            None if preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested JPEG 2000 Metal tile grid encode did not dispatch all required stages"
                            .into(),
                });
            }
            None => encoded.push(None),
        }
    }

    Ok(MetalEncodedTileRun {
        tiles: encoded,
        input_decode_duration,
        compose_duration,
        input_decode_batches: u64::from(input_decode_duration > Duration::ZERO),
        compose_batches: 1,
        encode_batches,
        gpu_encode_stats,
        row_batch_rows: row_count,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn try_submit_metal_whole_level_strip_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    strip_layout: WholeLevelStripLayout,
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
) -> Result<PendingMetalEncodedTileRun, WsiDicomError> {
    let preference = metal_input.preference;
    let tile_count =
        tiles_across
            .checked_mul(row_count)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile grid batch size overflow".into(),
            })?;
    let tile_size_u64 = u64::from(tile_size);
    let x_start = 0u64;
    let y_start =
        start_row
            .checked_mul(tile_size_u64)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile y offset overflow".into(),
            })?;
    let requested_batch_width = u64::try_from(tiles_across)
        .map_err(|_| WsiDicomError::Unsupported {
            reason: "tile grid column count exceeds u64".into(),
        })?
        .checked_mul(tile_size_u64)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "tile grid batch width overflow".into(),
        })?;
    let requested_batch_height = u64::try_from(row_count)
        .map_err(|_| WsiDicomError::Unsupported {
            reason: "tile grid row count exceeds u64".into(),
        })?
        .checked_mul(tile_size_u64)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "tile grid batch height overflow".into(),
        })?;
    let batch_width = matrix_columns
        .saturating_sub(x_start)
        .min(requested_batch_width);
    let batch_height = matrix_rows
        .saturating_sub(y_start)
        .min(requested_batch_height);
    let source_tile_width = u64::from(strip_layout.width);
    let source_tile_height = u64::from(strip_layout.height);
    let first_source_col = x_start / source_tile_width;
    let first_source_row = y_start / source_tile_height;
    let source_col_count = x_start
        .checked_add(batch_width)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile x end overflow".into(),
        })?
        .div_ceil(source_tile_width)
        .saturating_sub(first_source_col);
    let source_row_count = y_start
        .checked_add(batch_height)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile y end overflow".into(),
        })?
        .div_ceil(source_tile_height)
        .saturating_sub(first_source_row);
    let first_source_col_i64 =
        i64::try_from(first_source_col).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile column exceeds i64".into(),
        })?;
    let first_source_row_i64 =
        i64::try_from(first_source_row).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile row exceeds i64".into(),
        })?;
    let source_col_count_usize =
        usize::try_from(source_col_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile column count exceeds platform addressable memory".into(),
        })?;
    let source_row_count_usize =
        usize::try_from(source_row_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile row count exceeds platform addressable memory".into(),
        })?;
    let source_tile_count = source_col_count_usize
        .checked_mul(source_row_count_usize)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile batch size overflow".into(),
        })?;
    if source_tile_count == 0 {
        if preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile grid source batch is empty".into(),
            });
        }
        return empty_pending_metal_tile_run(
            j2k_encoder,
            tile_count,
            tile_size,
            tile_size,
            row_count,
            metal_input,
        );
    }

    let mut source_keys = Vec::with_capacity(source_tile_count);
    for source_row_offset in 0..source_row_count_usize {
        let source_row = first_source_row_i64
            .checked_add(i64::try_from(source_row_offset).map_err(|_| {
                WsiDicomError::Unsupported {
                    reason: "source tile row offset exceeds i64".into(),
                }
            })?)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "source tile row overflow".into(),
            })?;
        for source_col_offset in 0..source_col_count_usize {
            let source_col = first_source_col_i64
                .checked_add(i64::try_from(source_col_offset).map_err(|_| {
                    WsiDicomError::Unsupported {
                        reason: "source tile column offset exceeds i64".into(),
                    }
                })?)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "source tile column overflow".into(),
                })?;
            source_keys.push(MetalSourceTileKey {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                z,
                c,
                t,
                col: source_col,
                row: source_row,
            });
        }
    }

    let mut source_tiles = vec![None; source_tile_count];
    let mut missing_requests = Vec::new();
    let mut missing_keys = Vec::new();
    let mut missing_indices = Vec::new();
    for (index, key) in source_keys.iter().copied().enumerate() {
        if let Some(tile) = metal_input.whole_level_cache.get(key) {
            source_tiles[index] = Some(tile);
        } else {
            missing_requests.push(TileRequest {
                scene: key.scene,
                series: key.series,
                level: key.level,
                plane: PlaneSelection {
                    z: key.z,
                    c: key.c,
                    t: key.t,
                },
                col: key.col,
                row: key.row,
            });
            missing_keys.push(key);
            missing_indices.push(index);
        }
    }

    let mut input_decode_duration = Duration::ZERO;
    if !missing_requests.is_empty() {
        let input_decode_started = Instant::now();
        let pixels = match slide.read_tiles(
            &missing_requests,
            metal_input.source_tile_output_preference()?,
        ) {
            Ok(pixels) => pixels,
            Err(err) if preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::SlideRead {
                    message: format!("Metal WholeLevel tile grid decode failed: {err}"),
                });
            }
            Err(_) => {
                return empty_pending_metal_tile_run(
                    j2k_encoder,
                    tile_count,
                    tile_size,
                    tile_size,
                    row_count,
                    metal_input,
                );
            }
        };
        input_decode_duration = input_decode_started.elapsed();
        if pixels.len() != missing_requests.len() {
            if preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::SlideRead {
                    message: format!(
                        "Metal WholeLevel tile grid returned {} source tile(s), expected {}",
                        pixels.len(),
                        missing_requests.len()
                    ),
                });
            }
            return empty_pending_metal_tile_run(
                j2k_encoder,
                tile_count,
                tile_size,
                tile_size,
                row_count,
                metal_input,
            );
        }
        for ((index, key), pixels) in missing_indices
            .into_iter()
            .zip(missing_keys.into_iter())
            .zip(pixels.into_iter())
        {
            let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason:
                            "requested Metal WholeLevel tile grid decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                                .into(),
                    });
                }
                return empty_pending_metal_tile_run(
                    j2k_encoder,
                    tile_count,
                    tile_size,
                    tile_size,
                    row_count,
                    metal_input,
                );
            };
            if tile.width == 0
                || tile.height == 0
                || tile.width > strip_layout.width
                || tile.height > strip_layout.height
            {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason: format!(
                            "Metal WholeLevel tile grid geometry changed: expected <= {}x{}, got {}x{}",
                            strip_layout.width, strip_layout.height, tile.width, tile.height
                        ),
                    });
                }
                return empty_pending_metal_tile_run(
                    j2k_encoder,
                    tile_count,
                    tile_size,
                    tile_size,
                    row_count,
                    metal_input,
                );
            }
            metal_input.whole_level_cache.insert(key, tile.clone());
            source_tiles[index] = Some(tile);
        }
    }
    let source_tiles: Vec<_> = source_tiles
        .into_iter()
        .map(|tile| {
            tile.ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile grid cache returned incomplete source window".into(),
            })
        })
        .collect::<Result<_, _>>()?;

    let compose_started = Instant::now();
    let composer = metal_input.strip_composer()?;
    let packed = composer.pack_tiles(
        &source_tiles,
        strip_layout,
        first_source_col_i64,
        first_source_row_i64,
        source_col_count_usize,
    )?;
    let profile = pixel_profile_from_device_format(packed.format)?;
    let mut compose_requests = Vec::with_capacity(tile_count);
    for row_offset in 0..row_count {
        let output_row = start_row
            .checked_add(
                u64::try_from(row_offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "Metal WholeLevel output row offset exceeds u64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel output row overflow".into(),
            })?;
        let y =
            output_row
                .checked_mul(tile_size_u64)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "tile y offset overflow".into(),
                })?;
        let valid_height = (matrix_rows - y).min(tile_size_u64) as u32;
        let src_origin_y = u32::try_from(y).map_err(|_| WsiDicomError::Unsupported {
            reason: "Metal WholeLevel tile source y offset exceeds u32".into(),
        })?;
        for col in 0..tiles_across {
            let x = u64::try_from(col)
                .map_err(|_| WsiDicomError::Unsupported {
                    reason: "Metal WholeLevel output column exceeds u64".into(),
                })?
                .checked_mul(tile_size_u64)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "tile x offset overflow".into(),
                })?;
            let valid_width = (matrix_columns - x).min(tile_size_u64) as u32;
            let src_origin_x = u32::try_from(x).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile source x offset exceeds u32".into(),
            })?;
            compose_requests.push(MetalComposeTileRequest {
                src_origin_x,
                src_origin_y,
                valid_width,
                valid_height,
                output_width: tile_size,
                output_height: tile_size,
            });
        }
    }
    let composed_tiles = composer.compose_tiles(&packed, &compose_requests)?;
    let compose_duration = compose_started.elapsed();

    let encode_batches = metal_j2k_encode_batch_count(&composed_tiles, tile_size, tile_size);
    let submission = j2k_encoder.submit_metal_tiles_owned(composed_tiles, tile_size, tile_size)?;

    Ok(PendingMetalEncodedTileRun {
        tile_profiles: (0..tile_count).map(|_| Some(profile)).collect(),
        submission,
        input_decode_duration,
        compose_duration,
        input_decode_batches: u64::from(input_decode_duration > Duration::ZERO),
        compose_batches: 1,
        encode_batches,
        row_batch_rows: row_count,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
        preference,
        missing_encode_message:
            "requested JPEG 2000 Metal tile grid encode did not dispatch all required stages",
    })
}

#[cfg(test)]
mod tests {
    use super::metal_row_batch_rows;

    #[test]
    fn metal_row_batch_rows_prefers_explicit_rows_then_target_tiles_and_caps_remaining_rows() {
        assert_eq!(
            metal_row_batch_rows(0, 10, 5, Some(3), Some(96)).unwrap(),
            3
        );
        assert_eq!(metal_row_batch_rows(0, 10, 20, None, Some(64)).unwrap(), 4);
        assert_eq!(metal_row_batch_rows(8, 10, 20, None, Some(96)).unwrap(), 2);
    }
}
