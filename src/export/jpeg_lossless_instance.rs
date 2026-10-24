//! JPEG Lossless (Process 14, first-order prediction) instance export.
//!
//! Every frame is decoded to samples and re-encoded on the CPU.

use std::time::{Duration, Instant};

use wsi_rs::Slide;

use super::cpu_batch::{frame_batch_len, map_cpu_frames};
use super::frame_region::{FrameRectGrid, FrameRectOverflowReasons, OutputFrameRect};
use super::icc_profile::resolve_icc_profile;
use super::j2k_policy::{lossless_j2k_cpu_row_batch_count, lossless_j2k_use_direct_pixel_data};
use super::jpeg_lossless::encode_jpeg_lossless_frame;
use super::lossless_j2k_cpu::prepare_cpu_input_batch;
use super::lossless_j2k_plan::source_tile_codec;
use super::pending_instance::PendingLosslessInstance;
use super::tile_grid::TileGrid;
use super::{ensure_consistent_pixel_profile, InstanceExportContext};
use crate::coordinate::InstanceCoordinate;
use crate::error::Error;
use crate::lossy::{LossyCompressionAccumulator, LossyCompressionByteCounts};
use crate::options::{NormalizedExportOptions, TransferSyntax};
use crate::report::ExportMetrics;
use crate::request::ExportRequest;
use crate::routing::j2k_route_tile_size;
use crate::writer::{unique_spool_path, BufferedPixelDataSink, PixelDataSink};

