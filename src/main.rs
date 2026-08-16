#![forbid(unsafe_code)]

use clap::Parser;
use wsi_dicom::Error;

mod cli_args;
mod cli_calibration;
mod cli_export;
mod cli_output;
mod cli_profile;
mod cli_report;
mod cli_validation;
mod time;

use cli_args::{Cli, Command};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Error> {
    match Cli::parse().command {
        Command::Convert {
            source,
            out,
            metadata,
            research_placeholder,
            export,
            annotations,
            level,
            json,
        } => cli_export::handle_convert(cli_export::ConvertRequest {
            source,
            out,
            metadata,
            research_placeholder,
            export_args: export,
            annotation_args: annotations,
            level,
            json,
        }),
        Command::Calibration { command } => cli_calibration::handle(command),
        Command::Profile {
            source,
            encode,
            level,
            max_frames,
            json,
        } => cli_profile::handle_profile(source, encode, level, max_frames, json),
        Command::Coverage {
            source,
            encode,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            json,
        } => cli_profile::handle_coverage(
            source,
            encode,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            json,
        ),
        Command::CoverageCorpus {
            root,
            encode,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            json,
        } => cli_profile::handle_coverage_corpus(
            root,
            encode,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            json,
        ),
        Command::SustainConvert {
            source,
            out,
            metadata,
            research_placeholder,
            export,
            level,
            iterations,
            interval_ms,
            json,
        } => cli_export::handle_sustain_convert(
            source,
            out,
            metadata,
            research_placeholder,
            export,
            level,
            iterations,
            interval_ms,
            json,
        ),
        Command::Sustain {
            source,
            encode,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            iterations,
            interval_ms,
            json,
        } => cli_profile::handle_sustain(
            source,
            encode,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            iterations,
            interval_ms,
            json,
        ),
        Command::Validate {
            path,
            strict,
            dcmvalidate_iod,
            htj2k_decoder,
            max_pixel_frames,
            command_timeout_secs,
            json,
        } => cli_validation::handle_validate(
            path,
            strict,
            dcmvalidate_iod,
            htj2k_decoder,
            max_pixel_frames,
            command_timeout_secs,
            json,
        ),
        Command::Doctor {
            strict,
            dcmvalidate_iod,
            htj2k_decoder,
            json,
        } => cli_validation::handle_doctor(strict, dcmvalidate_iod, htj2k_decoder, json),
        Command::SelfTest(arguments) => cli_validation::handle_self_test(arguments),
    }
}

fn sleep_between_iterations(interval_ms: u64, iteration: u32, iterations: u32) {
    if interval_ms > 0 && iteration < iterations {
        std::thread::sleep(std::time::Duration::from_millis(interval_ms));
    }
}

#[cfg(test)]
#[path = "main/tests/mod.rs"]
mod tests;
