use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use wsi_dicom::{time::duration_as_reported_micros, Error};

use crate::cli_report::{
    process_memory_pressure, process_resident_memory_bytes, process_thermal_state,
};

#[derive(Debug)]
pub(crate) struct SustainConfig {
    iterations: NonZeroU32,
    interval: Duration,
}

impl SustainConfig {
    pub(crate) fn new(iterations: u32, interval_ms: u64) -> Result<Self, Error> {
        let iterations = NonZeroU32::new(iterations).ok_or_else(|| Error::Unsupported {
            reason: "sustain iterations > 0 are required".into(),
        })?;
        Ok(Self {
            iterations,
            interval: Duration::from_millis(interval_ms),
        })
    }

    pub(crate) const fn should_pause_after(&self, iteration: u32) -> bool {
        !self.interval.is_zero() && iteration < self.iterations.get()
    }
}

pub(crate) struct SustainIteration {
    pub(crate) iteration: u32,
    pub(crate) iterations: u32,
    pub(crate) elapsed_micros: u128,
    pub(crate) rss_bytes: Option<u64>,
    pub(crate) thermal_state: Option<String>,
    pub(crate) memory_pressure: Option<String>,
}

pub(crate) fn run_sustained<T>(
    config: SustainConfig,
    mut operation: impl FnMut(u32) -> Result<T, Box<dyn std::error::Error>>,
    mut report: impl FnMut(&SustainIteration, &T) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    for iteration in 1..=config.iterations.get() {
        let started = Instant::now();
        let outcome = operation(iteration)?;
        let observation = SustainIteration {
            iteration,
            iterations: config.iterations.get(),
            elapsed_micros: duration_as_reported_micros(started.elapsed()),
            rss_bytes: process_resident_memory_bytes(),
            thermal_state: process_thermal_state(),
            memory_pressure: process_memory_pressure(),
        };
        report(&observation, &outcome)?;
        if config.should_pause_after(iteration) {
            std::thread::sleep(config.interval);
        }
    }
    Ok(())
}
