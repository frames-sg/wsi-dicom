use crate::cli_args::{Cli, Command};
use clap::Parser;
use std::path::PathBuf;
use wsi_dicom::{ExportMetrics, ExportReport, RouteCoverageReport, RouteProfileReport};

mod arguments_core;
mod arguments_export;
mod arguments_profiling;
mod reporting;
mod sustain_reporting;
mod sustain_runner;

fn parsed_max_level_ms(args: &[&str]) -> Option<u64> {
    let cli = Cli::try_parse_from(args).unwrap();
    match cli.command {
        Command::Coverage { max_level_ms, .. }
        | Command::CoverageCorpus { max_level_ms, .. }
        | Command::Sustain { max_level_ms, .. } => max_level_ms,
        _ => panic!("expected coverage, coverage-corpus, or sustain command"),
    }
}

fn test_metrics(configure: impl FnOnce(&mut ExportMetrics)) -> ExportMetrics {
    let mut metrics = ExportMetrics::default();
    configure(&mut metrics);
    metrics
}

fn export_report(output_dir: &str, metrics: ExportMetrics) -> ExportReport {
    let mut report = ExportReport::default();
    report.output_dir = PathBuf::from(output_dir);
    report.metrics = metrics;
    report
}

#[allow(clippy::too_many_arguments)]
fn route_coverage_report(
    source_path: &str,
    transfer_syntax_uid: &'static str,
    requested_frames_per_level: u64,
    available_frames: u64,
    complete_frame_coverage: bool,
    levels: Vec<RouteProfileReport>,
    metrics: ExportMetrics,
    elapsed_micros: u128,
) -> RouteCoverageReport {
    let mut report = RouteCoverageReport::default();
    report.source_path = PathBuf::from(source_path);
    report.transfer_syntax_uid = transfer_syntax_uid;
    report.requested_frames_per_level = requested_frames_per_level;
    report.available_frames = available_frames;
    report.complete_frame_coverage = complete_frame_coverage;
    report.levels = levels;
    report.metrics = metrics;
    report.elapsed_micros = elapsed_micros;
    report
}
