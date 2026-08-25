use std::path::PathBuf;
use std::time::Duration;

use wsi_dicom::{
    profile_dicom_route_corpus_coverage, profile_dicom_route_coverage, profile_dicom_routes, Error,
    RouteCoverageReport, RouteCoverageRequest, RouteProfileRequest, RouteProgressSink,
};

use crate::cli_args::EncodeArgs;
use crate::cli_output::{print_cli_output, print_json_line};
use crate::cli_report::{
    format_corpus_coverage_summary, format_coverage_summary, format_profile_summary,
    format_sustain_iteration_summary,
};
use crate::cli_sustain::{run_sustained, SustainConfig};

pub(crate) fn handle_profile(
    source: PathBuf,
    encode: EncodeArgs,
    level: u32,
    max_frames: u64,
    json: bool,
) -> Result<(), Error> {
    let request =
        RouteProfileRequest::new(source, encode.lossless_review_options(), level, max_frames)
            .with_source_aware_transfer_syntax(encode.source_aware_transfer_syntax());
    let report = profile_dicom_routes(request)?;
    print_cli_output(json, &report, format_profile_summary)
}

pub(crate) fn handle_coverage(
    source: PathBuf,
    encode: EncodeArgs,
    max_frames_per_level: u64,
    full_frame_coverage: bool,
    max_levels: Option<u32>,
    max_level_ms: Option<u64>,
    json: bool,
) -> Result<(), Error> {
    let mut request = RouteCoverageRequest::new(source, encode.lossless_review_options());
    configure_coverage_request(
        &mut request,
        encode,
        max_frames_per_level,
        full_frame_coverage,
        max_levels,
        max_level_ms,
        json,
    )?;
    let report = profile_dicom_route_coverage(request)?;
    print_cli_output(json, &report, format_coverage_summary)
}

pub(crate) fn handle_coverage_corpus(
    root: PathBuf,
    encode: EncodeArgs,
    max_frames_per_level: u64,
    full_frame_coverage: bool,
    max_levels: Option<u32>,
    max_level_ms: Option<u64>,
    json: bool,
) -> Result<(), Error> {
    let mut request = RouteCoverageRequest::new_corpus(root, encode.lossless_review_options());
    configure_coverage_request(
        &mut request,
        encode,
        max_frames_per_level,
        full_frame_coverage,
        max_levels,
        max_level_ms,
        json,
    )?;
    let report = profile_dicom_route_corpus_coverage(request)?;
    print_cli_output(json, &report, format_corpus_coverage_summary)
}

#[allow(clippy::too_many_arguments)]
fn configure_coverage_request(
    request: &mut RouteCoverageRequest,
    encode: EncodeArgs,
    max_frames_per_level: u64,
    full_frame_coverage: bool,
    max_levels: Option<u32>,
    max_level_ms: Option<u64>,
    json: bool,
) -> Result<(), Error> {
    request.source_aware_transfer_syntax = encode.source_aware_transfer_syntax();
    request.max_frames_per_level =
        effective_max_frames_per_level(max_frames_per_level, full_frame_coverage);
    request.max_levels = max_levels;
    request.max_level_elapsed = max_level_elapsed_from_ms(max_level_ms)?;
    request.progress = (!json).then_some(RouteProgressSink::Stderr);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_sustain(
    source: PathBuf,
    encode: EncodeArgs,
    max_frames_per_level: u64,
    full_frame_coverage: bool,
    max_levels: Option<u32>,
    max_level_ms: Option<u64>,
    iterations: u32,
    interval_ms: u64,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = SustainConfig::new(iterations, interval_ms)?;
    let source_aware_transfer_syntax = encode.source_aware_transfer_syntax();
    let options = encode.lossless_review_options();
    let max_frames_per_level =
        effective_max_frames_per_level(max_frames_per_level, full_frame_coverage);
    let max_level_elapsed = max_level_elapsed_from_ms(max_level_ms)?;
    run_sustained(
        config,
        |_| {
            let mut request = RouteCoverageRequest::new(source.clone(), options.clone());
            request.source_aware_transfer_syntax = source_aware_transfer_syntax;
            request.max_frames_per_level = max_frames_per_level;
            request.max_levels = max_levels;
            request.max_level_elapsed = max_level_elapsed;
            profile_dicom_route_coverage(request)
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
        },
        |iteration, report| {
            if json {
                print_json_line(&SustainCoverageIterationJson {
                    mode: "coverage",
                    iteration: iteration.iteration,
                    iterations: iteration.iterations,
                    rss_bytes: iteration.rss_bytes,
                    thermal_state: iteration.thermal_state.as_deref(),
                    memory_pressure: iteration.memory_pressure.as_deref(),
                    report,
                })
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)?;
            } else {
                println!(
                    "{}",
                    format_sustain_iteration_summary(
                        iteration.iteration,
                        iteration.iterations,
                        report,
                        iteration.rss_bytes,
                        iteration.thermal_state.as_deref(),
                        iteration.memory_pressure.as_deref(),
                    )
                );
            }
            Ok(())
        },
    )
}

#[derive(serde::Serialize)]
struct SustainCoverageIterationJson<'a> {
    mode: &'static str,
    iteration: u32,
    iterations: u32,
    rss_bytes: Option<u64>,
    thermal_state: Option<&'a str>,
    memory_pressure: Option<&'a str>,
    report: &'a RouteCoverageReport,
}

pub(crate) fn effective_max_frames_per_level(
    max_frames_per_level: u64,
    full_frame_coverage: bool,
) -> u64 {
    if full_frame_coverage {
        u64::MAX
    } else {
        max_frames_per_level
    }
}

pub(crate) fn max_level_elapsed_from_ms(
    max_level_ms: Option<u64>,
) -> Result<Option<Duration>, Error> {
    match max_level_ms {
        Some(0) => Err(Error::Unsupported {
            reason: "--max-level-ms must be greater than 0 when provided".into(),
        }),
        Some(max_level_ms) => Ok(Some(Duration::from_millis(max_level_ms))),
        None => Ok(None),
    }
}
