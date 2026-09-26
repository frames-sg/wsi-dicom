//! CPU-decoded input sharing the resident Metal batch encoder.

use super::cpu_batch::frame_batch_len;
use super::lossless_j2k_cpu::{
    lossless_j2k_samples_from_prepared_region, prepare_cpu_input_batch, LosslessJ2kCpuBatchOutcome,
};
use super::lossless_j2k_pipeline::LosslessJ2kBatchContext;
use crate::encode::DicomJ2kEncoder;
use crate::{EncodeBackendPreference, Error, ExportMetrics};

pub(super) fn encode_host_fallback_batch(
    context: LosslessJ2kBatchContext<'_>,
    encoder: &mut DicomJ2kEncoder,
    indices: &[usize],
    metrics: &mut ExportMetrics,
) -> Result<Vec<(usize, LosslessJ2kCpuBatchOutcome)>, Error> {
    let mut results = Vec::with_capacity(indices.len());
    for batch in indices.chunks(frame_batch_len(context.tile_size, context.tile_size)) {
        let frames: Vec<_> = batch.iter().map(|&i| context.planned[i].rect()).collect();
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
        let encoded =
            match encoder.encode_host_samples_batch(&samples, context.tile_size, context.tile_size)
            {
                Ok(encoded) => {
                    metrics.record_gpu_batches(0, 0, 1);
                    metrics.record_gpu_encode_batch_stats(encoded.gpu_encode_stats);
                    encoded.frames
                }
                Err(err) if encoder.preference() == EncodeBackendPreference::RequireDevice => {
                    return Err(err)
                }
                Err(_) => (0..samples.len()).map(|_| None).collect(),
            };
        for (((&index, tile), sample), encoded) in
            batch.iter().zip(&prepared).zip(&samples).zip(encoded)
        {
            let encoded = match encoded {
                Some(encoded) => Ok(encoded),
                None => encoder.cpu_only_peer().encode(*sample),
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
    }
    Ok(results)
}
