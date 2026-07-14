use std::time::{Duration, Instant};

use crate::error::Error;
use crate::time::duration_as_reported_micros;

#[derive(Clone, Copy)]
pub(super) struct RouteLevelDeadline {
    pub(super) started: Instant,
    pub(super) max_elapsed: Duration,
}

impl RouteLevelDeadline {
    pub(super) fn new(max_elapsed: Option<Duration>) -> Option<Self> {
        max_elapsed.map(|max_elapsed| Self {
            started: Instant::now(),
            max_elapsed,
        })
    }
}

pub(super) fn validate_max_level_elapsed(
    max_level_elapsed: Option<Duration>,
    context: &str,
) -> Result<(), Error> {
    if max_level_elapsed == Some(Duration::ZERO) {
        return Err(Error::Unsupported {
            reason: format!("{context} requires max_level_elapsed > 0 when provided"),
        });
    }
    Ok(())
}

pub(super) fn check_route_level_deadline(
    deadline: Option<RouteLevelDeadline>,
    level_idx: u32,
) -> Result<(), Error> {
    let Some(deadline) = deadline else {
        return Ok(());
    };
    let elapsed = deadline.started.elapsed();
    if elapsed > deadline.max_elapsed {
        return Err(Error::Unsupported {
            reason: format!(
                "route coverage level {level_idx} timed out after {:.3} ms (max_level_elapsed {:.3} ms)",
                duration_as_reported_micros(elapsed) as f64 / 1000.0,
                duration_as_reported_micros(deadline.max_elapsed) as f64 / 1000.0
            ),
        });
    }
    Ok(())
}
