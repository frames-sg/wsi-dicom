#![forbid(unsafe_code)]

use std::{path::PathBuf, time::Duration};

use clap::{Args, Parser, Subcommand};
use wsi_dicom::{
    doctor_dicom_environment, profile_dicom_route_corpus_coverage, profile_dicom_route_coverage,
    profile_dicom_routes, run_dicom_self_test, validate_dicom_path, CodecValidation, DoctorOptions,
    DoctorReport, EncodeBackendPreference, Error, Export, ExportOptions, ExportPreset,
    ExportReport, IccProfilePolicy, JpegDirectHtj2kProfile, MetadataSource, RouteCoverageReport,
    RouteCoverageRequest, RouteProfileRequest, RouteProgressSink, SelfTestOptions, SelfTestReport,
    TransferSyntax, UidPolicy, ValidationOptions, ValidationReport,
};

mod cli_report;
mod time;

use cli_report::{
    format_corpus_coverage_summary, format_coverage_summary, format_profile_summary,
    format_report_summary, format_sustain_export_iteration_summary,
    format_sustain_iteration_summary, process_memory_pressure, process_resident_memory_bytes,
    process_thermal_state,
};

#[derive(Debug, Parser)]
#[command(name = "wsi-dicom")]
#[command(about = "Convert wsi-rs-readable whole-slide images to DICOM VL WSI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Convert {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, conflicts_with = "research_placeholder")]
        metadata: Option<PathBuf>,
        #[arg(long)]
        research_placeholder: bool,
        #[command(flatten)]
        export: ExportCliArgs,
        #[arg(long)]
        level: Option<u32>,
        #[arg(long)]
        json: bool,
    },
    Profile {
        source: PathBuf,
        #[command(flatten)]
        encode: EncodeArgs,
        #[arg(long, default_value_t = 0)]
        level: u32,
        #[arg(long, default_value_t = 64)]
        max_frames: u64,
        #[arg(long)]
        json: bool,
    },
    Coverage {
        source: PathBuf,
        #[command(flatten)]
        encode: EncodeArgs,
        #[arg(long, default_value_t = 64)]
        max_frames_per_level: u64,
        #[arg(long)]
        full_frame_coverage: bool,
        #[arg(long)]
        max_levels: Option<u32>,
        #[arg(long)]
        max_level_ms: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    CoverageCorpus {
        root: PathBuf,
        #[command(flatten)]
        encode: EncodeArgs,
        #[arg(long, default_value_t = 64)]
        max_frames_per_level: u64,
        #[arg(long)]
        full_frame_coverage: bool,
        #[arg(long)]
        max_levels: Option<u32>,
        #[arg(long)]
        max_level_ms: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    SustainConvert {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, conflicts_with = "research_placeholder")]
        metadata: Option<PathBuf>,
        #[arg(long)]
        research_placeholder: bool,
        #[command(flatten)]
        export: ExportCliArgs,
        #[arg(long)]
        level: Option<u32>,
        #[arg(long, default_value_t = 5)]
        iterations: u32,
        #[arg(long, default_value_t = 0)]
        interval_ms: u64,
        #[arg(long)]
        json: bool,
    },
    Sustain {
        source: PathBuf,
        #[command(flatten)]
        encode: EncodeArgs,
        #[arg(long, default_value_t = 64)]
        max_frames_per_level: u64,
        #[arg(long)]
        full_frame_coverage: bool,
        #[arg(long)]
        max_levels: Option<u32>,
        #[arg(long)]
        max_level_ms: Option<u64>,
        #[arg(long, default_value_t = 5)]
        iterations: u32,
        #[arg(long, default_value_t = 0)]
        interval_ms: u64,
        #[arg(long)]
        json: bool,
    },
    Validate {
        path: PathBuf,
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        dcmvalidate_iod: Option<PathBuf>,
        #[arg(long)]
        htj2k_decoder: Option<String>,
        #[arg(long, default_value_t = 1)]
        max_pixel_frames: usize,
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..))]
        command_timeout_secs: u64,
        #[arg(long)]
        json: bool,
    },
    Doctor {
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        dcmvalidate_iod: Option<PathBuf>,
        #[arg(long)]
        htj2k_decoder: Option<String>,
        #[arg(long)]
        json: bool,
    },
    SelfTest(SelfTestArgs),
}

#[derive(Debug, Args)]
struct SelfTestArgs {
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long)]
    keep_output: bool,
    #[arg(long)]
    strict: bool,
    #[arg(long)]
    dcmvalidate_iod: Option<PathBuf>,
    #[arg(long)]
    htj2k_decoder: Option<String>,
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..))]
    command_timeout_secs: u64,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, Args)]
struct EncodeArgs {
    #[arg(long, value_enum, default_value_t = EncodeBackendPreference::Auto)]
    backend: EncodeBackendPreference,
    #[arg(long, default_value_t = 512)]
    tile_size: u32,
    #[arg(long, default_value_t = 90)]
    jpeg_quality: u8,
    #[arg(long, value_enum)]
    transfer_syntax: Option<TransferSyntax>,
    #[arg(long, value_enum)]
    jpeg_direct_htj2k_profile: Option<JpegDirectHtj2kProfile>,
    #[arg(long)]
    j2k_decomposition_levels: Option<u8>,
    #[arg(long, value_enum, default_value_t = CodecValidation::Disabled)]
    codec_validation: CodecValidation,
    #[arg(long)]
    source_device_decode: bool,
    #[command(flatten)]
    gpu_encode: GpuEncodeArgs,
}

impl EncodeArgs {
    fn source_aware_transfer_syntax(self) -> bool {
        self.transfer_syntax.is_none()
    }

    fn defaulted_transfer_syntax(self) -> TransferSyntax {
        self.transfer_syntax
            .unwrap_or(ExportOptions::default().transfer_syntax)
    }

    fn lossless_review_options(self) -> ExportOptions {
        self.options_with_transfer_syntax(
            ExportPreset::LosslessReview.options(self.tile_size, self.jpeg_quality),
            self.defaulted_transfer_syntax(),
        )
    }

    fn options_with_transfer_syntax(
        self,
        mut options: ExportOptions,
        transfer_syntax: TransferSyntax,
    ) -> ExportOptions {
        options.transfer_syntax = transfer_syntax;
        options.jpeg_direct_htj2k_profile = self.jpeg_direct_htj2k_profile.unwrap_or_else(|| {
            JpegDirectHtj2kProfile::default_for_transfer_syntax(transfer_syntax)
        });
        options.j2k_decomposition_levels = self.j2k_decomposition_levels;
        options.encode_backend = self.backend;
        options.codec_validation = self.codec_validation;
        options.source_device_decode = self.source_device_decode;
        self.gpu_encode.into_options_fields(&mut options);
        options
    }
}

#[derive(Debug, Clone, Copy, Args)]
struct ExportCliArgs {
    #[command(flatten)]
    encode: EncodeArgs,
    #[arg(long, value_enum, default_value_t = ExportPreset::LosslessReview)]
    preset: ExportPreset,
    #[arg(long, value_enum, default_value_t = IccProfilePolicy::FallbackSrgb)]
    icc: IccProfilePolicy,
    #[arg(long, value_enum, default_value_t = UidPolicy::Fresh)]
    uid_policy: UidPolicy,
    #[arg(long)]
    overwrite: bool,
}

impl ExportCliArgs {
    fn options(self) -> Result<ExportOptions, Error> {
        let transfer_syntax =
            resolve_export_transfer_syntax(self.preset, self.encode.transfer_syntax)?;
        let mut options = self.encode.options_with_transfer_syntax(
            self.preset
                .options(self.encode.tile_size, self.encode.jpeg_quality),
            transfer_syntax,
        );
        options.icc_profile_policy = self.icc;
        options.uid_policy = self.uid_policy;
        options.overwrite = self.overwrite;
        Ok(options)
    }
}

#[derive(Debug, Clone, Copy, Default, Args)]
struct GpuEncodeArgs {
    #[arg(long)]
    gpu_encode_inflight_tiles: Option<usize>,
    #[arg(long)]
    gpu_encode_memory_mib: Option<u64>,
    #[arg(long)]
    gpu_pipeline_depth: Option<usize>,
    #[arg(long)]
    gpu_row_batch_rows: Option<usize>,
    #[arg(long)]
    gpu_row_batch_target_tiles: Option<usize>,
}

