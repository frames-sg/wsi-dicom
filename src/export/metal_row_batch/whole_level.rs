use super::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
pub(in crate::export) struct WholeLevelStripLayout {
    pub(in crate::export) width: u32,
    pub(in crate::export) height: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
pub(in crate::export) struct WholeLevelStripGridRunRequest {
    pub(in crate::export) strip_layout: WholeLevelStripLayout,
    pub(in crate::export) scene_idx: usize,
    pub(in crate::export) series_idx: usize,
    pub(in crate::export) level_idx: u32,
    pub(in crate::export) z: u32,
    pub(in crate::export) c: u32,
    pub(in crate::export) t: u32,
    pub(in crate::export) start_row: u64,
    pub(in crate::export) tiles_across: usize,
    pub(in crate::export) row_count: usize,
    pub(in crate::export) matrix_columns: u64,
    pub(in crate::export) matrix_rows: u64,
    pub(in crate::export) tile_size: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct WholeLevelSourceWindow {
    first_col: i64,
    first_row: i64,
    col_count: usize,
    tile_count: usize,
    keys: Vec<MetalSourceTileKey>,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct WholeLevelSourceRead {
    tiles: Option<Vec<MetalDeviceTile>>,
    input_decode_duration: Duration,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
enum WholeLevelGridPreparedRun {
    Empty(WholeLevelGridEmptyRun),
    Composed(WholeLevelGridComposedRun),
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct WholeLevelGridEmptyRun {
    tile_count: usize,
    row_count: usize,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct WholeLevelGridComposedRun {
    tile_count: usize,
    row_count: usize,
    profile: PixelProfile,
    composed_tiles: Vec<MetalDeviceTile>,
    input_decode_duration: Duration,
    compose_duration: Duration,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl WholeLevelGridPreparedRun {
    fn into_composed(self) -> Result<WholeLevelGridComposedRun, WholeLevelGridEmptyRun> {
        match self {
            Self::Empty(empty) => Err(empty),
            Self::Composed(composed) => Ok(composed),
        }
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct WholeLevelSourceReadMessages {
    decode_subject: &'static str,
    returned_unit: &'static str,
    cpu_pixels_reason: &'static str,
    geometry_subject: &'static str,
    incomplete_cache_reason: &'static str,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
const WHOLE_LEVEL_ROW_SOURCE_READ_MESSAGES: WholeLevelSourceReadMessages =
    WholeLevelSourceReadMessages {
        decode_subject: "Metal WholeLevel tile batch",
        returned_unit: "tile(s)",
        cpu_pixels_reason:
            "requested Metal WholeLevel tile decode returned CPU pixels; set WSI_RS_JPEG_DEVICE_DECODE=1 or WSI_RS_JP2K_DEVICE_DECODE=1 for compressed WSI tiles",
        geometry_subject: "Metal WholeLevel tile",
        incomplete_cache_reason: "Metal WholeLevel source tile cache returned incomplete row window",
    };

#[cfg(all(feature = "metal", target_os = "macos"))]
const WHOLE_LEVEL_GRID_SOURCE_READ_MESSAGES: WholeLevelSourceReadMessages =
    WholeLevelSourceReadMessages {
        decode_subject: "Metal WholeLevel tile grid",
        returned_unit: "source tile(s)",
        cpu_pixels_reason:
            "requested Metal WholeLevel tile grid decode returned CPU pixels; set WSI_RS_JPEG_DEVICE_DECODE=1 or WSI_RS_JP2K_DEVICE_DECODE=1 for compressed WSI tiles",
        geometry_subject: "Metal WholeLevel tile grid",
        incomplete_cache_reason: "Metal WholeLevel tile grid cache returned incomplete source window",
    };

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn whole_level_source_window(
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    first_source_col: u64,
    first_source_row: u64,
    source_col_count: u64,
    source_row_count: u64,
) -> Result<WholeLevelSourceWindow, Error> {
    let first_col = i64::try_from(first_source_col).map_err(|_| Error::Unsupported {
        reason: "source tile column exceeds i64".into(),
    })?;
    let first_row = i64::try_from(first_source_row).map_err(|_| Error::Unsupported {
        reason: "source tile row exceeds i64".into(),
    })?;
    let col_count = usize::try_from(source_col_count).map_err(|_| Error::Unsupported {
        reason: "source tile column count exceeds platform addressable memory".into(),
    })?;
    let row_count = usize::try_from(source_row_count).map_err(|_| Error::Unsupported {
        reason: "source tile row count exceeds platform addressable memory".into(),
    })?;
    let tile_count = col_count
        .checked_mul(row_count)
        .ok_or_else(|| Error::Unsupported {
            reason: "source tile batch size overflow".into(),
        })?;

    let mut keys = Vec::with_capacity(tile_count);
    for source_row_offset in 0..row_count {
        let source_row = first_row
            .checked_add(
                i64::try_from(source_row_offset).map_err(|_| Error::Unsupported {
                    reason: "source tile row offset exceeds i64".into(),
                })?,
            )
            .ok_or_else(|| Error::Unsupported {
                reason: "source tile row overflow".into(),
            })?;
        for source_col_offset in 0..col_count {
            let source_col = first_col
                .checked_add(
                    i64::try_from(source_col_offset).map_err(|_| Error::Unsupported {
                        reason: "source tile column offset exceeds i64".into(),
                    })?,
                )
                .ok_or_else(|| Error::Unsupported {
                    reason: "source tile column overflow".into(),
                })?;
            keys.push(MetalSourceTileKey {
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

    Ok(WholeLevelSourceWindow {
        first_col,
        first_row,
        col_count,
        tile_count,
        keys,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn whole_level_source_tile_request(key: MetalSourceTileKey) -> TileRequest {
    TileRequest::new(key.scene, key.series, key.level, key.col, key.row)
        .with_plane(PlaneSelection::new(key.z, key.c, key.t))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn read_whole_level_source_tiles(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    strip_layout: WholeLevelStripLayout,
    source_window: &WholeLevelSourceWindow,
    preference: EncodeBackendPreference,
    messages: &WholeLevelSourceReadMessages,
) -> Result<WholeLevelSourceRead, Error> {
    let mut source_tiles = vec![None; source_window.tile_count];
    let mut missing_requests = Vec::new();
    let mut missing_keys = Vec::new();
    let mut missing_indices = Vec::new();
    for (index, key) in source_window.keys.iter().copied().enumerate() {
        if let Some(tile) = metal_input.whole_level_cache.get(key) {
            source_tiles[index] = Some(tile);
        } else {
            missing_requests.push(whole_level_source_tile_request(key));
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
                return Err(Error::SlideRead {
                    message: format!("{} decode failed: {err}", messages.decode_subject),
                });
            }
            Err(_) => {
                return Ok(WholeLevelSourceRead {
                    tiles: None,
                    input_decode_duration: Duration::ZERO,
                });
            }
        };
        input_decode_duration = input_decode_started.elapsed();
        if pixels.len() != missing_requests.len() {
            if preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::SlideRead {
                    message: format!(
                        "{} returned {} {}, expected {}",
                        messages.decode_subject,
                        pixels.len(),
                        messages.returned_unit,
                        missing_requests.len()
                    ),
                });
            }
            return Ok(WholeLevelSourceRead {
                tiles: None,
                input_decode_duration: Duration::ZERO,
            });
        }
        for ((index, key), pixels) in missing_indices.into_iter().zip(missing_keys).zip(pixels) {
            let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(Error::Unsupported {
                        reason: messages.cpu_pixels_reason.into(),
                    });
                }
                return Ok(WholeLevelSourceRead {
                    tiles: None,
                    input_decode_duration: Duration::ZERO,
                });
            };
            if tile.width == 0
                || tile.height == 0
                || tile.width > strip_layout.width
                || tile.height > strip_layout.height
            {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(Error::Unsupported {
                        reason: format!(
                            "{} geometry changed: expected <= {}x{}, got {}x{}",
                            messages.geometry_subject,
                            strip_layout.width,
                            strip_layout.height,
                            tile.width,
                            tile.height
                        ),
                    });
                }
                return Ok(WholeLevelSourceRead {
                    tiles: None,
                    input_decode_duration: Duration::ZERO,
                });
            }
            cache_and_store_whole_level_source_tile(
                metal_input,
                &mut source_tiles,
                index,
                key,
                tile,
            );
        }
    }

    let tiles = source_tiles
        .into_iter()
        .map(|tile| {
            tile.ok_or_else(|| Error::Unsupported {
                reason: messages.incomplete_cache_reason.into(),
            })
        })
        .collect::<Result<_, _>>()?;

    Ok(WholeLevelSourceRead {
        tiles: Some(tiles),
        input_decode_duration,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn whole_level_grid_compose_requests(
    start_row: u64,
    tiles_across: usize,
    row_count: usize,
    tile_count: usize,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
) -> Result<Vec<MetalComposeTileRequest>, Error> {
    let tile_size_u64 = u64::from(tile_size);
    let mut compose_requests = Vec::with_capacity(tile_count);
    for row_offset in 0..row_count {
        let output_row = start_row
            .checked_add(u64::try_from(row_offset).map_err(|_| Error::Unsupported {
                reason: "Metal WholeLevel output row offset exceeds u64".into(),
            })?)
            .ok_or_else(|| Error::Unsupported {
                reason: "Metal WholeLevel output row overflow".into(),
            })?;
        let y = output_row
            .checked_mul(tile_size_u64)
            .ok_or_else(|| Error::Unsupported {
                reason: "tile y offset overflow".into(),
            })?;
        let valid_height = (matrix_rows - y).min(tile_size_u64) as u32;
        let src_origin_y = u32::try_from(y).map_err(|_| Error::Unsupported {
            reason: "Metal WholeLevel tile source y offset exceeds u32".into(),
        })?;
        for col in 0..tiles_across {
            let x = u64::try_from(col)
                .map_err(|_| Error::Unsupported {
                    reason: "Metal WholeLevel output column exceeds u64".into(),
                })?
                .checked_mul(tile_size_u64)
                .ok_or_else(|| Error::Unsupported {
                    reason: "tile x offset overflow".into(),
                })?;
            let valid_width = (matrix_columns - x).min(tile_size_u64) as u32;
            let src_origin_x = u32::try_from(x).map_err(|_| Error::Unsupported {
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
    Ok(compose_requests)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(in crate::export) fn try_encode_metal_whole_level_strip_run(
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
) -> Result<MetalEncodedTileRun, Error> {
    let preference = metal_input.preference;
    let tile_size_u64 = u64::from(tile_size);
    let x_start = start_col
        .checked_mul(tile_size_u64)
        .ok_or_else(|| Error::Unsupported {
            reason: "tile x offset overflow".into(),
        })?;
    let y = row
        .checked_mul(tile_size_u64)
        .ok_or_else(|| Error::Unsupported {
            reason: "tile y offset overflow".into(),
        })?;
    let requested_batch_width = u64::try_from(tile_count)
        .map_err(|_| Error::Unsupported {
            reason: "tile batch size exceeds u64".into(),
        })?
        .checked_mul(tile_size_u64)
        .ok_or_else(|| Error::Unsupported {
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
        .ok_or_else(|| Error::Unsupported {
            reason: "source tile x end overflow".into(),
        })?
        .div_ceil(source_tile_width)
        .saturating_sub(first_source_col);
    let source_row_count = y
        .checked_add(u64::from(valid_height))
        .ok_or_else(|| Error::Unsupported {
            reason: "source tile y end overflow".into(),
        })?
        .div_ceil(source_tile_height)
        .saturating_sub(first_source_row);
    let source_window = whole_level_source_window(
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
        first_source_col,
        first_source_row,
        source_col_count,
        source_row_count,
    )?;
    if source_window.keys.is_empty() {
        if preference == EncodeBackendPreference::RequireDevice {
            return Err(Error::Unsupported {
                reason: "Metal WholeLevel tile source batch is empty".into(),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let source_read = read_whole_level_source_tiles(
        slide,
        metal_input,
        strip_layout,
        &source_window,
        preference,
        &WHOLE_LEVEL_ROW_SOURCE_READ_MESSAGES,
    )?;
    let Some(source_tiles) = source_read.tiles else {
        return Ok(empty_metal_tile_run(tile_count));
    };
    let input_decode_duration = source_read.input_decode_duration;

    let compose_started = Instant::now();
    let composer = metal_input.strip_composer()?;
    let packed = composer.pack_tiles(
        &source_tiles,
        strip_layout,
        source_window.first_col,
        source_window.first_row,
        source_window.col_count,
    )?;
    let profile = pixel_profile_from_device_format(packed.format)?;
    let src_origin_y = u32::try_from(y).map_err(|_| Error::Unsupported {
        reason: "Metal WholeLevel tile source y offset exceeds u32".into(),
    })?;
    let mut compose_requests = Vec::with_capacity(tile_count);
    for offset in 0..tile_count {
        let col = start_col
            .checked_add(u64::try_from(offset).map_err(|_| Error::Unsupported {
                reason: "tile batch offset exceeds u64".into(),
            })?)
            .ok_or_else(|| Error::Unsupported {
                reason: "tile column overflow".into(),
            })?;
        let x = col
            .checked_mul(tile_size_u64)
            .ok_or_else(|| Error::Unsupported {
                reason: "tile x offset overflow".into(),
            })?;
        let valid_width = (matrix_columns - x).min(tile_size_u64) as u32;
        let src_origin_x = u32::try_from(x).map_err(|_| Error::Unsupported {
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
                return Err(Error::Unsupported {
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
fn prepare_metal_whole_level_strip_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    preference: EncodeBackendPreference,
    request: WholeLevelStripGridRunRequest,
) -> Result<WholeLevelGridPreparedRun, Error> {
    let WholeLevelStripGridRunRequest {
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
    } = request;
    let tile_count = tiles_across
        .checked_mul(row_count)
        .ok_or_else(|| Error::Unsupported {
            reason: "Metal WholeLevel tile grid batch size overflow".into(),
        })?;
    let tile_size_u64 = u64::from(tile_size);
    let x_start = 0u64;
    let y_start = start_row
        .checked_mul(tile_size_u64)
        .ok_or_else(|| Error::Unsupported {
            reason: "tile y offset overflow".into(),
        })?;
    let requested_batch_width = u64::try_from(tiles_across)
        .map_err(|_| Error::Unsupported {
            reason: "tile grid column count exceeds u64".into(),
        })?
        .checked_mul(tile_size_u64)
        .ok_or_else(|| Error::Unsupported {
            reason: "tile grid batch width overflow".into(),
        })?;
    let requested_batch_height = u64::try_from(row_count)
        .map_err(|_| Error::Unsupported {
            reason: "tile grid row count exceeds u64".into(),
        })?
        .checked_mul(tile_size_u64)
        .ok_or_else(|| Error::Unsupported {
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
        .ok_or_else(|| Error::Unsupported {
            reason: "source tile x end overflow".into(),
        })?
        .div_ceil(source_tile_width)
        .saturating_sub(first_source_col);
    let source_row_count = y_start
        .checked_add(batch_height)
        .ok_or_else(|| Error::Unsupported {
            reason: "source tile y end overflow".into(),
        })?
        .div_ceil(source_tile_height)
        .saturating_sub(first_source_row);
    let source_window = whole_level_source_window(
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
        first_source_col,
        first_source_row,
        source_col_count,
        source_row_count,
    )?;
    if source_window.tile_count == 0 {
        if preference == EncodeBackendPreference::RequireDevice {
            return Err(Error::Unsupported {
                reason: "Metal WholeLevel tile grid source batch is empty".into(),
            });
        }
        return Ok(WholeLevelGridPreparedRun::Empty(WholeLevelGridEmptyRun {
            tile_count,
            row_count,
        }));
    }

    let source_read = read_whole_level_source_tiles(
        slide,
        metal_input,
        strip_layout,
        &source_window,
        preference,
        &WHOLE_LEVEL_GRID_SOURCE_READ_MESSAGES,
    )?;
    let Some(source_tiles) = source_read.tiles else {
        return Ok(WholeLevelGridPreparedRun::Empty(WholeLevelGridEmptyRun {
            tile_count,
            row_count,
        }));
    };
    let input_decode_duration = source_read.input_decode_duration;

    let compose_started = Instant::now();
    let composer = metal_input.strip_composer()?;
    let packed = composer.pack_tiles(
        &source_tiles,
        strip_layout,
        source_window.first_col,
        source_window.first_row,
        source_window.col_count,
    )?;
    let profile = pixel_profile_from_device_format(packed.format)?;
    let compose_requests = whole_level_grid_compose_requests(
        start_row,
        tiles_across,
        row_count,
        tile_count,
        matrix_columns,
        matrix_rows,
        tile_size,
    )?;
    let composed_tiles = composer.compose_tiles(&packed, &compose_requests)?;
    let compose_duration = compose_started.elapsed();

    Ok(WholeLevelGridPreparedRun::Composed(
        WholeLevelGridComposedRun {
            tile_count,
            row_count,
            profile,
            composed_tiles,
            input_decode_duration,
            compose_duration,
        },
    ))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_encode_metal_whole_level_strip_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    request: WholeLevelStripGridRunRequest,
) -> Result<MetalEncodedTileRun, Error> {
    let preference = metal_input.preference;
    let tile_size = request.tile_size;
    let prepared =
        prepare_metal_whole_level_strip_grid_run(slide, metal_input, preference, request)?;
    let composed = match prepared.into_composed() {
        Ok(composed) => composed,
        Err(empty) => return Ok(empty_metal_tile_run(empty.tile_count)),
    };

    let mut encoded = Vec::with_capacity(composed.tile_count);
    let encode_batches =
        metal_j2k_encode_batch_count(&composed.composed_tiles, tile_size, tile_size);
    let batch_encoded =
        j2k_encoder.encode_metal_tiles(&composed.composed_tiles, tile_size, tile_size)?;
    let gpu_encode_stats = batch_encoded.gpu_encode_stats;
    for frame in batch_encoded.frames {
        match frame {
            Some(codestream) => encoded.push(Some((codestream, composed.profile))),
            None if preference == EncodeBackendPreference::RequireDevice => {
                return Err(Error::Unsupported {
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
        input_decode_duration: composed.input_decode_duration,
        compose_duration: composed.compose_duration,
        input_decode_batches: u64::from(composed.input_decode_duration > Duration::ZERO),
        compose_batches: 1,
        encode_batches,
        gpu_encode_stats,
        row_batch_rows: composed.row_count,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_submit_metal_whole_level_strip_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    request: WholeLevelStripGridRunRequest,
) -> Result<PendingMetalEncodedTileRun, Error> {
    let preference = metal_input.preference;
    let tile_size = request.tile_size;
    let prepared =
        prepare_metal_whole_level_strip_grid_run(slide, metal_input, preference, request)?;
    let composed = match prepared.into_composed() {
        Ok(composed) => composed,
        Err(empty) => {
            return empty_pending_metal_tile_run(
                j2k_encoder,
                empty.tile_count,
                tile_size,
                tile_size,
                empty.row_count,
                metal_input,
            );
        }
    };

    let encode_batches =
        metal_j2k_encode_batch_count(&composed.composed_tiles, tile_size, tile_size);
    let submission =
        j2k_encoder.submit_metal_tiles_owned(composed.composed_tiles, tile_size, tile_size)?;

    Ok(PendingMetalEncodedTileRun {
        tile_profiles: (0..composed.tile_count)
            .map(|_| Some(composed.profile))
            .collect(),
        submission,
        input_decode_duration: composed.input_decode_duration,
        compose_duration: composed.compose_duration,
        input_decode_batches: u64::from(composed.input_decode_duration > Duration::ZERO),
        compose_batches: 1,
        encode_batches,
        row_batch_rows: composed.row_count,
        row_batch_target_tiles: metal_input.row_batch_target_tiles,
        preference,
        missing_encode_message:
            "requested JPEG 2000 Metal tile grid encode did not dispatch all required stages",
    })
}
