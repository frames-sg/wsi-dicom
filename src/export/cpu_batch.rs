//! Bounded decoded-frame storage and CPU execution within the export pool.

use rayon::prelude::*;

use crate::Error;

const PREPARED_BATCH_MEMORY_BYTES: u64 = 128 * 1024 * 1024;

pub(super) fn frame_batch_len(width: u32, height: u32) -> usize {
    // An accepted source can be RGBA16 (8 B/px). Allow both decoded input and
    // prepared output to coexist. Codec scratch is owned by the codec itself.
    let working_bytes = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(16);
    let memory_frames = PREPARED_BATCH_MEMORY_BYTES / working_bytes.max(1);
    usize::try_from(memory_frames)
        .unwrap_or(usize::MAX)
        .max(1)
        .min(rayon::current_num_threads().saturating_mul(4).max(1))
}

pub(super) fn map_cpu_frames<T: Sync, R: Send>(
    frames: &[T],
    workers: usize,
    operation: impl Fn(&T) -> Result<R, Error> + Sync + Send,
) -> Result<Vec<R>, Error> {
    if frames.is_empty() {
        return Ok(Vec::new());
    }
    // At most `workers` chunks exist. Each chunk executes serially, including
    // when its surrounding Rayon pool has more threads than this batch permits.
    frames
        .par_chunks(frames.len().div_ceil(workers.max(1)))
        .map(|chunk| chunk.iter().map(&operation).collect::<Result<Vec<_>, _>>())
        .collect::<Result<Vec<_>, _>>()
        .map(|chunks| chunks.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn execution_obeys_worker_limit_and_preserves_frame_order() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(8)
            .build()
            .unwrap();
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let input: Vec<_> = (0..32).collect();
        let output = pool
            .install(|| {
                map_cpu_frames(&input, 2, |value| {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(current, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(value * 3)
                })
            })
            .unwrap();
        assert!(peak.load(Ordering::SeqCst) <= 2);
        assert_eq!(
            output,
            input.iter().map(|value| value * 3).collect::<Vec<_>>()
        );
    }
}
