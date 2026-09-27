//! CPU-decoded input sharing the resident Metal batch encoder.

use std::collections::VecDeque;
use std::sync::mpsc::{sync_channel, Receiver};

use super::cpu_batch::frame_batch_len;
use super::frame_region::PreparedCpuRegion;
use super::j2k_policy::effective_gpu_pipeline_depth;
use super::lossless_j2k_cpu::{
    lossless_j2k_samples_from_prepared_region, prepare_cpu_input_batch, LosslessJ2kCpuBatchOutcome,
};
use super::lossless_j2k_pipeline::LosslessJ2kBatchContext;
use crate::encode::{DicomJ2kEncoder, MetalHostSampleUploader, SubmittedDicomJ2kMetalTileBatch};
use crate::{EncodeBackendPreference, Error, ExportMetrics};

/// Decoded chunks waiting for submission. One chunk may wait here while the
/// next one decodes, which bounds decoded storage to a few chunks.
const PREPARED_HOST_CHUNKS_AHEAD: usize = 1;

/// A decoded chunk and its device upload, or the reason the upload failed.
struct PreparedHostChunk<'a> {
    indices: &'a [usize],
    prepared: Vec<PreparedCpuRegion>,
    uploaded: Result<Vec<wsi_rs::output::metal::MetalDeviceTile>, Error>,
}

/// One decoded chunk whose device encode may still be running.
struct InflightHostChunk<'a> {
    indices: &'a [usize],
    prepared: Vec<PreparedCpuRegion>,
    submission: Result<SubmittedDicomJ2kMetalTileBatch, Error>,
}

pub(super) fn encode_host_fallback_batch(
    context: LosslessJ2kBatchContext<'_>,
    encoder: &mut DicomJ2kEncoder,
    indices: &[usize],
    metrics: &mut ExportMetrics,
) -> Result<Vec<(usize, LosslessJ2kCpuBatchOutcome)>, Error> {
    let chunk_len = frame_batch_len(context.tile_size, context.tile_size);
    let uploader = match encoder.metal_host_sample_uploader() {
        Ok(uploader) => Some(uploader),
        Err(err) if encoder.preference() == EncodeBackendPreference::RequireDevice => {
            return Err(err)
        }
        // Without a device session every chunk takes the CPU fallback.
        Err(_) => None,
    };
    std::thread::scope(|scope| {
        // Decode and upload on their own thread, so the pool keeps preparing
        // the next chunk while this thread submits and waits on device work,
        // and uploads never queue behind unrelated pool work on this thread.
        let (sender, receiver) =
            sync_channel::<Result<PreparedHostChunk<'_>, Error>>(PREPARED_HOST_CHUNKS_AHEAD);
        let producer = scope.spawn(move || {
            for chunk in indices.chunks(chunk_len) {
                let prepared = prepare_host_chunk(context, uploader.as_ref(), chunk);
                let failed = prepared.is_err();
                // A closed channel means the consumer stopped after an error.
                if sender.send(prepared).is_err() || failed {
                    break;
                }
            }
        });
        // The receiver is dropped when submission returns, including on error,
        // so a producer blocked on a full channel always wakes and exits.
        let results =
            submit_prepared_host_chunks(context, encoder, receiver, indices.len(), metrics);
        producer.join().map_err(|_| Error::Encode {
            message: "Metal host input decode thread panicked".into(),
        })?;
        results
    })
}

fn prepare_host_chunk<'a>(
    context: LosslessJ2kBatchContext<'_>,
    uploader: Option<&MetalHostSampleUploader>,
    indices: &'a [usize],
) -> Result<PreparedHostChunk<'a>, Error> {
    let frames: Vec<_> = indices.iter().map(|&i| context.planned[i].rect()).collect();
    let prepared = prepare_cpu_input_batch(
        context.slide,
        context.level,
        context.location,
        &frames,
        context.tile_size,
        context.options.resources.max_prepared_frame_bytes,
    )?;
    let samples = prepared
        .iter()
        .map(|tile| lossless_j2k_samples_from_prepared_region(tile, context.tile_size))
        .collect::<Result<Vec<_>, _>>()?;
    let uploaded = uploader.map_or_else(
        || {
            Err(Error::Encode {
                message: "JPEG 2000 Metal session unavailable for host input".into(),
            })
        },
        |uploader| uploader.upload(&samples),
    );
    Ok(PreparedHostChunk {
        indices,
        prepared,
        uploaded,
    })
}

fn submit_prepared_host_chunks<'a>(
    context: LosslessJ2kBatchContext<'_>,
    encoder: &mut DicomJ2kEncoder,
    prepared_chunks: Receiver<Result<PreparedHostChunk<'a>, Error>>,
    frame_count: usize,
    metrics: &mut ExportMetrics,
) -> Result<Vec<(usize, LosslessJ2kCpuBatchOutcome)>, Error> {
    // Keep several chunks submitted so the host round trips inside each device
    // encode overlap other chunks' GPU work.
    let depth = effective_gpu_pipeline_depth(context.options).max(1);
    let mut inflight = VecDeque::with_capacity(depth);
    let mut results = Vec::with_capacity(frame_count);
    for prepared_chunk in prepared_chunks {
        let PreparedHostChunk {
            indices,
            prepared,
            uploaded,
        } = prepared_chunk?;
        let submission = uploaded.and_then(|tiles| {
            encoder.submit_metal_tiles_owned(tiles, context.tile_size, context.tile_size)
        });
        inflight.push_back(InflightHostChunk {
            indices,
            prepared,
            submission,
        });
        while inflight.len() >= depth {
            if let Some(chunk) = inflight.pop_front() {
                finish_host_chunk(context, encoder, chunk, metrics, &mut results)?;
            }
        }
    }
    while let Some(chunk) = inflight.pop_front() {
        finish_host_chunk(context, encoder, chunk, metrics, &mut results)?;
    }
    Ok(results)
}

fn finish_host_chunk(
    context: LosslessJ2kBatchContext<'_>,
    encoder: &DicomJ2kEncoder,
    chunk: InflightHostChunk<'_>,
    metrics: &mut ExportMetrics,
    results: &mut Vec<(usize, LosslessJ2kCpuBatchOutcome)>,
) -> Result<(), Error> {
    let InflightHostChunk {
        indices,
        prepared,
        submission,
    } = chunk;
    let encoded = match submission.and_then(SubmittedDicomJ2kMetalTileBatch::wait) {
        Ok(encoded) => {
            metrics.record_gpu_batches(0, 0, 1);
            metrics.record_gpu_encode_batch_stats(encoded.gpu_encode_stats);
            encoded.frames
        }
        Err(err) if encoder.preference() == EncodeBackendPreference::RequireDevice => {
            return Err(err)
        }
        Err(_) => (0..prepared.len()).map(|_| None).collect(),
    };
    for ((&index, tile), encoded) in indices.iter().zip(&prepared).zip(encoded) {
        let encoded = match encoded {
            Some(encoded) => Ok(encoded),
            None => encoder
                .cpu_only_peer()
                .encode(lossless_j2k_samples_from_prepared_region(
                    tile,
                    context.tile_size,
                )?),
        };
        results.push((
            index,
            LosslessJ2kCpuBatchOutcome {
                encoded,
                profile: tile.profile,
                input_decode_duration: tile.input_decode_duration,
                compose_duration: tile.compose_duration,
            },
        ));
    }
    Ok(())
}