impl GpuEncodeArgs {
    fn into_options_fields(self, options: &mut ExportOptions) {
        options.gpu_encode_inflight_tiles = self.gpu_encode_inflight_tiles;
        options.gpu_encode_memory_mib = self.gpu_encode_memory_mib;
        options.gpu_pipeline_depth = self.gpu_pipeline_depth;
        options.gpu_row_batch_rows = self.gpu_row_batch_rows;
        options.gpu_row_batch_target_tiles = self.gpu_row_batch_target_tiles;
    }
}

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
            level,
            json,
        } => handle_convert(
            source,
            out,
            metadata,
            research_placeholder,
            export,
            level,
            json,
        ),
        Command::Profile {
            source,
            encode,
            level,
            max_frames,
            json,
        } => handle_profile(source, encode, level, max_frames, json),
        Command::Coverage {
            source,
            encode,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            json,
        } => handle_coverage(
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
        } => handle_coverage_corpus(
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
        } => handle_sustain_convert(
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
        } => handle_sustain(
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
        } => handle_validate(
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
        } => handle_doctor(strict, dcmvalidate_iod, htj2k_decoder, json),
        Command::SelfTest(arguments) => handle_self_test(arguments),
    }
}

fn handle_convert(
    source: PathBuf,
    out: PathBuf,
    metadata: Option<PathBuf>,
    research_placeholder: bool,
    export_args: ExportCliArgs,
    level: Option<u32>,
    json: bool,
) -> Result<(), Error> {
    let metadata = load_metadata_source(metadata, research_placeholder)?;
    let mut export = Export::from_slide(source)
        .to_directory(out)
        .with_metadata(metadata)
        .with_options(export_args.options()?);
    if let Some(level) = level {
        export = export.level(level);
    }
    let report = export.run()?;
    print_cli_output(json, &report, format_report_summary)
}

