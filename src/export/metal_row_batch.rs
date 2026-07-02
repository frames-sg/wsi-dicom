use super::*;

mod aligned;
mod whole_level;

pub(super) use aligned::try_encode_metal_aligned_tile_run;
pub(super) use whole_level::{
    try_encode_metal_whole_level_strip_run, WholeLevelStripGridRunRequest, WholeLevelStripLayout,
};

use aligned::{try_encode_metal_aligned_tile_grid_run, try_submit_metal_aligned_tile_grid_run};
use whole_level::{
    try_encode_metal_whole_level_strip_grid_run, try_submit_metal_whole_level_strip_grid_run,
};

type MetalDeviceTile = wsi_rs::output::metal::MetalDeviceTile;
type MetalTileEntry = Option<(MetalDeviceTile, PixelProfile)>;

struct EncodedMetalTileEntries {
    tiles: Vec<Option<(EncodedDicomJ2kFrame, PixelProfile)>>,
    encode_batches: u64,
    gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Clone, Copy)]
pub(super) struct MetalTileGridRunRequest<'a> {
    pub(super) level: &'a wsi_rs::Level,
    pub(super) scene_idx: usize,
    pub(super) series_idx: usize,
    pub(super) level_idx: u32,
    pub(super) z: u32,
    pub(super) c: u32,
    pub(super) t: u32,
    pub(super) row: u64,
    pub(super) start_col: u64,
    pub(super) tile_count: usize,
    pub(super) matrix_columns: u64,
    pub(super) matrix_rows: u64,
    pub(super) tile_size: u32,
    pub(super) first_row_key: MetalEncodedRowRunKey,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalTileGridRunRequest<'_> {
    fn full_row_tiles_across(self) -> Option<u64> {
        let tiles_across = self.matrix_columns.div_ceil(u64::from(self.tile_size));
        (self.start_col == 0 && u64::try_from(self.tile_count).ok() == Some(tiles_across))
            .then_some(tiles_across)
    }

    fn row_batch_rows(self, metal_input: &MetalInputTileReader) -> Result<usize, Error> {
        metal_row_batch_rows(
            self.row,
            self.matrix_rows.div_ceil(u64::from(self.tile_size)),
            self.tile_count,
            metal_input.row_batch_rows,
            metal_input.row_batch_target_tiles,
        )
    }

    fn whole_level_request(
        self,
        strip_layout: WholeLevelStripLayout,
        row_count: usize,
    ) -> WholeLevelStripGridRunRequest {
        WholeLevelStripGridRunRequest {
            strip_layout,
            scene_idx: self.scene_idx,
            series_idx: self.series_idx,
            level_idx: self.level_idx,
            z: self.z,
            c: self.c,
            t: self.t,
            start_row: self.row,
            tiles_across: self.tile_count,
            row_count,
            matrix_columns: self.matrix_columns,
            matrix_rows: self.matrix_rows,
            tile_size: self.tile_size,
        }
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn try_encode_metal_input_tile_grid_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    request: MetalTileGridRunRequest<'_>,
) -> Result<Option<MetalEncodedTileRun>, Error> {
    if request.full_row_tiles_across().is_none() {
        return Ok(None);
    }
    let row_count = request.row_batch_rows(metal_input)?;
    if row_count <= 1 {
        return Ok(None);
    }
    let whole_level_request = |strip_layout| request.whole_level_request(strip_layout, row_count);

    let grid_run = if output_tile_maps_to_wsi_rs_tile(request.level, request.tile_size) {
        try_encode_metal_aligned_tile_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            request.scene_idx,
            request.series_idx,
            request.level_idx,
            request.z,
            request.c,
            request.t,
            request.row,
            request.tile_count,
            row_count,
            request.matrix_columns,
            request.matrix_rows,
            request.tile_size,
        )?
    } else if let Some(source_layout) = regular_tiled_source_layout(request.level) {
        try_encode_metal_whole_level_strip_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            whole_level_request(source_layout),
        )?
    } else if let Some(strip_layout) = whole_level_strip_layout(request.level) {
        try_encode_metal_whole_level_strip_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            whole_level_request(strip_layout),
        )?
    } else {
        return Ok(None);
    };

    Ok(Some(cache_split_metal_grid_run(
        metal_input,
        request.first_row_key,
        grid_run,
        request.tile_count,
        row_count,
    )?))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn try_encode_metal_input_tile_grid_pipeline_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    request: MetalTileGridRunRequest<'_>,
) -> Result<Option<MetalEncodedTileRun>, Error> {
    if request.full_row_tiles_across().is_none() {
        return Ok(None);
    }
    if metal_input.pipeline_depth <= 1 {
        return try_encode_metal_input_tile_grid_run(slide, metal_input, j2k_encoder, request);
    }

    let tiles_down = request.matrix_rows.div_ceil(u64::from(request.tile_size));
    if metal_input
        .next_grid_pipeline_row
        .is_none_or(|next| next < request.row)
    {
        metal_input.next_grid_pipeline_row = Some(request.row);
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
            request.tile_count,
            metal_input.row_batch_rows,
            metal_input.row_batch_target_tiles,
        )?;
        if row_count <= 1 {
            break;
        }
        let submit_key = MetalEncodedRowRunKey {
            scene: request.scene_idx,
            series: request.series_idx,
            level: request.level_idx,
            z: request.z,
            c: request.c,
            t: request.t,
            row: submit_row,
            start_col: request.start_col,
            tile_count: request.tile_count,
            matrix_columns: request.matrix_columns,
            matrix_rows: request.matrix_rows,
            tile_size: request.tile_size,
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
            request.level,
            request.scene_idx,
            request.series_idx,
            request.level_idx,
            request.z,
            request.c,
            request.t,
            submit_row,
            request.tile_count,
            row_count,
            request.matrix_columns,
            request.matrix_rows,
            request.tile_size,
        )?
        else {
            break;
        };
        metal_input.pending_encoded_grid_runs.insert(
            submit_key,
            PendingMetalEncodedGridRun {
                run,
                first_row_key: submit_key,
                tiles_per_row: request.tile_count,
                row_count,
            },
        );
        metal_input.next_grid_pipeline_row =
            Some(next_metal_grid_pipeline_row(submit_row, row_count)?);
    }

    let Some(pending) = metal_input
        .pending_encoded_grid_runs
        .remove(&request.first_row_key)
    else {
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
fn next_metal_grid_pipeline_row(row: u64, row_count: usize) -> Result<u64, Error> {
    row.checked_add(u64::try_from(row_count).map_err(|_| Error::Unsupported {
        reason: "Metal row batch pipeline row count exceeds u64".into(),
    })?)
    .ok_or_else(|| Error::Unsupported {
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
) -> Result<usize, Error> {
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
    let remaining_rows = usize::try_from(tiles_down - row).map_err(|_| Error::Unsupported {
        reason: "remaining tile rows exceed platform addressable memory".into(),
    })?;
    Ok(requested.min(remaining_rows))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn configured_metal_row_batch_rows() -> Result<Option<usize>, Error> {
    let value = match std::env::var(WSI_DICOM_METAL_ROW_BATCH_ROWS_ENV) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Err(err) => {
            return Err(Error::InvalidOptions {
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
        .map_err(|_| Error::InvalidOptions {
            reason: format!("{WSI_DICOM_METAL_ROW_BATCH_ROWS_ENV} must be a positive integer"),
        })?;
    if rows == 0 {
        return Err(Error::InvalidOptions {
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
) -> Result<MetalEncodedTileRun, Error> {
    let expected_tiles =
        tiles_per_row
            .checked_mul(row_count)
            .ok_or_else(|| Error::Unsupported {
                reason: "Metal row batch tile count overflow".into(),
            })?;
    if grid_run.tiles.len() != expected_tiles {
        return Err(Error::Encode {
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
            .checked_add(u64::try_from(offset + 1).map_err(|_| Error::Unsupported {
                reason: "Metal row batch cache offset exceeds u64".into(),
            })?)
            .ok_or_else(|| Error::Unsupported {
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
    level: &wsi_rs::Level,
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
) -> Result<Option<PendingMetalEncodedTileRun>, Error> {
    if output_tile_maps_to_wsi_rs_tile(level, tile_size) {
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
    let whole_level_request = |strip_layout| WholeLevelStripGridRunRequest {
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
    };
    if let Some(source_layout) = regular_tiled_source_layout(level) {
        return try_submit_metal_whole_level_strip_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            whole_level_request(source_layout),
        )
        .map(Some);
    }
    if let Some(strip_layout) = whole_level_strip_layout(level) {
        return try_submit_metal_whole_level_strip_grid_run(
            slide,
            metal_input,
            j2k_encoder,
            whole_level_request(strip_layout),
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
) -> Result<PendingMetalEncodedTileRun, Error> {
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
fn encode_metal_tile_entries(
    j2k_encoder: &mut DicomJ2kEncoder,
    tile_entries: Vec<MetalTileEntry>,
    tile_width: u32,
    tile_height: u32,
    preference: EncodeBackendPreference,
    missing_encode_message: &'static str,
) -> Result<EncodedMetalTileEntries, Error> {
    let (batch_tiles, tile_profiles) = split_metal_tile_entries(tile_entries);
    let encode_batches = metal_j2k_encode_batch_count(&batch_tiles, tile_width, tile_height);
    let batch_encoded = j2k_encoder.encode_metal_tiles(&batch_tiles, tile_width, tile_height)?;
    let gpu_encode_stats = batch_encoded.gpu_encode_stats;
    let tiles = merge_metal_tile_batch_frames(
        tile_profiles,
        batch_encoded.frames,
        preference,
        missing_encode_message,
    )?;
    Ok(EncodedMetalTileEntries {
        tiles,
        encode_batches,
        gpu_encode_stats,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn split_metal_tile_entries(
    tile_entries: Vec<MetalTileEntry>,
) -> (Vec<MetalDeviceTile>, Vec<Option<PixelProfile>>) {
    let mut batch_tiles = Vec::new();
    let mut tile_profiles = Vec::with_capacity(tile_entries.len());
    for entry in tile_entries {
        if let Some((tile, profile)) = entry {
            batch_tiles.push(tile);
            tile_profiles.push(Some(profile));
        } else {
            tile_profiles.push(None);
        }
    }
    (batch_tiles, tile_profiles)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn merge_metal_tile_batch_frames(
    tile_profiles: Vec<Option<PixelProfile>>,
    frames: Vec<Option<EncodedDicomJ2kFrame>>,
    preference: EncodeBackendPreference,
    missing_encode_message: &'static str,
) -> Result<Vec<Option<(EncodedDicomJ2kFrame, PixelProfile)>>, Error> {
    let mut frames = frames.into_iter();
    let mut encoded = Vec::with_capacity(tile_profiles.len());
    for profile in tile_profiles {
        let Some(profile) = profile else {
            encoded.push(None);
            continue;
        };
        let Some(encoded_frame) = frames.next() else {
            return Err(Error::Encode {
                message: "Metal batch encode result count did not match input tile count".into(),
            });
        };
        match encoded_frame {
            Some(codestream) => encoded.push(Some((codestream, profile))),
            None if preference == EncodeBackendPreference::RequireDevice => {
                return Err(Error::Unsupported {
                    reason: missing_encode_message.into(),
                });
            }
            None => encoded.push(None),
        }
    }
    Ok(encoded)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn cache_and_store_whole_level_source_tile(
    metal_input: &mut MetalInputTileReader,
    source_tiles: &mut [Option<MetalDeviceTile>],
    index: usize,
    key: MetalSourceTileKey,
    tile: MetalDeviceTile,
) {
    metal_input.whole_level_cache.insert(key, tile.clone());
    source_tiles[index] = Some(tile);
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
