//! Preserve CPU-reference reconstruction of irreversible JPEG 2000 sources.

use super::super::cpu_batch::frame_batch_len;
use super::super::frame_region::OutputFrameRect;
use super::super::lossless_j2k_cpu::{
    lossless_j2k_samples_from_prepared_region, prepare_cpu_input_batch,
};
use super::{
    empty_metal_tile_run, MetalEncodedTileRun, MetalInputTileReader, MetalInputTileRunRequest,
};
use crate::encode::DicomJ2kEncoder;
use crate::{EncodeBackendPreference, Error};
use wsi_rs::{Compression, Slide, TileLayout};

pub(super) fn try_encode_canonical_source(
    slide: &Slide,
    reader: &mut MetalInputTileReader,
    encoder: &mut DicomJ2kEncoder,
    request: MetalInputTileRunRequest<'_>,
) -> Result<Option<MetalEncodedTileRun>, Error> {
    let TileLayout::Regular {
        tile_width,
        tile_height,
        ..
    } = request.level.tile_layout
    else {
        return Ok(None);
    };
    if tile_width == 0 || tile_height == 0 || request.tile_count == 0 {
        return Ok(None);
    }
    let x = request.start_col.checked_mul(u64::from(request.tile_size));
    let y = request.row.checked_mul(u64::from(request.tile_size));
    let (Some(x), Some(y)) = (x, y) else {
        return Err(Error::Unsupported {
            reason: "canonical source origin overflow".into(),
        });
    };
    if x >= request.matrix_columns || y >= request.matrix_rows || request.tile_size == 0 {
        return Err(Error::Unsupported {
            reason: "canonical source request is outside the pixel matrix".into(),
        });
    }
    let x_end = (request.tile_count as u64)
        .checked_mul(u64::from(request.tile_size))
        .and_then(|width| x.checked_add(width))
        .ok_or_else(|| Error::Unsupported {
            reason: "canonical source span overflow".into(),
        })?
        .min(request.matrix_columns);
    let y_end = y
        .checked_add(u64::from(request.tile_size))
        .ok_or_else(|| Error::Unsupported {
            reason: "canonical source row overflow".into(),
        })?
        .min(request.matrix_rows);
    let mut canonical = false;
    for row in y / u64::from(tile_height)..y_end.div_ceil(u64::from(tile_height)) {
        for col in x / u64::from(tile_width)..x_end.div_ceil(u64::from(tile_width)) {
            let source_col = i64::try_from(col).map_err(|_| Error::Unsupported {
                reason: "canonical source column exceeds i64".into(),
            })?;
            let source_row = i64::try_from(row).map_err(|_| Error::Unsupported {
                reason: "canonical source row exceeds i64".into(),
            })?;
            let raw = match slide
                .read_raw_compressed_tile(&request.location.tile_request(source_col, source_row))
            {
                Ok(raw) => raw,
                Err(_) => continue, // The ordinary source reader reports decode failures.
            };
            if !matches!(
                raw.compression(),
                Compression::Jp2kRgb | Compression::Jp2kYcbcr
            ) {
                return Ok(None);
            }
            // A following row can use a different irreversible coding style.
            // Do not prefetch it before its own source-policy inspection.
            let end = request.row.saturating_add(1).saturating_mul(
                request
                    .matrix_columns
                    .div_ceil(u64::from(request.tile_size)),
            );
            reader.frame_limit = Some(reader.frame_limit.map_or(end, |limit| limit.min(end)));
            let lossless = j2k::J2kView::parse(raw.data())
                .ok()
                .and_then(|view| {
                    view.passthrough_candidate()
                        .map(|candidate| candidate.transfer_syntax().is_lossless())
                })
                .unwrap_or(false);
            canonical |= !lossless;
        }
    }
    if !canonical {
        return Ok(None);
    }
    if reader.preference == EncodeBackendPreference::Auto {
        // The auto probe compares the production CPU path and its host-input
        // device candidate. The inexact resident candidate is ineligible.
        return Ok(Some(empty_metal_tile_run(request.tile_count)));
    }
    let mut run = empty_metal_tile_run(0);
    run.used_gpu_input = false;
    run.row_batch_rows = 1;
    for start in
        (0..request.tile_count).step_by(frame_batch_len(request.tile_size, request.tile_size))
    {
        let end =
            (start + frame_batch_len(request.tile_size, request.tile_size)).min(request.tile_count);
        let frames: Vec<_> = (start..end)
            .map(|offset| {
                let frame_x = x + offset as u64 * u64::from(request.tile_size);
                OutputFrameRect::new(
                    frame_x,
                    y,
                    (request.matrix_columns - frame_x).min(u64::from(request.tile_size)) as u32,
                    (request.matrix_rows - y).min(u64::from(request.tile_size)) as u32,
                )
            })
            .collect();
        let prepared = prepare_cpu_input_batch(
            slide,
            request.level,
            request.location,
            &frames,
            request.tile_size,
            reader.max_prepared_frame_bytes,
        )?;
        let samples = prepared
            .iter()
            .map(|tile| lossless_j2k_samples_from_prepared_region(tile, request.tile_size))
            .collect::<Result<Vec<_>, _>>()?;
        let encoded =
            match encoder.encode_host_samples_batch(&samples, request.tile_size, request.tile_size)
            {
                Ok(encoded) => encoded,
                Err(err) if reader.preference == EncodeBackendPreference::RequireDevice => {
                    return Err(err)
                }
                Err(_) => return Ok(Some(empty_metal_tile_run(request.tile_count))),
            };
        run.gpu_encode_stats.add_assign(encoded.gpu_encode_stats);
        run.encode_batches += 1;
        for (tile, encoded) in prepared.into_iter().zip(encoded.frames) {
            run.input_decode_duration += tile.input_decode_duration;
            run.compose_duration += tile.compose_duration;
            run.tiles
                .push(encoded.map(|encoded| (encoded, tile.profile)));
        }
    }
    Ok(Some(run))
}
