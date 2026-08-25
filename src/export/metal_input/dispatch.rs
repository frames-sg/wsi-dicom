use std::time::Duration;

use wsi_rs::Slide;

use super::super::metal_route::{
    output_tile_maps_to_wsi_rs_tile, regular_tiled_source_layout, whole_level_strip_layout,
};
use super::super::metal_row_batch::{
    self, try_encode_metal_aligned_tile_run, try_encode_metal_whole_level_strip_run,
};
use super::{MetalEncodedTileRun, MetalInputTileReader, MetalInputTileRunRequest};
use crate::encode::{self, DicomJ2kEncoder};
use crate::error::Error;
use crate::options::EncodeBackendPreference;
use crate::routing::level_is_synthetic_downsample;

pub(in crate::export) fn try_encode_metal_input_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    request: MetalInputTileRunRequest<'_>,
) -> Result<MetalEncodedTileRun, Error> {
    // Long NDPI exports create thousands of autoreleased Metal/ObjC temporaries.
    // Drain them per run so later rows do not encode zero-filled composed buffers.
    objc2::rc::autoreleasepool(|_| {
        let MetalInputTileRunRequest {
            level,
            location,
            row,
            start_col,
            tile_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        } = request;
        let row_run_key = request.row_run_key();

        if !metal_input.enabled() {
            return Ok(empty_metal_tile_run(tile_count));
        }
        if let Some(cached) = metal_input.encoded_row_runs.remove(&row_run_key) {
            return Ok(cached);
        }
        if level_is_synthetic_downsample(
            slide,
            location.scene_idx,
            location.series_idx,
            location.level_idx,
        )? {
            return Ok(empty_metal_tile_run(tile_count));
        }

        if let Some(run) = metal_row_batch::try_encode_metal_input_tile_grid_pipeline_run(
            slide,
            metal_input,
            j2k_encoder,
            metal_row_batch::MetalTileGridRunRequest {
                level,
                location,
                row,
                start_col,
                tile_count,
                matrix_columns,
                matrix_rows,
                tile_size,
                first_row_key: row_run_key,
            },
        )? {
            return Ok(run);
        }

        if output_tile_maps_to_wsi_rs_tile(level, tile_size) {
            return try_encode_metal_aligned_tile_run(slide, metal_input, j2k_encoder, request);
        }

        if let Some(source_layout) = regular_tiled_source_layout(level) {
            return try_encode_metal_whole_level_strip_run(
                slide,
                metal_input,
                j2k_encoder,
                request,
                source_layout,
            );
        }

        if let Some(strip_layout) = whole_level_strip_layout(level) {
            return try_encode_metal_whole_level_strip_run(
                slide,
                metal_input,
                j2k_encoder,
                request,
                strip_layout,
            );
        }

        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(Error::Unsupported {
                reason:
                    "requested Metal input tile decode requires a DICOM tile grid that can be sourced from aligned wsi-rs tiles, regular tiled composition, or WholeLevel strip tiles"
                        .into(),
            });
        }
        Ok(empty_metal_tile_run(tile_count))
    })
}

pub(in crate::export) fn empty_metal_tile_run(tile_count: usize) -> MetalEncodedTileRun {
    MetalEncodedTileRun {
        tiles: (0..tile_count).map(|_| None).collect(),
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
        input_decode_batches: 0,
        compose_batches: 0,
        encode_batches: 0,
        gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats::default(),
        row_batch_rows: 0,
        row_batch_target_tiles: None,
    }
}

pub(in crate::export) fn metal_j2k_encode_batch_count(
    tiles: &[wsi_rs::output::metal::MetalDeviceTile],
    output_width: u32,
    output_height: u32,
) -> u64 {
    let mut batches = 0u64;
    let mut start = 0usize;
    while start < tiles.len() {
        batches = batches.saturating_add(1);
        let padded =
            encode::metal_tile_is_padded_contiguous(&tiles[start], output_width, output_height);
        let mut end = start + 1;
        while end < tiles.len()
            && encode::metal_tile_is_padded_contiguous(&tiles[end], output_width, output_height)
                == padded
        {
            end += 1;
        }
        start = end;
    }
    batches
}