fn handle_profile(
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

fn handle_coverage(
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

fn handle_coverage_corpus(
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
fn handle_sustain_convert(
    source: PathBuf,
    out: PathBuf,
    metadata: Option<PathBuf>,
    research_placeholder: bool,
    export_args: ExportCliArgs,
    level: Option<u32>,
    iterations: u32,
    interval_ms: u64,
    json: bool,
) -> Result<(), Error> {
    if iterations == 0 {
        return Err(Error::Unsupported {
            reason: "sustain-convert requires iterations > 0".into(),
        });
    }
    let metadata = load_metadata_source(metadata, research_placeholder)?;
    let options = export_args.options()?;
    for iteration in 1..=iterations {
        let output_dir = out.join(format!("iteration-{iteration:04}"));
        let started = std::time::Instant::now();
        let mut export = Export::from_slide(source.clone())
            .to_directory(output_dir)
            .with_metadata(metadata.clone())
            .with_options(options.clone());
        if let Some(level) = level {
            export = export.level(level);
        }
        let report = export.run()?;
        let elapsed_micros = time::duration_as_reported_micros(started.elapsed());
        let thermal_state = process_thermal_state();
        let memory_pressure = process_memory_pressure();
        let rss_bytes = process_resident_memory_bytes();
        if json {
            print_json_line(&SustainExportIterationJson {
                mode: "convert",
                iteration,
                iterations,
                elapsed_micros,
                rss_bytes,
                thermal_state: thermal_state.as_deref(),
                memory_pressure: memory_pressure.as_deref(),
                report: &report,
            })?;
        } else {
            println!(
                "{}",
                format_sustain_export_iteration_summary(
                    iteration,
                    iterations,
                    &report,
                    elapsed_micros,
                    rss_bytes,
                    thermal_state.as_deref(),
                    memory_pressure.as_deref(),
                )
            );
        }
        sleep_between_iterations(interval_ms, iteration, iterations);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_sustain(
    source: PathBuf,
    encode: EncodeArgs,
    max_frames_per_level: u64,
    full_frame_coverage: bool,
    max_levels: Option<u32>,
    max_level_ms: Option<u64>,
    iterations: u32,
    interval_ms: u64,
    json: bool,
) -> Result<(), Error> {
    if iterations == 0 {
        return Err(Error::Unsupported {
            reason: "sustain requires iterations > 0".into(),
        });
    }
    let source_aware_transfer_syntax = encode.source_aware_transfer_syntax();
    let options = encode.lossless_review_options();
    let max_frames_per_level =
        effective_max_frames_per_level(max_frames_per_level, full_frame_coverage);
    let max_level_elapsed = max_level_elapsed_from_ms(max_level_ms)?;
    for iteration in 1..=iterations {
        let mut request = RouteCoverageRequest::new(source.clone(), options.clone());
        request.source_aware_transfer_syntax = source_aware_transfer_syntax;
        request.max_frames_per_level = max_frames_per_level;
        request.max_levels = max_levels;
        request.max_level_elapsed = max_level_elapsed;
        let report = profile_dicom_route_coverage(request)?;
        let thermal_state = process_thermal_state();
        let memory_pressure = process_memory_pressure();
        let rss_bytes = process_resident_memory_bytes();
        if json {
            print_json_line(&SustainCoverageIterationJson {
                mode: "coverage",
                iteration,
                iterations,
                rss_bytes,
                thermal_state: thermal_state.as_deref(),
                memory_pressure: memory_pressure.as_deref(),
                report: &report,
            })?;
        } else {
            println!(
                "{}",
                format_sustain_iteration_summary(
                    iteration,
                    iterations,
                    &report,
                    rss_bytes,
                    thermal_state.as_deref(),
                    memory_pressure.as_deref(),
                )
            );
        }
        sleep_between_iterations(interval_ms, iteration, iterations);
    }
    Ok(())
}

fn sleep_between_iterations(interval_ms: u64, iteration: u32, iterations: u32) {
    if interval_ms > 0 && iteration < iterations {
        std::thread::sleep(std::time::Duration::from_millis(interval_ms));
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_validate(
    path: PathBuf,
    strict: bool,
    dcmvalidate_iod: Option<PathBuf>,
    htj2k_decoder: Option<String>,
    max_pixel_frames: usize,
    command_timeout_secs: u64,
    json: bool,
) -> Result<(), Error> {
    let mut options = ValidationOptions::default();
    options.strict = strict;
    options.dcmvalidate_iod = dcmvalidate_iod;
    options.htj2k_decoder = htj2k_decoder;
    options.max_pixel_frames = max_pixel_frames;
    options.command_timeout_secs = command_timeout_secs;
    let report = validate_dicom_path(&path, &options)?;
    let has_failures = report.has_failures();
    let failed_checks = report.failed_checks();
    print_cli_output(json, &report, format_validation_summary)?;
    if has_failures {
        Err(Error::Validation {
            reason: format!("{failed_checks} validation check(s) failed"),
        })
    } else {
        Ok(())
    }
}

fn handle_doctor(
    strict: bool,
    dcmvalidate_iod: Option<PathBuf>,
    htj2k_decoder: Option<String>,
    json: bool,
) -> Result<(), Error> {
    let mut options = DoctorOptions::default();
    options.strict = strict;
    options.dcmvalidate_iod = dcmvalidate_iod;
    options.htj2k_decoder = htj2k_decoder;
    let report = doctor_dicom_environment(&options);
    let has_failures = report.has_failures();
    let failed_tools = report.failed_tools();
    print_cli_output(json, &report, format_doctor_summary)?;
    if has_failures {
        Err(Error::Validation {
            reason: format!("{failed_tools} doctor check(s) failed"),
        })
    } else {
        Ok(())
    }
}

fn handle_self_test(arguments: SelfTestArgs) -> Result<(), Error> {
    let mut validation = ValidationOptions::default();
    validation.strict = arguments.strict;
    validation.dcmvalidate_iod = arguments.dcmvalidate_iod;
    validation.htj2k_decoder = arguments.htj2k_decoder;
    validation.command_timeout_secs = arguments.command_timeout_secs;
    let mut options = SelfTestOptions::default();
    options.output_dir = arguments.out;
    options.keep_output = arguments.keep_output;
    options.validation = validation;
    let report = run_dicom_self_test(options)?;
    let has_failures = report.validation_report.has_failures();
    let failed_checks = report.validation_report.failed_checks();
    print_cli_output(arguments.json, &report, format_self_test_summary)?;
    if has_failures {
        Err(Error::Validation {
            reason: format!("{failed_checks} self-test validation check(s) failed"),
        })
    } else {
        Ok(())
    }
}

fn format_validation_summary(report: &ValidationReport) -> String {
    format!(
        "validated {} DICOM file(s) from {}; checks passed={} failed={} skipped={}",
        report.files.len(),
        report.input.display(),
        report.passed_checks(),
        report.failed_checks(),
        report.skipped_checks()
    )
}

fn format_doctor_summary(report: &DoctorReport) -> String {
    format!(
        "checked DICOM tooling; available={} failed={} missing={} skipped={}",
        report.available_tools(),
        report.failed_tools(),
        report.missing_tools(),
        report.skipped_tools()
    )
}

fn format_self_test_summary(report: &SelfTestReport) -> String {
    format!(
        "self-test wrote {} DICOM file(s) to {}; validation passed={} failed={} skipped={}; kept_output={}",
        report.export_report.instances.len(),
        report.output_dir.display(),
        report.validation_report.passed_checks(),
        report.validation_report.failed_checks(),
        report.validation_report.skipped_checks(),
        report.kept_output
    )
}

#[derive(serde::Serialize)]
struct SustainExportIterationJson<'a> {
    mode: &'static str,
    iteration: u32,
    iterations: u32,
    elapsed_micros: u128,
    rss_bytes: Option<u64>,
    thermal_state: Option<&'a str>,
    memory_pressure: Option<&'a str>,
    report: &'a ExportReport,
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

fn cli_output_line<T, F>(json: bool, value: &T, summary: F) -> Result<String, Error>
where
    T: serde::Serialize,
    F: FnOnce(&T) -> String,
{
    if json {
        serde_json::to_string(value).map_err(|source| Error::JsonSerialize {
            message: source.to_string(),
        })
    } else {
        Ok(summary(value))
    }
}

fn print_cli_output<T, F>(json: bool, value: &T, summary: F) -> Result<(), Error>
where
    T: serde::Serialize,
    F: FnOnce(&T) -> String,
{
    println!("{}", cli_output_line(json, value, summary)?);
    Ok(())
}

fn print_json_line<T: serde::Serialize>(value: &T) -> Result<(), Error> {
    let json = serde_json::to_string(value).map_err(|source| Error::JsonSerialize {
        message: source.to_string(),
    })?;
    println!("{json}");
    Ok(())
}

fn effective_max_frames_per_level(max_frames_per_level: u64, full_frame_coverage: bool) -> u64 {
    if full_frame_coverage {
        u64::MAX
    } else {
        max_frames_per_level
    }
}

fn max_level_elapsed_from_ms(max_level_ms: Option<u64>) -> Result<Option<Duration>, Error> {
    match max_level_ms {
        Some(0) => Err(Error::Unsupported {
            reason: "--max-level-ms must be greater than 0 when provided".into(),
        }),
        Some(max_level_ms) => Ok(Some(Duration::from_millis(max_level_ms))),
        None => Ok(None),
    }
}

fn resolve_export_transfer_syntax(
    preset: ExportPreset,
    transfer_syntax: Option<TransferSyntax>,
) -> Result<TransferSyntax, Error> {
    match (preset, transfer_syntax) {
        (ExportPreset::FastJpeg, Some(_)) => Err(Error::InvalidOptions {
            reason: "--preset fast-jpeg cannot be combined with --transfer-syntax".into(),
        }),
        (_, Some(transfer_syntax)) => Ok(transfer_syntax),
        (ExportPreset::LosslessReview, None) => Ok(TransferSyntax::Htj2kLosslessRpcl),
        (ExportPreset::FastJpeg, None) => Ok(TransferSyntax::JpegBaseline8Bit),
        (_, None) => Ok(TransferSyntax::Htj2kLosslessRpcl),
    }
}

fn load_metadata_source(
    metadata_path: Option<PathBuf>,
    research_placeholder: bool,
) -> Result<MetadataSource, Error> {
    if metadata_path.is_some() && research_placeholder {
        return Err(Error::Metadata {
            reason: "--metadata cannot be combined with --research-placeholder".into(),
        });
    }
    if research_placeholder {
        return Ok(MetadataSource::ResearchPlaceholder);
    }

    let Some(path) = metadata_path else {
        return Err(Error::Metadata {
            reason: "provide --metadata <json> or explicitly pass --research-placeholder".into(),
        });
    };

    MetadataSource::from_json_file(path)
}

#[cfg(test)]
mod tests {
    use super::{
        cli_output_line, effective_max_frames_per_level, load_metadata_source,
        max_level_elapsed_from_ms, resolve_export_transfer_syntax, Cli, Command, SelfTestArgs,
    };
    use crate::cli_report::{
        format_corpus_coverage_summary_with_memory, format_coverage_summary_with_memory,
        format_profile_summary_with_memory, format_report_summary_with_memory,
        format_sustain_export_iteration_summary, format_sustain_iteration_summary,
    };
    use clap::Parser;
    use std::path::PathBuf;
    use wsi_dicom::{
        ExportMetrics, ExportReport, RouteCorpusCoverageFailure, RouteCorpusCoverageReport,
        RouteCoverageReport, RouteProfileReport,
    };
    use wsi_dicom::{
        ExportPreset, IccProfilePolicy, JpegDirectHtj2kProfile, MetadataSource, TransferSyntax,
        UidPolicy,
    };

    #[derive(serde::Serialize)]
    struct SampleCliOutput {
        value: u8,
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

    fn route_profile_report(
        source_path: &str,
        transfer_syntax_uid: &'static str,
        level: u32,
        requested_frames: u64,
        available_frames: u64,
        metrics: ExportMetrics,
        elapsed_micros: u128,
    ) -> RouteProfileReport {
        let mut report = RouteProfileReport::default();
        report.source_path = PathBuf::from(source_path);
        report.transfer_syntax_uid = transfer_syntax_uid;
        report.level = level;
        report.requested_frames = requested_frames;
        report.available_frames = available_frames;
        report.metrics = metrics;
        report.elapsed_micros = elapsed_micros;
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

    fn corpus_failure(source_path: &str, message: &str) -> RouteCorpusCoverageFailure {
        let mut failure = RouteCorpusCoverageFailure::default();
        failure.source_path = PathBuf::from(source_path);
        failure.message = message.into();
        failure
    }

    #[allow(clippy::too_many_arguments)]
    fn route_corpus_coverage_report(
        source_root: &str,
        transfer_syntax_uids: Vec<&'static str>,
        requested_frames_per_level: u64,
        max_levels: Option<u32>,
        sources_considered: usize,
        available_frames: u64,
        complete_frame_coverage: bool,
        reports: Vec<RouteCoverageReport>,
        failures: Vec<RouteCorpusCoverageFailure>,
        metrics: ExportMetrics,
        elapsed_micros: u128,
    ) -> RouteCorpusCoverageReport {
        let mut report = RouteCorpusCoverageReport::default();
        report.source_root = PathBuf::from(source_root);
        report.transfer_syntax_uid = common_transfer_syntax_uid(&transfer_syntax_uids);
        report.transfer_syntax_uids = transfer_syntax_uids;
        report.requested_frames_per_level = requested_frames_per_level;
        report.max_levels = max_levels;
        report.sources_considered = sources_considered;
        report.available_frames = available_frames;
        report.complete_frame_coverage = complete_frame_coverage;
        report.reports = reports;
        report.failures = failures;
        report.metrics = metrics;
        report.elapsed_micros = elapsed_micros;
        report
    }

    fn common_transfer_syntax_uid(transfer_syntax_uids: &[&'static str]) -> Option<&'static str> {
        let first = transfer_syntax_uids.first().copied()?;
        transfer_syntax_uids
            .iter()
            .all(|uid| *uid == first)
            .then_some(first)
    }

    #[test]
    fn cli_output_line_formats_summary_or_json() {
        let sample = SampleCliOutput { value: 7 };

        let summary = cli_output_line(false, &sample, |_| "summary".to_string()).unwrap();
        assert_eq!(summary, "summary");

        let json = cli_output_line(true, &sample, |_| "summary".to_string()).unwrap();
        assert_eq!(json, r#"{"value":7}"#);
    }

    #[test]
    fn cli_requires_metadata_or_explicit_research_placeholder() {
        let err = load_metadata_source(None, false).unwrap_err();
        assert!(err.to_string().contains("--metadata"));

        let metadata = load_metadata_source(None, true).unwrap();

        assert!(matches!(metadata, MetadataSource::ResearchPlaceholder));
    }

    #[test]
    fn cli_coverage_full_frame_coverage_overrides_bounded_frame_count() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "coverage",
            "source.svs",
            "--max-frames-per-level",
            "1",
            "--full-frame-coverage",
        ])
        .unwrap();

        let Command::Coverage {
            max_frames_per_level,
            full_frame_coverage,
            ..
        } = cli.command
        else {
            panic!("expected coverage command");
        };

        assert_eq!(
            effective_max_frames_per_level(max_frames_per_level, full_frame_coverage),
            u64::MAX
        );
    }

    #[test]
    fn cli_coverage_accepts_max_level_elapsed_limit_ms() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "coverage",
            "source.svs",
            "--max-level-ms",
            "250",
        ])
        .unwrap();

        let Command::Coverage { max_level_ms, .. } = cli.command else {
            panic!("expected coverage command");
        };

        assert_eq!(max_level_ms, Some(250));
    }

    #[test]
    fn cli_coverage_accepts_source_device_decode_opt_in() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "coverage",
            "source.svs",
            "--source-device-decode",
        ])
        .unwrap();

        let Command::Coverage { encode, .. } = cli.command else {
            panic!("expected coverage command");
        };

        assert!(encode.source_device_decode);
    }

    #[test]
    fn cli_convert_accepts_source_device_decode_opt_in() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--source-device-decode",
        ])
        .unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert!(export.encode.source_device_decode);
    }

    #[test]
    fn cli_convert_defers_transfer_syntax_when_omitted() {
        let cli =
            Cli::try_parse_from(["wsi-dicom", "convert", "source.svs", "--out", "out"]).unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(export.preset, ExportPreset::LosslessReview);
        assert_eq!(export.encode.transfer_syntax, None);
    }

    #[test]
    fn cli_convert_accepts_fast_jpeg_preset() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--preset",
            "fast-jpeg",
        ])
        .unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(export.preset, ExportPreset::FastJpeg);
    }

    #[test]
    fn cli_convert_rejects_metadata_and_research_placeholder_conflict() {
        let err = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--metadata",
            "metadata.json",
            "--research-placeholder",
        ])
        .unwrap_err();

        assert!(err.to_string().contains("cannot be used with"));
    }

    #[test]
    fn cli_sustain_convert_accepts_fast_jpeg_preset() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "sustain-convert",
            "source.svs",
            "--out",
            "out",
            "--preset",
            "fast-jpeg",
        ])
        .unwrap();

        let Command::SustainConvert { export, .. } = cli.command else {
            panic!("expected sustain-convert command");
        };

        assert_eq!(export.preset, ExportPreset::FastJpeg);
    }

    #[test]
    fn fast_jpeg_preset_rejects_explicit_transfer_syntax() {
        let err = resolve_export_transfer_syntax(
            ExportPreset::FastJpeg,
            Some(TransferSyntax::Htj2kLosslessRpcl),
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("--preset fast-jpeg cannot be combined with --transfer-syntax"));
    }

    #[test]
    fn export_preset_resolves_default_lossless_and_fast_jpeg() {
        assert_eq!(
            resolve_export_transfer_syntax(ExportPreset::LosslessReview, None).unwrap(),
            TransferSyntax::Htj2kLosslessRpcl
        );
        assert_eq!(
            resolve_export_transfer_syntax(ExportPreset::FastJpeg, None).unwrap(),
            TransferSyntax::JpegBaseline8Bit
        );
    }

    #[test]
    fn cli_convert_defaults_tile_size_to_512() {
        let cli =
            Cli::try_parse_from(["wsi-dicom", "convert", "source.svs", "--out", "out"]).unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(export.encode.tile_size, 512);
    }

    #[test]
    fn cli_convert_preserves_explicit_htj2k_lossless_rpcl() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--transfer-syntax",
            "htj2k-lossless-rpcl",
        ])
        .unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(
            export.encode.transfer_syntax,
            Some(TransferSyntax::Htj2kLosslessRpcl)
        );
    }

    #[test]
    fn cli_convert_accepts_explicit_htj2k_97_profile() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--transfer-syntax",
            "htj2k",
            "--jpeg-direct-htj2k-profile",
            "97",
        ])
        .unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(export.encode.transfer_syntax, Some(TransferSyntax::Htj2k));
        assert_eq!(
            export.encode.jpeg_direct_htj2k_profile,
            Some(JpegDirectHtj2kProfile::Lossy97)
        );
    }

    #[test]
    fn cli_validate_accepts_validation_options() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "validate",
            "dicom-out",
            "--strict",
            "--json",
            "--dcmvalidate-iod",
            "vl-wsi.xml",
            "--htj2k-decoder",
            "ojph_expand -i {input} -o {output}",
            "--max-pixel-frames",
            "3",
            "--command-timeout-secs",
            "12",
        ])
        .unwrap();

        let Command::Validate {
            path,
            strict,
            json,
            dcmvalidate_iod,
            htj2k_decoder,
            max_pixel_frames,
            command_timeout_secs,
        } = cli.command
        else {
            panic!("expected validate command");
        };

        assert_eq!(path, PathBuf::from("dicom-out"));
        assert!(strict);
        assert!(json);
        assert_eq!(dcmvalidate_iod, Some(PathBuf::from("vl-wsi.xml")));
        assert_eq!(
            htj2k_decoder.as_deref(),
            Some("ojph_expand -i {input} -o {output}")
        );
        assert_eq!(max_pixel_frames, 3);
        assert_eq!(command_timeout_secs, 12);
    }

    #[test]
    fn cli_validate_defaults_to_one_pixel_frame_smoke() {
        let cli = Cli::try_parse_from(["wsi-dicom", "validate", "dicom-out"]).unwrap();

        let Command::Validate {
            strict,
            json,
            dcmvalidate_iod,
            htj2k_decoder,
            max_pixel_frames,
            command_timeout_secs,
            ..
        } = cli.command
        else {
            panic!("expected validate command");
        };

        assert!(!strict);
        assert!(!json);
        assert_eq!(dcmvalidate_iod, None);
        assert_eq!(htj2k_decoder, None);
        assert_eq!(max_pixel_frames, 1);
        assert_eq!(command_timeout_secs, 60);
    }

    #[test]
    fn cli_doctor_accepts_reviewer_tooling_options() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "doctor",
            "--strict",
            "--json",
            "--dcmvalidate-iod",
            "vl-wsi.xml",
            "--htj2k-decoder",
            "ojph_expand -i {input} -o {output}",
        ])
        .unwrap();

        let Command::Doctor {
            strict,
            json,
            dcmvalidate_iod,
            htj2k_decoder,
        } = cli.command
        else {
            panic!("expected doctor command");
        };

        assert!(strict);
        assert!(json);
        assert_eq!(dcmvalidate_iod, Some(PathBuf::from("vl-wsi.xml")));
        assert_eq!(
            htj2k_decoder.as_deref(),
            Some("ojph_expand -i {input} -o {output}")
        );
    }

    #[test]
    fn cli_self_test_accepts_reviewer_evidence_options() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "self-test",
            "--strict",
            "--json",
            "--out",
            "evidence",
            "--keep-output",
            "--dcmvalidate-iod",
            "vl-wsi.xml",
            "--htj2k-decoder",
            "ojph_expand -i {input} -o {output}",
            "--command-timeout-secs",
            "12",
        ])
        .unwrap();

        let Command::SelfTest(SelfTestArgs {
            strict,
            json,
            out,
            keep_output,
            dcmvalidate_iod,
            htj2k_decoder,
            command_timeout_secs,
        }) = cli.command
        else {
            panic!("expected self-test command");
        };

        assert!(strict);
        assert!(json);
        assert_eq!(out, Some(PathBuf::from("evidence")));
        assert!(keep_output);
        assert_eq!(dcmvalidate_iod, Some(PathBuf::from("vl-wsi.xml")));
        assert_eq!(
            htj2k_decoder.as_deref(),
            Some("ojph_expand -i {input} -o {output}")
        );
        assert_eq!(command_timeout_secs, 12);
    }

    #[test]
    fn cli_convert_defaults_icc_to_fallback_srgb() {
        let cli =
            Cli::try_parse_from(["wsi-dicom", "convert", "source.svs", "--out", "out"]).unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(export.icc, IccProfilePolicy::FallbackSrgb);
    }

    #[test]
    fn cli_convert_accepts_icc_policy() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--icc",
            "fallback-display-p3",
        ])
        .unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(export.icc, IccProfilePolicy::FallbackDisplayP3);
    }

    #[test]
    fn cli_convert_defaults_to_fresh_uids_and_accepts_deterministic_policy() {
        let default =
            Cli::try_parse_from(["wsi-dicom", "convert", "source.svs", "--out", "out"]).unwrap();
        let Command::Convert { export, .. } = default.command else {
            panic!("expected convert command");
        };
        assert_eq!(export.uid_policy, UidPolicy::Fresh);

        let deterministic = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--uid-policy",
            "deterministic",
        ])
        .unwrap();
        let Command::Convert { export, .. } = deterministic.command else {
            panic!("expected convert command");
        };
        assert_eq!(export.uid_policy, UidPolicy::Deterministic);
    }

    #[test]
    fn cli_sustain_convert_accepts_icc_policy() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "sustain-convert",
            "source.svs",
            "--out",
            "out",
            "--icc",
            "strict",
        ])
        .unwrap();

        let Command::SustainConvert { export, .. } = cli.command else {
            panic!("expected sustain-convert command");
        };

        assert_eq!(export.icc, IccProfilePolicy::Strict);
    }

    #[test]
    fn cli_convert_accepts_named_htj2k_97_quality_profiles() {
        for (profile_arg, expected) in [
            ("lossy97", JpegDirectHtj2kProfile::Lossy97),
            ("lossy97-near", JpegDirectHtj2kProfile::Lossy97Near),
            ("lossy97-balanced", JpegDirectHtj2kProfile::Lossy97Balanced),
            (
                "lossy97-aggressive",
                JpegDirectHtj2kProfile::Lossy97Aggressive,
            ),
            ("lossy97-preview", JpegDirectHtj2kProfile::Lossy97Preview),
            (
                "lossy97-thumbnail",
                JpegDirectHtj2kProfile::Lossy97Thumbnail,
            ),
        ] {
            let cli = Cli::try_parse_from([
                "wsi-dicom",
                "convert",
                "source.svs",
                "--out",
                "out",
                "--transfer-syntax",
                "htj2k",
                "--jpeg-direct-htj2k-profile",
                profile_arg,
            ])
            .unwrap();

            let Command::Convert { export, .. } = cli.command else {
                panic!("expected convert command");
            };

            assert_eq!(export.encode.jpeg_direct_htj2k_profile, Some(expected));
        }
    }

    #[test]
    fn cli_convert_accepts_gpu_encode_tuning_flags() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--gpu-encode-inflight-tiles",
            "8",
            "--gpu-encode-memory-mib",
            "4096",
            "--gpu-pipeline-depth",
            "3",
            "--gpu-row-batch-rows",
            "6",
            "--gpu-row-batch-target-tiles",
            "96",
        ])
        .unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        let gpu_encode = export.encode.gpu_encode;
        assert_eq!(gpu_encode.gpu_encode_inflight_tiles, Some(8));
        assert_eq!(gpu_encode.gpu_encode_memory_mib, Some(4096));
        assert_eq!(gpu_encode.gpu_pipeline_depth, Some(3));
        assert_eq!(gpu_encode.gpu_row_batch_rows, Some(6));
        assert_eq!(gpu_encode.gpu_row_batch_target_tiles, Some(96));
    }

    #[test]
    fn cli_convert_accepts_jpeg_quality_and_j2k_decomposition_levels() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--jpeg-quality",
            "80",
            "--j2k-decomposition-levels",
            "0",
        ])
        .unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(export.encode.jpeg_quality, 80);
        assert_eq!(export.encode.j2k_decomposition_levels, Some(0));
    }

    #[test]
    fn cli_profile_coverage_and_sustain_accept_equivalence_flags() {
        let profile = Cli::try_parse_from([
            "wsi-dicom",
            "profile",
            "source.svs",
            "--jpeg-quality",
            "80",
            "--j2k-decomposition-levels",
            "0",
        ])
        .unwrap();
        let Command::Profile { encode, .. } = profile.command else {
            panic!("expected profile command");
        };
        assert_eq!(encode.jpeg_quality, 80);
        assert_eq!(encode.j2k_decomposition_levels, Some(0));

        let coverage = Cli::try_parse_from([
            "wsi-dicom",
            "coverage",
            "source.svs",
            "--jpeg-quality",
            "80",
            "--j2k-decomposition-levels",
            "0",
        ])
        .unwrap();
        let Command::Coverage { encode, .. } = coverage.command else {
            panic!("expected coverage command");
        };
        assert_eq!(encode.jpeg_quality, 80);
        assert_eq!(encode.j2k_decomposition_levels, Some(0));

        let sustain_convert = Cli::try_parse_from([
            "wsi-dicom",
            "sustain-convert",
            "source.svs",
            "--out",
            "out",
            "--jpeg-quality",
            "80",
            "--j2k-decomposition-levels",
            "0",
        ])
        .unwrap();
        let Command::SustainConvert { export, .. } = sustain_convert.command else {
            panic!("expected sustain-convert command");
        };
        assert_eq!(export.encode.jpeg_quality, 80);
        assert_eq!(export.encode.j2k_decomposition_levels, Some(0));

        let sustain = Cli::try_parse_from([
            "wsi-dicom",
            "sustain",
            "source.svs",
            "--jpeg-quality",
            "80",
            "--j2k-decomposition-levels",
            "0",
        ])
        .unwrap();
        let Command::Sustain { encode, .. } = sustain.command else {
            panic!("expected sustain command");
        };
        assert_eq!(encode.jpeg_quality, 80);
        assert_eq!(encode.j2k_decomposition_levels, Some(0));
    }

    #[test]
    fn cli_coverage_corpus_accepts_max_level_elapsed_limit_ms() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "coverage-corpus",
            "slides",
            "--max-level-ms",
            "250",
        ])
        .unwrap();

        let Command::CoverageCorpus { max_level_ms, .. } = cli.command else {
            panic!("expected coverage-corpus command");
        };

        assert_eq!(max_level_ms, Some(250));
    }

    #[test]
    fn cli_sustain_accepts_max_level_elapsed_limit_ms() {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "sustain",
            "source.svs",
            "--max-level-ms",
            "250",
        ])
        .unwrap();

        let Command::Sustain { max_level_ms, .. } = cli.command else {
            panic!("expected sustain command");
        };

        assert_eq!(max_level_ms, Some(250));
    }

    #[test]
    fn cli_rejects_zero_max_level_elapsed_limit_ms() {
        let err = max_level_elapsed_from_ms(Some(0)).unwrap_err();

        assert!(
            err.to_string().contains("--max-level-ms"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn cli_summary_reports_passthrough_and_fallback_counts() {
        let summary = format_report_summary_with_memory(
            &export_report(
                "out",
                test_metrics(|metrics| {
                    metrics.routes.total_frames = 27;
                    metrics.routes.cpu_input_frames = 1;
                    metrics.routes.gpu_input_decode_frames = 2;
                    metrics.routes.gpu_encode_frames = 3;
                    metrics.routes.gpu_validation_frames = 4;
                    metrics.routes.jpeg_passthrough_frames = 5;
                    metrics.routes.j2k_passthrough_frames = 6;
                    metrics.routes.gpu_transcode_frames = 7;
                    metrics.routes.resident_gpu_transcode_frames = 4;
                    metrics.routes.partial_gpu_transcode_frames = 3;
                    metrics.routes.gpu_input_decode_batches = 2;
                    metrics.routes.gpu_compose_batches = 1;
                    metrics.routes.gpu_encode_batches = 3;
                    metrics.routes.cpu_fallback_frames = 9;
                    metrics.routes.jpeg_decode_fallback_frames = 1;
                    metrics.routes.jpeg_cpu_encode_frames = 1;
                    metrics.routes.jpeg_metal_encode_frames = 2;
                    metrics.gpu_encode.gpu_encode_configured_inflight_tiles = 8;
                    metrics.gpu_encode.gpu_encode_effective_inflight_tiles = 4;
                    metrics.gpu_encode.gpu_encode_max_observed_inflight_tiles = 4;
                    metrics.gpu_encode.gpu_encode_configured_memory_mib = 4096;
                    metrics.gpu_encode.gpu_encode_effective_memory_mib = 3277;
                    metrics.gpu_encode.gpu_encode_wall_micros = 5_000;
                    metrics.gpu_encode.gpu_encode_hardware_micros = 2_000;
                    metrics.gpu_encode.gpu_encode_dispatch_overhead_micros = 4_500;
                    metrics.timings.gpu_dispatch_micros = 6_500;
                }),
            ),
            Some(10 * 1024 * 1024),
        );

        assert!(summary.contains("jpeg_passthrough=5"));
        assert!(summary.contains("j2k_passthrough=6"));
        assert!(summary.contains("jpeg_decode_fallback=1"));
        assert!(summary.contains("jpeg_metal_encode=2"));
        assert!(summary.contains("route_passthrough=11"));
        assert!(summary.contains("route_passthrough_pct=40.7"));
        assert!(summary.contains("route_gpu_transcode=7"));
        assert!(summary.contains("route_gpu_transcode_pct=25.9"));
        assert!(summary.contains("route_resident_gpu_transcode=4"));
        assert!(summary.contains("route_partial_gpu_transcode=3"));
        assert!(summary.contains("gpu_input_batches=2"));
        assert!(summary.contains("gpu_compose_batches=1"));
        assert!(summary.contains("gpu_encode_batches=3"));
        assert!(summary.contains("gpu_encode_configured_inflight_tiles=8"));
        assert!(summary.contains("gpu_encode_effective_inflight_tiles=4"));
        assert!(summary.contains("gpu_encode_max_observed_inflight_tiles=4"));
        assert!(summary.contains("gpu_encode_configured_memory_mib=4096"));
        assert!(summary.contains("gpu_encode_effective_memory_mib=3277"));
        assert!(summary.contains("gpu_encode_wall_ms=5.000"));
        assert!(summary.contains("gpu_encode_effective_parallelism=0.400"));
        assert!(summary.contains("gpu_dispatch_ms=6.500"));
        assert!(summary.contains("gpu_encode_hardware_ms=2.000"));
        assert!(summary.contains("gpu_encode_dispatch_overhead_ms=4.500"));
        assert!(summary.contains("route_cpu_fallback=9"));
        assert!(summary.contains("route_cpu_fallback_pct=33.3"));
        assert!(summary.contains("route_unclassified=0"));
        assert!(summary.contains("rss_mb=10.0"));
        assert_eq!(
            summary,
            concat!(
                "wrote 0 DICOM instance(s) to out; frames total=27 route_passthrough=11 route_passthrough_pct=40.7 ",
                "route_gpu_transcode=7 route_gpu_transcode_pct=25.9 route_resident_gpu_transcode=4 route_partial_gpu_transcode=3 ",
                "route_cpu_fallback=9 route_cpu_fallback_pct=33.3 route_unclassified=0 cpu_input=1 gpu_input_decode=2 ",
                "gpu_encode=3 gpu_validation=4 gray_frames=0 rgb_like_frames=0 other_component_frames=0 unknown_pixel_profile_frames=0 ",
                "bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=2 gpu_compose_batches=1 gpu_encode_batches=3 ",
                "gpu_encode_configured_inflight_tiles=8 gpu_encode_effective_inflight_tiles=4 gpu_encode_max_observed_inflight_tiles=4 ",
                "gpu_encode_configured_memory_mib=4096 gpu_encode_effective_memory_mib=3277 gpu_encode_wall_ms=5.000 ",
                "gpu_encode_effective_parallelism=0.400 gpu_dispatch_ms=6.500 gpu_encode_hardware_ms=2.000 ",
                "gpu_encode_dispatch_overhead_ms=4.500 auto_probe_frames=0 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=0 ",
                "auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=5 j2k_passthrough=6 j2k_direct_htj2k=0 ",
                "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=1 ",
                "jpeg_cpu_encode=1 jpeg_metal_encode=2 input_decode_ms=0.000 compose_ms=0.000 encode_ms=0.000 ",
                "validation_ms=0.000 write_ms=0.000 rss_mb=10.0"
            )
        );
    }

    #[test]
    fn cli_profile_summary_reports_bounded_route_counts() {
        let summary = format_profile_summary_with_memory(
            &route_profile_report(
                "source.svs",
                "1.2.840.10008.1.2.4.202",
                2,
                12,
                20,
                test_metrics(|metrics| {
                    metrics.routes.total_frames = 10;
                    metrics.routes.gpu_transcode_frames = 7;
                    metrics.routes.resident_gpu_transcode_frames = 4;
                    metrics.routes.partial_gpu_transcode_frames = 3;
                    metrics.routes.cpu_fallback_frames = 3;
                    metrics.routes.gpu_input_decode_frames = 7;
                    metrics.routes.gpu_encode_frames = 7;
                    metrics.routes.jpeg_passthrough_frames = 2;
                    metrics.routes.jpeg_decode_fallback_frames = 1;
                    metrics.routes.jpeg_cpu_encode_frames = 1;
                    metrics.routes.jpeg_metal_encode_frames = 2;
                    metrics.routes.auto_route_probe_frames = 2;
                    metrics.routes.auto_route_probe_gpu_batches = 3;
                    metrics.routes.auto_route_probe_cpu_micros = 1_200;
                    metrics.routes.auto_route_probe_gpu_micros = 1_300;
                    metrics.timings.gpu_dispatch_micros = 6_500;
                    metrics.timings.write_micros = 1_250;
                }),
                42_500,
            ),
            Some(20 * 1024 * 1024),
        );

        assert!(summary.contains("profiled source.svs"));
        assert!(summary.contains("level=2"));
        assert!(summary.contains("requested_frames=12"));
        assert!(summary.contains("available_frames=20"));
        assert!(summary.contains("sampled_frames_pct=50.0000"));
        assert!(summary.contains("frames total=10"));
        assert!(summary.contains("route_gpu_transcode=7"));
        assert!(summary.contains("route_gpu_transcode_pct=70.0"));
        assert!(summary.contains("route_resident_gpu_transcode=4"));
        assert!(summary.contains("route_partial_gpu_transcode=3"));
        assert!(summary.contains("route_cpu_fallback=3"));
        assert!(summary.contains("jpeg_passthrough=2"));
        assert!(summary.contains("jpeg_decode_fallback=1"));
        assert!(summary.contains("jpeg_cpu_encode=1"));
        assert!(summary.contains("jpeg_metal_encode=2"));
        assert!(summary.contains("auto_probe_frames=2"));
        assert!(summary.contains("auto_probe_selected_gpu_input=0"));
        assert!(summary.contains("auto_probe_gpu_batches=3"));
        assert!(summary.contains("auto_probe_cpu_ms=1.200"));
        assert!(summary.contains("auto_probe_gpu_ms=1.300"));
        assert!(summary.contains("gpu_dispatch_ms=6.500"));
        assert!(summary.contains("final_byte_ms=1.250"));
        assert!(summary.contains("elapsed_ms=42.500"));
        assert!(summary.contains("rss_mb=20.0"));
        assert_eq!(
            summary,
            concat!(
                "profiled source.svs level=2 transfer_syntax=1.2.840.10008.1.2.4.202 requested_frames=12 available_frames=20 ",
                "sampled_frames_pct=50.0000 frames total=10 route_passthrough=2 route_passthrough_pct=20.0 ",
                "route_gpu_transcode=7 route_gpu_transcode_pct=70.0 route_resident_gpu_transcode=4 route_partial_gpu_transcode=3 ",
                "route_cpu_fallback=3 route_cpu_fallback_pct=30.0 route_unclassified=0 cpu_input=0 gpu_input_decode=7 gpu_encode=7 ",
                "gpu_validation=0 gray_frames=0 rgb_like_frames=0 other_component_frames=0 unknown_pixel_profile_frames=0 bits8_frames=0 ",
                "bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=0 gpu_compose_batches=0 gpu_encode_batches=0 ",
                "gpu_encode_configured_inflight_tiles=0 gpu_encode_effective_inflight_tiles=0 gpu_encode_max_observed_inflight_tiles=0 ",
                "gpu_encode_configured_memory_mib=0 gpu_encode_effective_memory_mib=0 gpu_encode_wall_ms=0.000 ",
                "gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=6.500 gpu_encode_hardware_ms=0.000 ",
                "gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=2 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=3 ",
                "auto_probe_cpu_ms=1.200 auto_probe_gpu_ms=1.300 jpeg_passthrough=2 j2k_passthrough=0 j2k_direct_htj2k=0 ",
                "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=1 ",
                "jpeg_cpu_encode=1 jpeg_metal_encode=2 final_byte_ms=1.250 input_decode_ms=0.000 compose_ms=0.000 ",
                "encode_ms=0.000 validation_ms=0.000 elapsed_ms=42.500 rss_mb=20.0"
            )
        );
    }

    #[test]
    fn cli_coverage_summary_reports_aggregate_route_counts() {
        let summary = format_coverage_summary_with_memory(
            &route_coverage_report(
                "source.ndpi",
                "1.2.840.10008.1.2.4.50",
                8,
                20,
                false,
                vec![
                    route_profile_report(
                        "source.ndpi",
                        "1.2.840.10008.1.2.4.50",
                        0,
                        8,
                        16,
                        test_metrics(|metrics| {
                            metrics.routes.total_frames = 8;
                            metrics.routes.jpeg_passthrough_frames = 8;
                        }),
                        1_000,
                    ),
                    route_profile_report(
                        "source.ndpi",
                        "1.2.840.10008.1.2.4.50",
                        1,
                        8,
                        4,
                        test_metrics(|metrics| {
                            metrics.routes.total_frames = 4;
                            metrics.routes.cpu_fallback_frames = 4;
                            metrics.routes.jpeg_decode_fallback_frames = 4;
                            metrics.routes.jpeg_cpu_encode_frames = 4;
                        }),
                        2_000,
                    ),
                ],
                test_metrics(|metrics| {
                    metrics.routes.total_frames = 12;
                    metrics.routes.jpeg_passthrough_frames = 8;
                    metrics.routes.cpu_fallback_frames = 4;
                    metrics.routes.jpeg_decode_fallback_frames = 4;
                    metrics.routes.jpeg_cpu_encode_frames = 4;
                    metrics.timings.input_decode_micros = 3_000;
                    metrics.timings.encode_micros = 4_000;
                    metrics.timings.gpu_dispatch_micros = 7_000;
                }),
                5_000,
            ),
            Some(30 * 1024 * 1024),
        );

        assert!(summary.contains("covered source.ndpi"));
        assert!(summary.contains("levels=2"));
        assert!(summary.contains("requested_frames_per_level=8"));
        assert!(summary.contains("available_frames=20"));
        assert!(summary.contains("sampled_frames_pct=60.0000"));
        assert!(summary.contains("complete_frame_coverage=false"));
        assert!(summary.contains("frames total=12"));
        assert!(summary.contains("route_passthrough=8"));
        assert!(summary.contains("route_passthrough_pct=66.7"));
        assert!(summary.contains("route_cpu_fallback=4"));
        assert!(summary.contains("route_cpu_fallback_pct=33.3"));
        assert!(summary.contains("jpeg_passthrough=8"));
        assert!(summary.contains("jpeg_decode_fallback=4"));
        assert!(summary.contains("jpeg_cpu_encode=4"));
        assert!(summary.contains("gpu_dispatch_ms=7.000"));
        assert!(summary.contains("elapsed_ms=5.000"));
        assert!(summary.contains("rss_mb=30.0"));
        assert_eq!(
            summary,
            concat!(
                "covered source.ndpi levels=2 transfer_syntax=1.2.840.10008.1.2.4.50 requested_frames_per_level=8 ",
                "available_frames=20 sampled_frames_pct=60.0000 complete_frame_coverage=false frames total=12 route_passthrough=8 ",
                "route_passthrough_pct=66.7 route_gpu_transcode=0 route_gpu_transcode_pct=0.0 route_resident_gpu_transcode=0 ",
                "route_partial_gpu_transcode=0 route_cpu_fallback=4 route_cpu_fallback_pct=33.3 route_unclassified=0 cpu_input=0 ",
                "gpu_input_decode=0 gpu_encode=0 gpu_validation=0 gray_frames=0 rgb_like_frames=0 other_component_frames=0 ",
                "unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=0 ",
                "gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 gpu_encode_effective_inflight_tiles=0 ",
                "gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 gpu_encode_effective_memory_mib=0 ",
                "gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=7.000 gpu_encode_hardware_ms=0.000 ",
                "gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=0 ",
                "auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=8 j2k_passthrough=0 j2k_direct_htj2k=0 ",
                "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=4 ",
                "jpeg_cpu_encode=4 jpeg_metal_encode=0 final_byte_ms=0.000 input_decode_ms=3.000 compose_ms=0.000 encode_ms=4.000 ",
                "validation_ms=0.000 elapsed_ms=5.000 rss_mb=30.0"
            )
        );
    }

    #[test]
    fn cli_coverage_summary_formats_full_frame_coverage_request_as_all() {
        let summary = format_coverage_summary_with_memory(
            &route_coverage_report(
                "source.svs",
                "1.2.840.10008.1.2.4.202",
                u64::MAX,
                1,
                true,
                Vec::new(),
                test_metrics(|metrics| {
                    metrics.routes.total_frames = 1;
                    metrics.routes.gpu_transcode_frames = 1;
                }),
                1_000,
            ),
            None,
        );

        assert!(summary.contains("requested_frames_per_level=all"));
        assert!(summary.contains("complete_frame_coverage=true"));
        assert_eq!(
            summary,
            concat!(
                "covered source.svs levels=0 transfer_syntax=1.2.840.10008.1.2.4.202 requested_frames_per_level=all ",
                "available_frames=1 sampled_frames_pct=100.0000 complete_frame_coverage=true frames total=1 route_passthrough=0 ",
                "route_passthrough_pct=0.0 route_gpu_transcode=1 route_gpu_transcode_pct=100.0 route_resident_gpu_transcode=0 ",
                "route_partial_gpu_transcode=0 route_cpu_fallback=0 route_cpu_fallback_pct=0.0 route_unclassified=0 cpu_input=0 ",
                "gpu_input_decode=0 gpu_encode=0 gpu_validation=0 gray_frames=0 rgb_like_frames=0 other_component_frames=0 ",
                "unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=0 ",
                "gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 gpu_encode_effective_inflight_tiles=0 ",
                "gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 gpu_encode_effective_memory_mib=0 ",
                "gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=0.000 gpu_encode_hardware_ms=0.000 ",
                "gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=0 ",
                "auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=0 j2k_passthrough=0 j2k_direct_htj2k=0 ",
                "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=0 ",
                "jpeg_cpu_encode=0 jpeg_metal_encode=0 final_byte_ms=0.000 input_decode_ms=0.000 compose_ms=0.000 encode_ms=0.000 ",
                "validation_ms=0.000 elapsed_ms=1.000 rss_mb=unknown"
            )
        );
    }

    #[test]
    fn cli_corpus_coverage_summary_reports_sources_failures_and_aggregate_routes() {
        let summary = format_corpus_coverage_summary_with_memory(
            &route_corpus_coverage_report(
                "corpus",
                vec!["1.2.840.10008.1.2.4.202"],
                4,
                Some(1),
                3,
                100_000,
                false,
                vec![route_coverage_report(
                    "corpus/source.svs",
                    "1.2.840.10008.1.2.4.202",
                    4,
                    100_000,
                    false,
                    vec![route_profile_report(
                        "corpus/source.svs",
                        "1.2.840.10008.1.2.4.202",
                        0,
                        4,
                        100_000,
                        test_metrics(|metrics| {
                            metrics.routes.total_frames = 4;
                            metrics.routes.gpu_transcode_frames = 4;
                            metrics.routes.resident_gpu_transcode_frames = 4;
                            metrics.routes.gpu_input_decode_frames = 4;
                            metrics.routes.gpu_encode_frames = 4;
                        }),
                        10_000,
                    )],
                    test_metrics(|metrics| {
                        metrics.routes.total_frames = 4;
                        metrics.routes.gpu_transcode_frames = 4;
                        metrics.routes.resident_gpu_transcode_frames = 4;
                        metrics.routes.gpu_input_decode_frames = 4;
                        metrics.routes.gpu_encode_frames = 4;
                    }),
                    10_000,
                )],
                vec![corpus_failure("corpus/bad.svs", "unsupported")],
                test_metrics(|metrics| {
                    metrics.routes.total_frames = 4;
                    metrics.routes.gpu_transcode_frames = 4;
                    metrics.routes.resident_gpu_transcode_frames = 4;
                    metrics.routes.gpu_input_decode_frames = 4;
                    metrics.routes.gpu_encode_frames = 4;
                    metrics.timings.gpu_dispatch_micros = 9_000;
                }),
                12_000,
            ),
            Some(40 * 1024 * 1024),
        );

        assert!(summary.contains("covered_corpus corpus"));
        assert!(summary.contains("sources_considered=3"));
        assert!(summary.contains("sources_profiled=1"));
        assert!(summary.contains("failures=1"));
        assert!(summary.contains("available_frames=100000"));
        assert!(summary.contains("sampled_frames_pct=0.0040"));
        assert!(summary.contains("complete_frame_coverage=false"));
        assert!(summary.contains("route_gpu_transcode=4"));
        assert!(summary.contains("route_gpu_transcode_pct=100.0"));
        assert!(summary.contains("route_resident_gpu_transcode=4"));
        assert!(summary.contains("gpu_dispatch_ms=9.000"));
        assert!(summary.contains("rss_mb=40.0"));
        assert_eq!(
            summary,
            concat!(
                "covered_corpus corpus sources_considered=3 sources_profiled=1 failures=1 common_transfer_syntax=1.2.840.10008.1.2.4.202 transfer_syntaxes=1.2.840.10008.1.2.4.202 ",
                "requested_frames_per_level=4 available_frames=100000 sampled_frames_pct=0.0040 complete_frame_coverage=false ",
                "frames total=4 route_passthrough=0 route_passthrough_pct=0.0 route_gpu_transcode=4 route_gpu_transcode_pct=100.0 ",
                "route_resident_gpu_transcode=4 route_partial_gpu_transcode=0 route_cpu_fallback=0 route_cpu_fallback_pct=0.0 ",
                "route_unclassified=0 cpu_input=0 gpu_input_decode=4 gpu_encode=4 gpu_validation=0 gray_frames=0 rgb_like_frames=0 ",
                "other_component_frames=0 unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 ",
                "gpu_input_batches=0 gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 ",
                "gpu_encode_effective_inflight_tiles=0 gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 ",
                "gpu_encode_effective_memory_mib=0 gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=9.000 ",
                "gpu_encode_hardware_ms=0.000 gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 ",
                "auto_probe_gpu_batches=0 auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=0 j2k_passthrough=0 j2k_direct_htj2k=0 ",
                "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=0 ",
                "jpeg_cpu_encode=0 jpeg_metal_encode=0 final_byte_ms=0.000 input_decode_ms=0.000 ",
                "compose_ms=0.000 encode_ms=0.000 validation_ms=0.000 elapsed_ms=12.000 rss_mb=40.0"
            )
        );
    }

    #[test]
    fn cli_sustain_iteration_summary_reports_throughput_memory_and_thermal_state() {
        let summary = format_sustain_iteration_summary(
            2,
            5,
            &route_coverage_report(
                "source.svs",
                "1.2.840.10008.1.2.4.202",
                4,
                12,
                true,
                Vec::new(),
                test_metrics(|metrics| {
                    metrics.routes.total_frames = 12;
                    metrics.routes.gpu_transcode_frames = 12;
                    metrics.routes.resident_gpu_transcode_frames = 12;
                    metrics.routes.gpu_input_decode_frames = 12;
                    metrics.routes.gpu_encode_frames = 12;
                    metrics.timings.gpu_dispatch_micros = 12_500;
                }),
                2_000_000,
            ),
            Some(40 * 1024 * 1024),
            Some("No thermal warning level has been recorded"),
            Some("System-wide memory free percentage: 92%"),
        );

        assert!(summary.contains("sustain_iteration=2/5"));
        assert!(summary.contains("frames=12"));
        assert!(summary.contains("available_frames=12"));
        assert!(summary.contains("sampled_frames_pct=100.0000"));
        assert!(summary.contains("complete_frame_coverage=true"));
        assert!(summary.contains("frames_per_sec=6.00"));
        assert!(summary.contains("route_gpu_transcode=12"));
        assert!(summary.contains("route_resident_gpu_transcode=12"));
        assert!(summary.contains("gpu_dispatch_ms=12.500"));
        assert!(summary.contains("rss_mb=40.0"));
        assert!(summary.contains("thermal=\"No thermal warning level has been recorded\""));
        assert!(summary.contains("memory_pressure=\"System-wide memory free percentage: 92%\""));
        assert_eq!(
            summary,
            concat!(
                "sustain_iteration=2/5 source=source.svs transfer_syntax=1.2.840.10008.1.2.4.202 frames=12 available_frames=12 ",
                "sampled_frames_pct=100.0000 complete_frame_coverage=true frames_per_sec=6.00 route_passthrough=0 ",
                "route_passthrough_pct=0.0 route_gpu_transcode=12 route_gpu_transcode_pct=100.0 route_resident_gpu_transcode=12 ",
                "route_partial_gpu_transcode=0 route_cpu_fallback=0 route_cpu_fallback_pct=0.0 route_unclassified=0 cpu_input=0 ",
                "gpu_input_decode=12 gpu_encode=12 gpu_validation=0 gray_frames=0 rgb_like_frames=0 other_component_frames=0 ",
                "unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 gpu_input_batches=0 ",
                "gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 gpu_encode_effective_inflight_tiles=0 ",
                "gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 gpu_encode_effective_memory_mib=0 ",
                "gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=12.500 gpu_encode_hardware_ms=0.000 ",
                "gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 auto_probe_gpu_batches=0 ",
                "auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=0 j2k_passthrough=0 j2k_direct_htj2k=0 ",
                "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=0 ",
                "jpeg_cpu_encode=0 jpeg_metal_encode=0 final_byte_ms=0.000 input_decode_ms=0.000 compose_ms=0.000 encode_ms=0.000 ",
                "validation_ms=0.000 elapsed_ms=2000.000 rss_mb=40.0 thermal=\"No thermal warning level has been recorded\" ",
                "memory_pressure=\"System-wide memory free percentage: 92%\""
            )
        );
    }

    #[test]
    fn cli_sustain_convert_summary_reports_real_export_throughput() {
        let summary = format_sustain_export_iteration_summary(
            1,
            3,
            &export_report(
                "out/iteration-0001",
                test_metrics(|metrics| {
                    metrics.routes.total_frames = 20;
                    metrics.routes.j2k_passthrough_frames = 4;
                    metrics.routes.gpu_transcode_frames = 12;
                    metrics.routes.resident_gpu_transcode_frames = 10;
                    metrics.routes.partial_gpu_transcode_frames = 2;
                    metrics.routes.cpu_fallback_frames = 4;
                    metrics.routes.gpu_input_decode_frames = 12;
                    metrics.routes.gpu_encode_frames = 12;
                    metrics.timings.write_micros = 3_500;
                    metrics.timings.input_decode_micros = 8_000;
                    metrics.timings.compose_micros = 2_000;
                    metrics.timings.encode_micros = 4_000;
                    metrics.timings.validation_micros = 1_000;
                    metrics.timings.gpu_dispatch_micros = 15_000;
                }),
            ),
            2_000_000,
            Some(50 * 1024 * 1024),
            Some("No thermal warning level has been recorded"),
            Some("System-wide memory free percentage: 91%"),
        );

        assert!(summary.contains("sustain_iteration=1/3"));
        assert!(summary.contains("mode=convert"));
        assert!(summary.contains("output=out/iteration-0001"));
        assert!(summary.contains("frames=20"));
        assert!(summary.contains("frames_per_sec=10.00"));
        assert!(summary.contains("route_passthrough=4"));
        assert!(summary.contains("route_gpu_transcode=12"));
        assert!(summary.contains("route_resident_gpu_transcode=10"));
        assert!(summary.contains("route_partial_gpu_transcode=2"));
        assert!(summary.contains("route_cpu_fallback=4"));
        assert!(summary.contains("gpu_dispatch_ms=15.000"));
        assert!(summary.contains("final_byte_ms=3.500"));
        assert!(summary.contains("elapsed_ms=2000.000"));
        assert!(summary.contains("rss_mb=50.0"));
        assert!(summary.contains("thermal=\"No thermal warning level has been recorded\""));
        assert!(summary.contains("memory_pressure=\"System-wide memory free percentage: 91%\""));
        assert_eq!(
            summary,
            concat!(
                "sustain_iteration=1/3 mode=convert output=out/iteration-0001 instances=0 frames=20 frames_per_sec=10.00 ",
                "route_passthrough=4 route_passthrough_pct=20.0 route_gpu_transcode=12 route_gpu_transcode_pct=60.0 ",
                "route_resident_gpu_transcode=10 route_partial_gpu_transcode=2 route_cpu_fallback=4 route_cpu_fallback_pct=20.0 ",
                "route_unclassified=0 cpu_input=0 gpu_input_decode=12 gpu_encode=12 gpu_validation=0 gray_frames=0 rgb_like_frames=0 ",
                "other_component_frames=0 unknown_pixel_profile_frames=0 bits8_frames=0 bits16_frames=0 other_bit_depth_frames=0 ",
                "gpu_input_batches=0 gpu_compose_batches=0 gpu_encode_batches=0 gpu_encode_configured_inflight_tiles=0 ",
                "gpu_encode_effective_inflight_tiles=0 gpu_encode_max_observed_inflight_tiles=0 gpu_encode_configured_memory_mib=0 ",
                "gpu_encode_effective_memory_mib=0 gpu_encode_wall_ms=0.000 gpu_encode_effective_parallelism=0.000 gpu_dispatch_ms=15.000 ",
                "gpu_encode_hardware_ms=0.000 gpu_encode_dispatch_overhead_ms=0.000 auto_probe_frames=0 auto_probe_selected_gpu_input=0 ",
                "auto_probe_gpu_batches=0 auto_probe_cpu_ms=0.000 auto_probe_gpu_ms=0.000 jpeg_passthrough=0 j2k_passthrough=4 j2k_direct_htj2k=0 ",
                "jpeg_direct_htj2k_53=0 jpeg_direct_htj2k_97=0 jpeg_direct_htj2k_rejected=0 jpeg_retile=0 jpeg_retile_rejected=0 jpeg_retile_source_unsupported=0 jpeg_retile_geometry_mismatch=0 jpeg_retile_profile_unsupported=0 jpeg_retile_mcu_invalid=0 jpeg_retile_ms=0.000 jpeg_retile_to_htj2k_53=0 jpeg_decode_fallback=0 ",
                "jpeg_cpu_encode=0 jpeg_metal_encode=0 final_byte_ms=3.500 input_decode_ms=8.000 ",
                "compose_ms=2.000 encode_ms=4.000 validation_ms=1.000 elapsed_ms=2000.000 rss_mb=50.0 ",
                "thermal=\"No thermal warning level has been recorded\" memory_pressure=\"System-wide memory free percentage: 91%\""
            )
        );
    }
}
