//! Bounded decoded-frame storage and CPU execution within the export pool.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
    let workers = workers.clamp(1, frames.len().max(1));
    if workers == 1 {
        return frames.iter().map(&operation).collect();
    }
    // At most `workers` tasks exist, including when the surrounding Rayon pool
    // has more threads than this batch permits. Each task claims the next
    // unprocessed frame, so a costly frame (dense tissue) delays only its own
    // worker instead of every frame statically assigned behind it.
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let completed = (0..workers)
        .into_par_iter()
        .with_max_len(1)
        .map(|_| {
            let mut completed = Vec::new();
            while !failed.load(Ordering::Relaxed) {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(frame) = frames.get(index) else {
                    break;
                };
                let result = operation(frame);
                if result.is_err() {
                    failed.store(true, Ordering::Relaxed);
                }
                completed.push((index, result));
            }
            completed
        })
        .collect::<Vec<_>>();
    let mut slots: Vec<Option<Result<R, Error>>> = (0..frames.len()).map(|_| None).collect();
    for (index, result) in completed.into_iter().flatten() {
        slots[index] = Some(result);
    }
    // Frames are claimed in order, so every unclaimed frame follows a failure
    // and the first error in frame order is reported, as with serial mapping.
    slots
        .into_iter()
        .map(|slot| {
            slot.unwrap_or_else(|| {
                Err(Error::Encode {
                    message: "CPU frame batch stopped after an earlier frame failed".into(),
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slow_frame_does_not_hold_back_frames_queued_behind_it() {
        // Frame 0 finishes only after every other frame has. With fixed chunks
        // the frames sharing its chunk could never start, so it would time out.
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let input: Vec<usize> = (0..8).collect();
        let finished = AtomicUsize::new(0);
        let output = pool
            .install(|| {
                map_cpu_frames(&input, 2, |&value| {
                    if value == 0 {
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(10);
                        while finished.load(Ordering::SeqCst) < input.len() - 1 {
                            if std::time::Instant::now() > deadline {
                                return Err(Error::Encode {
                                    message: "frames behind the slow frame never ran".into(),
                                });
                            }
                            std::thread::yield_now();
                        }
                    } else {
                        finished.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(value * 2)
                })
            })
            .unwrap();
        assert_eq!(output, (0..8).map(|value| value * 2).collect::<Vec<_>>());
    }

    #[test]
    fn the_first_failure_in_frame_order_is_reported() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();
        let input: Vec<usize> = (0..64).collect();
        let err = pool
            .install(|| {
                map_cpu_frames(&input, 4, |&value| {
                    if value == 5 || value == 40 {
                        Err(Error::Encode {
                            message: format!("frame {value} failed"),
                        })
                    } else {
                        Ok(value)
                    }
                })
            })
            .unwrap_err();
        assert!(err.to_string().contains("frame 5 failed"), "{err}");
    }

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
