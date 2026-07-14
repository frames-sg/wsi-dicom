use std::time::Duration;

/// Convert a duration into report microseconds, rounding positive sub-microsecond work up to one.
pub(crate) fn duration_as_reported_micros(duration: Duration) -> u128 {
    match duration.as_micros() {
        0 if duration > Duration::ZERO => 1,
        micros => micros,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_micros_preserves_zero_and_rounds_positive_submicrosecond_work() {
        assert_eq!(duration_as_reported_micros(Duration::ZERO), 0);
        assert_eq!(duration_as_reported_micros(Duration::from_nanos(1)), 1);
    }
}