pub(super) fn prepare_jpeg_lossless_instance(
    slide: &Slide,
    request: &ExportRequest,
    export: InstanceExportContext<'_>,
) -> Result<PendingLosslessInstance, Error> {
    let pending_export = export;
    let InstanceExportContext {
        options,
        metadata,
        coordinate,
        level,
        per_frame_plan,
        ..
    } = export;
    // The metadata budget preflight sizes every non-Baseline instance with
    // this grid, so JPEG Lossless must use the same one.
    let tile_size = j2k_route_tile_size(
        options.semantics.tile_size,
        options.semantics.transfer_syntax,
        level,
    )?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let grid = TileGrid::square(matrix_columns, matrix_rows, tile_size)?;
    let frame_count = export.frame_count(grid.frame_count_u32()?);
    let context = export.dicom_context(slide, request)?;
    let declared_lossy_compression =
        super::lossy_provenance::source_lossy_history(slide, coordinate)?;
    let mut pixel_data = BufferedPixelDataSink::create(
        unique_spool_path(&context.path),
        frame_count as usize,
        lossless_j2k_use_direct_pixel_data(frame_count, tile_size, rayon::current_num_threads()),
    )?;
    let frame_grid = FrameRectGrid {
        matrix_columns,
        matrix_rows,
        frame_columns: tile_size,
        frame_rows: tile_size,
    };
    let mut metrics = ExportMetrics::default();
    let mut pixel_profile = None;
    let mut source_lossy_compression = LossyCompressionAccumulator::default();

    let mut row = 0;
    while row < grid.tiles_down {
        let row_count = lossless_j2k_cpu_row_batch_count(grid.tiles_across, grid.tiles_down - row);
        let mut frames = grid_frames(frame_grid, grid.tiles_across, row, row_count)?;
        frames.retain(|frame| {
            crate::writer::PerFrameFunctionalGroupsPlan::writes_grid_position(
                per_frame_plan,
                frame.row,
                frame.col,
            )
        });
        for batch in frames.chunks(frame_batch_len(tile_size, tile_size)) {
            let encoded = encode_frame_batch(slide, level, coordinate, batch, tile_size, options)?;
            for frame in encoded {
                metrics.record_cpu_input();
                metrics.record_input_decode_duration(frame.input_decode_duration);
                metrics.record_compose_duration(frame.compose_duration);
                metrics.record_pixel_profile(frame.profile);
                metrics.record_encode_duration(frame.encode_duration);
                metrics.record_validation_duration(frame.validation_duration);
                metrics.record_transcode_route(false, false);
                ensure_consistent_pixel_profile(
                    &mut pixel_profile,
                    frame.profile,
                    "pixel profile changed across frames",
                )?;
                if let Some(source) = &frame.source_lossy_compression {
                    source_lossy_compression.observe(source)?;
                }
                if frame.compressed_source {
                    metrics.record_compressed_source_reencode();
                }
                let write_started = Instant::now();
                pixel_data.push_owned_frame(frame.data)?;
                metrics.record_write_duration(write_started.elapsed());
            }
        }
        row = row
            .checked_add(row_count)
            .ok_or_else(|| Error::Unsupported {
                reason: "JPEG Lossless row batch overflow".into(),
            })?;
    }

    let profile = pixel_profile.ok_or_else(|| Error::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    let icc_profile = resolve_icc_profile(slide, request, metadata, coordinate, level, profile)?;
    // JPEG Lossless adds no lossy stage, so the history is the source's alone.
    let lossy_compression = if declared_lossy_compression.is_empty() {
        source_lossy_compression.into_history()?
    } else {
        declared_lossy_compression
    };

    Ok(PendingLosslessInstance::new(
        pending_export,
        context,
        tile_size,
        matrix_columns,
        matrix_rows,
        frame_count,
        profile,
        pixel_data,
        icc_profile,
        lossy_compression,
        metrics,
    ))
}

#[derive(Debug, Clone, Copy)]
struct GridFrame {
    row: u64,
    col: u64,
    rect: OutputFrameRect,
}

fn grid_frames(
    grid: FrameRectGrid,
    tiles_across: u64,
    start_row: u64,
    row_count: u64,
) -> Result<Vec<GridFrame>, Error> {
    (start_row..start_row.saturating_add(row_count))
        .flat_map(|row| (0..tiles_across).map(move |col| (row, col)))
        .map(|(row, col)| {
            let rect = OutputFrameRect::clamped(
                col,
                row,
                grid,
                FrameRectOverflowReasons {
                    x: "JPEG Lossless tile x offset overflow",
                    y: "JPEG Lossless tile y offset overflow",
                },
            )?;
            Ok(GridFrame { row, col, rect })
        })
        .collect()
}

struct EncodedJpegLosslessFrame {
    data: Vec<u8>,
    profile: crate::tile::PixelProfile,
    source_lossy_compression: Option<LossyCompressionByteCounts>,
    compressed_source: bool,
    input_decode_duration: Duration,
    compose_duration: Duration,
    encode_duration: Duration,
    validation_duration: Duration,
}

fn encode_frame_batch(
    slide: &Slide,
    level: &wsi_rs::Level,
    location: InstanceCoordinate,
    batch: &[GridFrame],
    tile_size: u32,
    options: &NormalizedExportOptions,
) -> Result<Vec<EncodedJpegLosslessFrame>, Error> {
    let pixel_reencode = options.semantics.pixel_reencode;
    let codec_validation = options.execution.codec_validation;
    let rects: Vec<_> = batch.iter().map(|frame| frame.rect).collect();
    let prepared = prepare_cpu_input_batch(
        slide,
        level,
        location,
        &rects,
        tile_size,
        options.resources.max_prepared_frame_bytes,
    )?;
    let work: Vec<_> = batch.iter().zip(&prepared).collect();
    map_cpu_frames(&work, rayon::current_num_threads(), |&(frame, tile)| {
        let frame_error = |error| match error {
            Error::Encode { message } => Error::FrameEncode {
                level: location.level_idx,
                row: frame.row,
                col: frame.col,
                message,
            },
            other => other,
        };
        let source = source_tile_codec(
            slide,
            level,
            location,
            frame.rect,
            grid_index_i64(frame.col, "column")?,
            grid_index_i64(frame.row, "row")?,
        )?;
        let compressed_source = source.as_ref().is_some_and(|source| source.compressed);
        // JPEG Lossless has no compressed-domain route: every compressed
        // source frame is decoded and re-encoded.
        if compressed_source && !pixel_reencode.permits(TransferSyntax::JpegLosslessSv1) {
            return Err(Error::Unsupported {
                reason: format!(
                    "JPEG Lossless frame row={} col={} has compressed source pixels; {}",
                    frame.row,
                    frame.col,
                    pixel_reencode.refusal(TransferSyntax::JpegLosslessSv1),
                ),
            });
        }
        let (data, encode_duration, validation_duration) = encode_jpeg_lossless_frame(
            &tile.bytes,
            tile.profile,
            tile_size,
            tile_size,
            codec_validation,
        )
        .map_err(frame_error)?;
        let source_lossy_compression = source.and_then(|source| source.lossy_compression);
        Ok(EncodedJpegLosslessFrame {
            data,
            profile: tile.profile,
            source_lossy_compression,
            compressed_source,
            input_decode_duration: tile.input_decode_duration,
            compose_duration: tile.compose_duration,
            encode_duration,
            validation_duration,
        })
    })
}

fn grid_index_i64(index: u64, axis: &str) -> Result<i64, Error> {
    i64::try_from(index).map_err(|_| Error::Unsupported {
        reason: format!("JPEG Lossless tile {axis} exceeds i64"),
    })
}
