#![forbid(unsafe_code)]

use std::{path::PathBuf, time::Duration};

use clap::{Parser, Subcommand, ValueEnum};
use wsi_dicom::{
    export_dicom, profile_dicom_route_corpus_coverage, profile_dicom_route_coverage,
    profile_dicom_routes, CodecValidation, DicomExportOptions, DicomExportReport,
    DicomExportRequest, DicomMetadata, DicomRouteCorpusCoverageProgress,
    DicomRouteCorpusCoverageReport, DicomRouteCorpusCoverageRequest, DicomRouteCoverageProgress,
    DicomRouteCoverageReport, DicomRouteCoverageRequest, DicomRouteProfileReport,
    DicomRouteProfileRequest, EncodeBackendPreference, MetadataSource, TransferSyntax,
    WsiDicomError,
};

#[derive(Debug, Parser)]
#[command(name = "wsi-dicom")]
#[command(about = "Convert statumen-readable whole-slide images to DICOM VL WSI")]
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
        #[arg(long)]
        metadata: Option<PathBuf>,
        #[arg(long)]
        research_placeholder: bool,
        #[arg(long, value_enum, default_value_t = BackendArg::Auto)]
        backend: BackendArg,
        #[arg(long, default_value_t = 512)]
        tile_size: u32,
        #[arg(long, value_enum, default_value_t = TransferSyntaxArg::Jpeg2000Lossless)]
        transfer_syntax: TransferSyntaxArg,
        #[arg(long, value_enum, default_value_t = CodecValidationArg::Disabled)]
        codec_validation: CodecValidationArg,
        #[arg(long)]
        source_device_decode: bool,
        #[arg(long)]
        level: Option<u32>,
        #[arg(long)]
        json: bool,
    },
    Profile {
        source: PathBuf,
        #[arg(long, value_enum, default_value_t = BackendArg::Auto)]
        backend: BackendArg,
        #[arg(long, default_value_t = 512)]
        tile_size: u32,
        #[arg(long, value_enum, default_value_t = TransferSyntaxArg::Htj2kLosslessRpcl)]
        transfer_syntax: TransferSyntaxArg,
        #[arg(long, value_enum, default_value_t = CodecValidationArg::Disabled)]
        codec_validation: CodecValidationArg,
        #[arg(long, default_value_t = 0)]
        level: u32,
        #[arg(long, default_value_t = 64)]
        max_frames: u64,
        #[arg(long)]
        source_device_decode: bool,
        #[arg(long)]
        json: bool,
    },
    Coverage {
        source: PathBuf,
        #[arg(long, value_enum, default_value_t = BackendArg::Auto)]
        backend: BackendArg,
        #[arg(long, default_value_t = 512)]
        tile_size: u32,
        #[arg(long, value_enum, default_value_t = TransferSyntaxArg::Htj2kLosslessRpcl)]
        transfer_syntax: TransferSyntaxArg,
        #[arg(long, value_enum, default_value_t = CodecValidationArg::Disabled)]
        codec_validation: CodecValidationArg,
        #[arg(long, default_value_t = 64)]
        max_frames_per_level: u64,
        #[arg(long)]
        full_frame_coverage: bool,
        #[arg(long)]
        max_levels: Option<u32>,
        #[arg(long)]
        max_level_ms: Option<u64>,
        #[arg(long)]
        source_device_decode: bool,
        #[arg(long)]
        json: bool,
    },
    CoverageCorpus {
        root: PathBuf,
        #[arg(long, value_enum, default_value_t = BackendArg::Auto)]
        backend: BackendArg,
        #[arg(long, default_value_t = 512)]
        tile_size: u32,
        #[arg(long, value_enum, default_value_t = TransferSyntaxArg::Htj2kLosslessRpcl)]
        transfer_syntax: TransferSyntaxArg,
        #[arg(long, value_enum, default_value_t = CodecValidationArg::Disabled)]
        codec_validation: CodecValidationArg,
        #[arg(long, default_value_t = 64)]
        max_frames_per_level: u64,
        #[arg(long)]
        full_frame_coverage: bool,
        #[arg(long)]
        max_levels: Option<u32>,
        #[arg(long)]
        max_level_ms: Option<u64>,
        #[arg(long)]
        source_device_decode: bool,
        #[arg(long)]
        json: bool,
    },
    SustainConvert {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        metadata: Option<PathBuf>,
        #[arg(long)]
        research_placeholder: bool,
        #[arg(long, value_enum, default_value_t = BackendArg::Auto)]
        backend: BackendArg,
        #[arg(long, default_value_t = 512)]
        tile_size: u32,
        #[arg(long, value_enum, default_value_t = TransferSyntaxArg::Htj2kLosslessRpcl)]
        transfer_syntax: TransferSyntaxArg,
        #[arg(long, value_enum, default_value_t = CodecValidationArg::Disabled)]
        codec_validation: CodecValidationArg,
        #[arg(long)]
        source_device_decode: bool,
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
        #[arg(long, value_enum, default_value_t = BackendArg::Auto)]
        backend: BackendArg,
        #[arg(long, default_value_t = 512)]
        tile_size: u32,
        #[arg(long, value_enum, default_value_t = TransferSyntaxArg::Htj2kLosslessRpcl)]
        transfer_syntax: TransferSyntaxArg,
        #[arg(long, value_enum, default_value_t = CodecValidationArg::Disabled)]
        codec_validation: CodecValidationArg,
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
        source_device_decode: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
    Auto,
    Cpu,
    PreferDevice,
    RequireDevice,
}

impl BackendArg {
    fn into_preference(self) -> EncodeBackendPreference {
        match self {
            Self::Auto => EncodeBackendPreference::Auto,
            Self::Cpu => EncodeBackendPreference::CpuOnly,
            Self::PreferDevice => EncodeBackendPreference::PreferDevice,
            Self::RequireDevice => EncodeBackendPreference::RequireDevice,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TransferSyntaxArg {
    JpegBaseline8Bit,
    Jpeg2000,
    Jpeg2000Lossless,
    Htj2kLossless,
    Htj2kLosslessRpcl,
}

impl TransferSyntaxArg {
    fn into_transfer_syntax(self) -> TransferSyntax {
        match self {
            Self::JpegBaseline8Bit => TransferSyntax::JpegBaseline8Bit,
            Self::Jpeg2000 => TransferSyntax::Jpeg2000,
            Self::Jpeg2000Lossless => TransferSyntax::Jpeg2000Lossless,
            Self::Htj2kLossless => TransferSyntax::Htj2kLossless,
            Self::Htj2kLosslessRpcl => TransferSyntax::Htj2kLosslessRpcl,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CodecValidationArg {
    Disabled,
    RoundTrip,
}

impl CodecValidationArg {
    fn into_codec_validation(self) -> CodecValidation {
        match self {
            Self::Disabled => CodecValidation::Disabled,
            Self::RoundTrip => CodecValidation::RoundTrip,
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), WsiDicomError> {
    match Cli::parse().command {
        Command::Convert {
            source,
            out,
            metadata,
            research_placeholder,
            backend,
            tile_size,
            transfer_syntax,
            codec_validation,
            source_device_decode,
            level,
            json,
        } => {
            let metadata = load_metadata_source(metadata, research_placeholder)?;
            let report = export_dicom(DicomExportRequest {
                source_path: source,
                output_dir: out,
                options: DicomExportOptions {
                    tile_size,
                    transfer_syntax: transfer_syntax.into_transfer_syntax(),
                    encode_backend: backend.into_preference(),
                    codec_validation: codec_validation.into_codec_validation(),
                    source_device_decode,
                },
                metadata,
                level_filter: level,
            })?;
            if json {
                print_json_line(&report)?;
            } else {
                println!("{}", format_report_summary(&report));
            }
            Ok(())
        }
        Command::Profile {
            source,
            backend,
            tile_size,
            transfer_syntax,
            codec_validation,
            level,
            max_frames,
            source_device_decode,
            json,
        } => {
            let report = profile_dicom_routes(DicomRouteProfileRequest {
                source_path: source,
                options: DicomExportOptions {
                    tile_size,
                    transfer_syntax: transfer_syntax.into_transfer_syntax(),
                    encode_backend: backend.into_preference(),
                    codec_validation: codec_validation.into_codec_validation(),
                    source_device_decode,
                },
                level,
                max_frames,
            })?;
            if json {
                print_json_line(&report)?;
            } else {
                println!("{}", format_profile_summary(&report));
            }
            Ok(())
        }
        Command::Coverage {
            source,
            backend,
            tile_size,
            transfer_syntax,
            codec_validation,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            source_device_decode,
            json,
        } => {
            let max_frames_per_level =
                effective_max_frames_per_level(max_frames_per_level, full_frame_coverage);
            let max_level_elapsed = max_level_elapsed_from_ms(max_level_ms)?;
            let report = profile_dicom_route_coverage(DicomRouteCoverageRequest {
                source_path: source,
                options: DicomExportOptions {
                    tile_size,
                    transfer_syntax: transfer_syntax.into_transfer_syntax(),
                    encode_backend: backend.into_preference(),
                    codec_validation: codec_validation.into_codec_validation(),
                    source_device_decode,
                },
                max_frames_per_level,
                max_levels,
                max_level_elapsed,
                progress: (!json).then_some(DicomRouteCoverageProgress::Stderr),
            })?;
            if json {
                print_json_line(&report)?;
            } else {
                println!("{}", format_coverage_summary(&report));
            }
            Ok(())
        }
        Command::CoverageCorpus {
            root,
            backend,
            tile_size,
            transfer_syntax,
            codec_validation,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            source_device_decode,
            json,
        } => {
            let max_frames_per_level =
                effective_max_frames_per_level(max_frames_per_level, full_frame_coverage);
            let max_level_elapsed = max_level_elapsed_from_ms(max_level_ms)?;
            let report = profile_dicom_route_corpus_coverage(DicomRouteCorpusCoverageRequest {
                source_root: root,
                options: DicomExportOptions {
                    tile_size,
                    transfer_syntax: transfer_syntax.into_transfer_syntax(),
                    encode_backend: backend.into_preference(),
                    codec_validation: codec_validation.into_codec_validation(),
                    source_device_decode,
                },
                max_frames_per_level,
                max_levels,
                max_level_elapsed,
                progress: (!json).then_some(DicomRouteCorpusCoverageProgress::Stderr),
            })?;
            if json {
                print_json_line(&report)?;
            } else {
                println!("{}", format_corpus_coverage_summary(&report));
            }
            Ok(())
        }
        Command::SustainConvert {
            source,
            out,
            metadata,
            research_placeholder,
            backend,
            tile_size,
            transfer_syntax,
            codec_validation,
            source_device_decode,
            level,
            iterations,
            interval_ms,
            json,
        } => {
            if iterations == 0 {
                return Err(WsiDicomError::Unsupported {
                    reason: "sustain-convert requires iterations > 0".into(),
                });
            }
            let metadata = load_metadata_source(metadata, research_placeholder)?;
            let options = DicomExportOptions {
                tile_size,
                transfer_syntax: transfer_syntax.into_transfer_syntax(),
                encode_backend: backend.into_preference(),
                codec_validation: codec_validation.into_codec_validation(),
                source_device_decode,
            };
            for iteration in 1..=iterations {
                let output_dir = out.join(format!("iteration-{iteration:04}"));
                let started = std::time::Instant::now();
                let report = export_dicom(DicomExportRequest {
                    source_path: source.clone(),
                    output_dir,
                    options: options.clone(),
                    metadata: metadata.clone(),
                    level_filter: level,
                })?;
                let elapsed_micros = duration_as_reported_micros(started.elapsed());
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
                if interval_ms > 0 && iteration < iterations {
                    std::thread::sleep(std::time::Duration::from_millis(interval_ms));
                }
            }
            Ok(())
        }
        Command::Sustain {
            source,
            backend,
            tile_size,
            transfer_syntax,
            codec_validation,
            max_frames_per_level,
            full_frame_coverage,
            max_levels,
            max_level_ms,
            iterations,
            interval_ms,
            source_device_decode,
            json,
        } => {
            if iterations == 0 {
                return Err(WsiDicomError::Unsupported {
                    reason: "sustain requires iterations > 0".into(),
                });
            }
            let options = DicomExportOptions {
                tile_size,
                transfer_syntax: transfer_syntax.into_transfer_syntax(),
                encode_backend: backend.into_preference(),
                codec_validation: codec_validation.into_codec_validation(),
                source_device_decode,
            };
            let max_frames_per_level =
                effective_max_frames_per_level(max_frames_per_level, full_frame_coverage);
            let max_level_elapsed = max_level_elapsed_from_ms(max_level_ms)?;
            for iteration in 1..=iterations {
                let report = profile_dicom_route_coverage(DicomRouteCoverageRequest {
                    source_path: source.clone(),
                    options: options.clone(),
                    max_frames_per_level,
                    max_levels,
                    max_level_elapsed,
                    progress: None,
                })?;
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
                if interval_ms > 0 && iteration < iterations {
                    std::thread::sleep(std::time::Duration::from_millis(interval_ms));
                }
            }
            Ok(())
        }
    }
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
    report: &'a DicomExportReport,
}

#[derive(serde::Serialize)]
struct SustainCoverageIterationJson<'a> {
    mode: &'static str,
    iteration: u32,
    iterations: u32,
    rss_bytes: Option<u64>,
    thermal_state: Option<&'a str>,
    memory_pressure: Option<&'a str>,
    report: &'a DicomRouteCoverageReport,
}

fn print_json_line<T: serde::Serialize>(value: &T) -> Result<(), WsiDicomError> {
    let json = serde_json::to_string(value).map_err(|source| WsiDicomError::JsonSerialize {
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

fn max_level_elapsed_from_ms(max_level_ms: Option<u64>) -> Result<Option<Duration>, WsiDicomError> {
    match max_level_ms {
        Some(0) => Err(WsiDicomError::Unsupported {
            reason: "--max-level-ms must be greater than 0 when provided".into(),
        }),
        Some(max_level_ms) => Ok(Some(Duration::from_millis(max_level_ms))),
        None => Ok(None),
    }
}

fn format_requested_frames_per_level(max_frames_per_level: u64) -> String {
    if max_frames_per_level == u64::MAX {
        "all".into()
    } else {
        max_frames_per_level.to_string()
    }
}

fn format_report_summary(report: &DicomExportReport) -> String {
    format_report_summary_with_memory(report, process_resident_memory_bytes())
}

fn format_report_summary_with_memory(report: &DicomExportReport, rss_bytes: Option<u64>) -> String {
    let metrics = report.metrics;
    let route_passthrough = metrics.route_passthrough_frames();
    let route_unclassified = metrics.route_unclassified_frames();
    format!(
        "wrote {} DICOM instance(s) to {}; frames total={} route_passthrough={} route_passthrough_pct={:.1} route_gpu_transcode={} route_gpu_transcode_pct={:.1} route_resident_gpu_transcode={} route_partial_gpu_transcode={} route_cpu_fallback={} route_cpu_fallback_pct={:.1} route_unclassified={} cpu_input={} gpu_input_decode={} gpu_encode={} gpu_validation={} gray_frames={} rgb_like_frames={} other_component_frames={} unknown_pixel_profile_frames={} bits8_frames={} bits16_frames={} other_bit_depth_frames={} gpu_input_batches={} gpu_compose_batches={} gpu_encode_batches={} gpu_dispatch_ms={:.3} gpu_encode_hardware_ms={:.3} gpu_encode_dispatch_overhead_ms={:.3} auto_probe_frames={} auto_probe_selected_gpu_input={} auto_probe_gpu_batches={} auto_probe_cpu_ms={:.3} auto_probe_gpu_ms={:.3} jpeg_passthrough={} j2k_passthrough={} jpeg_decode_fallback={} jpeg_cpu_encode={} jpeg_metal_encode={} input_decode_ms={:.3} compose_ms={:.3} encode_ms={:.3} validation_ms={:.3} write_ms={:.3} rss_mb={}",
        report.instances.len(),
        report.output_dir.display(),
        metrics.total_frames,
        route_passthrough,
        frame_percent(route_passthrough, metrics.total_frames),
        metrics.gpu_transcode_frames,
        frame_percent(metrics.gpu_transcode_frames, metrics.total_frames),
        metrics.resident_gpu_transcode_frames,
        metrics.partial_gpu_transcode_frames,
        metrics.cpu_fallback_frames,
        frame_percent(metrics.cpu_fallback_frames, metrics.total_frames),
        route_unclassified,
        metrics.cpu_input_frames,
        metrics.gpu_input_decode_frames,
        metrics.gpu_encode_frames,
        metrics.gpu_validation_frames,
        metrics.gray_frames,
        metrics.rgb_like_frames,
        metrics.other_component_frames,
        metrics.unknown_pixel_profile_frames,
        metrics.bits8_frames,
        metrics.bits16_frames,
        metrics.other_bit_depth_frames,
        metrics.gpu_input_decode_batches,
        metrics.gpu_compose_batches,
        metrics.gpu_encode_batches,
        micros_to_ms(metrics.gpu_dispatch_micros),
        micros_to_ms(metrics.gpu_encode_hardware_micros),
        micros_to_ms(metrics.gpu_encode_dispatch_overhead_micros),
        metrics.auto_route_probe_frames,
        metrics.auto_route_probe_selected_gpu_input_frames,
        metrics.auto_route_probe_gpu_batches,
        micros_to_ms(metrics.auto_route_probe_cpu_micros),
        micros_to_ms(metrics.auto_route_probe_gpu_micros),
        metrics.jpeg_passthrough_frames,
        metrics.j2k_passthrough_frames,
        metrics.jpeg_decode_fallback_frames,
        metrics.jpeg_cpu_encode_frames,
        metrics.jpeg_metal_encode_frames,
        micros_to_ms(metrics.input_decode_micros),
        micros_to_ms(metrics.compose_micros),
        micros_to_ms(metrics.encode_micros),
        micros_to_ms(metrics.validation_micros),
        micros_to_ms(metrics.write_micros),
        format_rss_mb(rss_bytes),
    )
}

fn micros_to_ms(micros: u128) -> f64 {
    micros as f64 / 1_000.0
}

fn format_profile_summary(report: &DicomRouteProfileReport) -> String {
    format_profile_summary_with_memory(report, process_resident_memory_bytes())
}

fn format_profile_summary_with_memory(
    report: &DicomRouteProfileReport,
    rss_bytes: Option<u64>,
) -> String {
    let metrics = report.metrics;
    let route_passthrough = metrics.route_passthrough_frames();
    let route_unclassified = metrics.route_unclassified_frames();
    format!(
        "profiled {} level={} transfer_syntax={} requested_frames={} available_frames={} sampled_frames_pct={:.4} frames total={} route_passthrough={} route_passthrough_pct={:.1} route_gpu_transcode={} route_gpu_transcode_pct={:.1} route_resident_gpu_transcode={} route_partial_gpu_transcode={} route_cpu_fallback={} route_cpu_fallback_pct={:.1} route_unclassified={} cpu_input={} gpu_input_decode={} gpu_encode={} gpu_validation={} gray_frames={} rgb_like_frames={} other_component_frames={} unknown_pixel_profile_frames={} bits8_frames={} bits16_frames={} other_bit_depth_frames={} gpu_input_batches={} gpu_compose_batches={} gpu_encode_batches={} gpu_dispatch_ms={:.3} gpu_encode_hardware_ms={:.3} gpu_encode_dispatch_overhead_ms={:.3} auto_probe_frames={} auto_probe_selected_gpu_input={} auto_probe_gpu_batches={} auto_probe_cpu_ms={:.3} auto_probe_gpu_ms={:.3} jpeg_passthrough={} j2k_passthrough={} jpeg_decode_fallback={} jpeg_cpu_encode={} jpeg_metal_encode={} final_byte_ms={:.3} input_decode_ms={:.3} compose_ms={:.3} encode_ms={:.3} validation_ms={:.3} elapsed_ms={:.3} rss_mb={}",
        report.source_path.display(),
        report.level,
        report.transfer_syntax_uid,
        report.requested_frames,
        report.available_frames,
        frame_percent(metrics.total_frames, report.available_frames),
        metrics.total_frames,
        route_passthrough,
        frame_percent(route_passthrough, metrics.total_frames),
        metrics.gpu_transcode_frames,
        frame_percent(metrics.gpu_transcode_frames, metrics.total_frames),
        metrics.resident_gpu_transcode_frames,
        metrics.partial_gpu_transcode_frames,
        metrics.cpu_fallback_frames,
        frame_percent(metrics.cpu_fallback_frames, metrics.total_frames),
        route_unclassified,
        metrics.cpu_input_frames,
        metrics.gpu_input_decode_frames,
        metrics.gpu_encode_frames,
        metrics.gpu_validation_frames,
        metrics.gray_frames,
        metrics.rgb_like_frames,
        metrics.other_component_frames,
        metrics.unknown_pixel_profile_frames,
        metrics.bits8_frames,
        metrics.bits16_frames,
        metrics.other_bit_depth_frames,
        metrics.gpu_input_decode_batches,
        metrics.gpu_compose_batches,
        metrics.gpu_encode_batches,
        micros_to_ms(metrics.gpu_dispatch_micros),
        micros_to_ms(metrics.gpu_encode_hardware_micros),
        micros_to_ms(metrics.gpu_encode_dispatch_overhead_micros),
        metrics.auto_route_probe_frames,
        metrics.auto_route_probe_selected_gpu_input_frames,
        metrics.auto_route_probe_gpu_batches,
        micros_to_ms(metrics.auto_route_probe_cpu_micros),
        micros_to_ms(metrics.auto_route_probe_gpu_micros),
        metrics.jpeg_passthrough_frames,
        metrics.j2k_passthrough_frames,
        metrics.jpeg_decode_fallback_frames,
        metrics.jpeg_cpu_encode_frames,
        metrics.jpeg_metal_encode_frames,
        micros_to_ms(metrics.write_micros),
        micros_to_ms(metrics.input_decode_micros),
        micros_to_ms(metrics.compose_micros),
        micros_to_ms(metrics.encode_micros),
        micros_to_ms(metrics.validation_micros),
        micros_to_ms(report.elapsed_micros),
        format_rss_mb(rss_bytes),
    )
}

fn format_coverage_summary(report: &DicomRouteCoverageReport) -> String {
    format_coverage_summary_with_memory(report, process_resident_memory_bytes())
}

fn format_coverage_summary_with_memory(
    report: &DicomRouteCoverageReport,
    rss_bytes: Option<u64>,
) -> String {
    let metrics = report.metrics;
    let route_passthrough = metrics.route_passthrough_frames();
    let route_unclassified = metrics.route_unclassified_frames();
    format!(
        "covered {} levels={} transfer_syntax={} requested_frames_per_level={} available_frames={} sampled_frames_pct={:.4} complete_frame_coverage={} frames total={} route_passthrough={} route_passthrough_pct={:.1} route_gpu_transcode={} route_gpu_transcode_pct={:.1} route_resident_gpu_transcode={} route_partial_gpu_transcode={} route_cpu_fallback={} route_cpu_fallback_pct={:.1} route_unclassified={} cpu_input={} gpu_input_decode={} gpu_encode={} gpu_validation={} gray_frames={} rgb_like_frames={} other_component_frames={} unknown_pixel_profile_frames={} bits8_frames={} bits16_frames={} other_bit_depth_frames={} gpu_input_batches={} gpu_compose_batches={} gpu_encode_batches={} gpu_dispatch_ms={:.3} gpu_encode_hardware_ms={:.3} gpu_encode_dispatch_overhead_ms={:.3} auto_probe_frames={} auto_probe_selected_gpu_input={} auto_probe_gpu_batches={} auto_probe_cpu_ms={:.3} auto_probe_gpu_ms={:.3} jpeg_passthrough={} j2k_passthrough={} jpeg_decode_fallback={} jpeg_cpu_encode={} jpeg_metal_encode={} final_byte_ms={:.3} input_decode_ms={:.3} compose_ms={:.3} encode_ms={:.3} validation_ms={:.3} elapsed_ms={:.3} rss_mb={}",
        report.source_path.display(),
        report.levels.len(),
        report.transfer_syntax_uid,
        format_requested_frames_per_level(report.requested_frames_per_level),
        report.available_frames,
        frame_percent(metrics.total_frames, report.available_frames),
        report.complete_frame_coverage,
        metrics.total_frames,
        route_passthrough,
        frame_percent(route_passthrough, metrics.total_frames),
        metrics.gpu_transcode_frames,
        frame_percent(metrics.gpu_transcode_frames, metrics.total_frames),
        metrics.resident_gpu_transcode_frames,
        metrics.partial_gpu_transcode_frames,
        metrics.cpu_fallback_frames,
        frame_percent(metrics.cpu_fallback_frames, metrics.total_frames),
        route_unclassified,
        metrics.cpu_input_frames,
        metrics.gpu_input_decode_frames,
        metrics.gpu_encode_frames,
        metrics.gpu_validation_frames,
        metrics.gray_frames,
        metrics.rgb_like_frames,
        metrics.other_component_frames,
        metrics.unknown_pixel_profile_frames,
        metrics.bits8_frames,
        metrics.bits16_frames,
        metrics.other_bit_depth_frames,
        metrics.gpu_input_decode_batches,
        metrics.gpu_compose_batches,
        metrics.gpu_encode_batches,
        micros_to_ms(metrics.gpu_dispatch_micros),
        micros_to_ms(metrics.gpu_encode_hardware_micros),
        micros_to_ms(metrics.gpu_encode_dispatch_overhead_micros),
        metrics.auto_route_probe_frames,
        metrics.auto_route_probe_selected_gpu_input_frames,
        metrics.auto_route_probe_gpu_batches,
        micros_to_ms(metrics.auto_route_probe_cpu_micros),
        micros_to_ms(metrics.auto_route_probe_gpu_micros),
        metrics.jpeg_passthrough_frames,
        metrics.j2k_passthrough_frames,
        metrics.jpeg_decode_fallback_frames,
        metrics.jpeg_cpu_encode_frames,
        metrics.jpeg_metal_encode_frames,
        micros_to_ms(metrics.write_micros),
        micros_to_ms(metrics.input_decode_micros),
        micros_to_ms(metrics.compose_micros),
        micros_to_ms(metrics.encode_micros),
        micros_to_ms(metrics.validation_micros),
        micros_to_ms(report.elapsed_micros),
        format_rss_mb(rss_bytes),
    )
}

fn format_corpus_coverage_summary(report: &DicomRouteCorpusCoverageReport) -> String {
    format_corpus_coverage_summary_with_memory(report, process_resident_memory_bytes())
}

fn format_corpus_coverage_summary_with_memory(
    report: &DicomRouteCorpusCoverageReport,
    rss_bytes: Option<u64>,
) -> String {
    let metrics = report.metrics;
    let route_passthrough = metrics.route_passthrough_frames();
    let route_unclassified = metrics.route_unclassified_frames();
    format!(
        "covered_corpus {} sources_considered={} sources_profiled={} failures={} transfer_syntax={} requested_frames_per_level={} available_frames={} sampled_frames_pct={:.4} complete_frame_coverage={} frames total={} route_passthrough={} route_passthrough_pct={:.1} route_gpu_transcode={} route_gpu_transcode_pct={:.1} route_resident_gpu_transcode={} route_partial_gpu_transcode={} route_cpu_fallback={} route_cpu_fallback_pct={:.1} route_unclassified={} cpu_input={} gpu_input_decode={} gpu_encode={} gpu_validation={} gray_frames={} rgb_like_frames={} other_component_frames={} unknown_pixel_profile_frames={} bits8_frames={} bits16_frames={} other_bit_depth_frames={} gpu_input_batches={} gpu_compose_batches={} gpu_encode_batches={} gpu_dispatch_ms={:.3} gpu_encode_hardware_ms={:.3} gpu_encode_dispatch_overhead_ms={:.3} auto_probe_frames={} auto_probe_selected_gpu_input={} auto_probe_gpu_batches={} auto_probe_cpu_ms={:.3} auto_probe_gpu_ms={:.3} jpeg_passthrough={} j2k_passthrough={} jpeg_decode_fallback={} jpeg_cpu_encode={} jpeg_metal_encode={} final_byte_ms={:.3} input_decode_ms={:.3} compose_ms={:.3} encode_ms={:.3} validation_ms={:.3} elapsed_ms={:.3} rss_mb={}",
        report.source_root.display(),
        report.sources_considered,
        report.reports.len(),
        report.failures.len(),
        report.transfer_syntax_uid,
        format_requested_frames_per_level(report.requested_frames_per_level),
        report.available_frames,
        frame_percent(metrics.total_frames, report.available_frames),
        report.complete_frame_coverage,
        metrics.total_frames,
        route_passthrough,
        frame_percent(route_passthrough, metrics.total_frames),
        metrics.gpu_transcode_frames,
        frame_percent(metrics.gpu_transcode_frames, metrics.total_frames),
        metrics.resident_gpu_transcode_frames,
        metrics.partial_gpu_transcode_frames,
        metrics.cpu_fallback_frames,
        frame_percent(metrics.cpu_fallback_frames, metrics.total_frames),
        route_unclassified,
        metrics.cpu_input_frames,
        metrics.gpu_input_decode_frames,
        metrics.gpu_encode_frames,
        metrics.gpu_validation_frames,
        metrics.gray_frames,
        metrics.rgb_like_frames,
        metrics.other_component_frames,
        metrics.unknown_pixel_profile_frames,
        metrics.bits8_frames,
        metrics.bits16_frames,
        metrics.other_bit_depth_frames,
        metrics.gpu_input_decode_batches,
        metrics.gpu_compose_batches,
        metrics.gpu_encode_batches,
        micros_to_ms(metrics.gpu_dispatch_micros),
        micros_to_ms(metrics.gpu_encode_hardware_micros),
        micros_to_ms(metrics.gpu_encode_dispatch_overhead_micros),
        metrics.auto_route_probe_frames,
        metrics.auto_route_probe_selected_gpu_input_frames,
        metrics.auto_route_probe_gpu_batches,
        micros_to_ms(metrics.auto_route_probe_cpu_micros),
        micros_to_ms(metrics.auto_route_probe_gpu_micros),
        metrics.jpeg_passthrough_frames,
        metrics.j2k_passthrough_frames,
        metrics.jpeg_decode_fallback_frames,
        metrics.jpeg_cpu_encode_frames,
        metrics.jpeg_metal_encode_frames,
        micros_to_ms(metrics.write_micros),
        micros_to_ms(metrics.input_decode_micros),
        micros_to_ms(metrics.compose_micros),
        micros_to_ms(metrics.encode_micros),
        micros_to_ms(metrics.validation_micros),
        micros_to_ms(report.elapsed_micros),
        format_rss_mb(rss_bytes),
    )
}

fn format_sustain_export_iteration_summary(
    iteration: u32,
    iterations: u32,
    report: &DicomExportReport,
    elapsed_micros: u128,
    rss_bytes: Option<u64>,
    thermal_state: Option<&str>,
    memory_pressure: Option<&str>,
) -> String {
    let metrics = report.metrics;
    let route_passthrough = metrics.route_passthrough_frames();
    let route_unclassified = metrics.route_unclassified_frames();
    let elapsed_seconds = elapsed_micros as f64 / 1_000_000.0;
    let frames_per_sec = if elapsed_seconds > 0.0 {
        metrics.total_frames as f64 / elapsed_seconds
    } else {
        0.0
    };
    let thermal_state = thermal_state
        .map(escape_summary_value)
        .unwrap_or_else(|| "unknown".into());
    let memory_pressure = memory_pressure
        .map(escape_summary_value)
        .unwrap_or_else(|| "unknown".into());
    format!(
        "sustain_iteration={}/{} mode=convert output={} instances={} frames={} frames_per_sec={:.2} route_passthrough={} route_passthrough_pct={:.1} route_gpu_transcode={} route_gpu_transcode_pct={:.1} route_resident_gpu_transcode={} route_partial_gpu_transcode={} route_cpu_fallback={} route_cpu_fallback_pct={:.1} route_unclassified={} cpu_input={} gpu_input_decode={} gpu_encode={} gpu_validation={} gray_frames={} rgb_like_frames={} other_component_frames={} unknown_pixel_profile_frames={} bits8_frames={} bits16_frames={} other_bit_depth_frames={} gpu_input_batches={} gpu_compose_batches={} gpu_encode_batches={} gpu_dispatch_ms={:.3} gpu_encode_hardware_ms={:.3} gpu_encode_dispatch_overhead_ms={:.3} auto_probe_frames={} auto_probe_selected_gpu_input={} auto_probe_gpu_batches={} auto_probe_cpu_ms={:.3} auto_probe_gpu_ms={:.3} jpeg_passthrough={} j2k_passthrough={} jpeg_decode_fallback={} jpeg_cpu_encode={} jpeg_metal_encode={} final_byte_ms={:.3} input_decode_ms={:.3} compose_ms={:.3} encode_ms={:.3} validation_ms={:.3} elapsed_ms={:.3} rss_mb={} thermal=\"{}\" memory_pressure=\"{}\"",
        iteration,
        iterations,
        report.output_dir.display(),
        report.instances.len(),
        metrics.total_frames,
        frames_per_sec,
        route_passthrough,
        frame_percent(route_passthrough, metrics.total_frames),
        metrics.gpu_transcode_frames,
        frame_percent(metrics.gpu_transcode_frames, metrics.total_frames),
        metrics.resident_gpu_transcode_frames,
        metrics.partial_gpu_transcode_frames,
        metrics.cpu_fallback_frames,
        frame_percent(metrics.cpu_fallback_frames, metrics.total_frames),
        route_unclassified,
        metrics.cpu_input_frames,
        metrics.gpu_input_decode_frames,
        metrics.gpu_encode_frames,
        metrics.gpu_validation_frames,
        metrics.gray_frames,
        metrics.rgb_like_frames,
        metrics.other_component_frames,
        metrics.unknown_pixel_profile_frames,
        metrics.bits8_frames,
        metrics.bits16_frames,
        metrics.other_bit_depth_frames,
        metrics.gpu_input_decode_batches,
        metrics.gpu_compose_batches,
        metrics.gpu_encode_batches,
        micros_to_ms(metrics.gpu_dispatch_micros),
        micros_to_ms(metrics.gpu_encode_hardware_micros),
        micros_to_ms(metrics.gpu_encode_dispatch_overhead_micros),
        metrics.auto_route_probe_frames,
        metrics.auto_route_probe_selected_gpu_input_frames,
        metrics.auto_route_probe_gpu_batches,
        micros_to_ms(metrics.auto_route_probe_cpu_micros),
        micros_to_ms(metrics.auto_route_probe_gpu_micros),
        metrics.jpeg_passthrough_frames,
        metrics.j2k_passthrough_frames,
        metrics.jpeg_decode_fallback_frames,
        metrics.jpeg_cpu_encode_frames,
        metrics.jpeg_metal_encode_frames,
        micros_to_ms(metrics.write_micros),
        micros_to_ms(metrics.input_decode_micros),
        micros_to_ms(metrics.compose_micros),
        micros_to_ms(metrics.encode_micros),
        micros_to_ms(metrics.validation_micros),
        micros_to_ms(elapsed_micros),
        format_rss_mb(rss_bytes),
        thermal_state,
        memory_pressure,
    )
}

fn format_sustain_iteration_summary(
    iteration: u32,
    iterations: u32,
    report: &DicomRouteCoverageReport,
    rss_bytes: Option<u64>,
    thermal_state: Option<&str>,
    memory_pressure: Option<&str>,
) -> String {
    let metrics = report.metrics;
    let route_passthrough = metrics.route_passthrough_frames();
    let route_unclassified = metrics.route_unclassified_frames();
    let elapsed_seconds = report.elapsed_micros as f64 / 1_000_000.0;
    let frames_per_sec = if elapsed_seconds > 0.0 {
        metrics.total_frames as f64 / elapsed_seconds
    } else {
        0.0
    };
    let thermal_state = thermal_state
        .map(escape_summary_value)
        .unwrap_or_else(|| "unknown".into());
    let memory_pressure = memory_pressure
        .map(escape_summary_value)
        .unwrap_or_else(|| "unknown".into());
    format!(
        "sustain_iteration={}/{} source={} transfer_syntax={} frames={} available_frames={} sampled_frames_pct={:.4} complete_frame_coverage={} frames_per_sec={:.2} route_passthrough={} route_passthrough_pct={:.1} route_gpu_transcode={} route_gpu_transcode_pct={:.1} route_resident_gpu_transcode={} route_partial_gpu_transcode={} route_cpu_fallback={} route_cpu_fallback_pct={:.1} route_unclassified={} cpu_input={} gpu_input_decode={} gpu_encode={} gpu_validation={} gray_frames={} rgb_like_frames={} other_component_frames={} unknown_pixel_profile_frames={} bits8_frames={} bits16_frames={} other_bit_depth_frames={} gpu_input_batches={} gpu_compose_batches={} gpu_encode_batches={} gpu_dispatch_ms={:.3} gpu_encode_hardware_ms={:.3} gpu_encode_dispatch_overhead_ms={:.3} auto_probe_frames={} auto_probe_selected_gpu_input={} auto_probe_gpu_batches={} auto_probe_cpu_ms={:.3} auto_probe_gpu_ms={:.3} jpeg_passthrough={} j2k_passthrough={} jpeg_decode_fallback={} jpeg_cpu_encode={} jpeg_metal_encode={} final_byte_ms={:.3} input_decode_ms={:.3} compose_ms={:.3} encode_ms={:.3} validation_ms={:.3} elapsed_ms={:.3} rss_mb={} thermal=\"{}\" memory_pressure=\"{}\"",
        iteration,
        iterations,
        report.source_path.display(),
        report.transfer_syntax_uid,
        metrics.total_frames,
        report.available_frames,
        frame_percent(metrics.total_frames, report.available_frames),
        report.complete_frame_coverage,
        frames_per_sec,
        route_passthrough,
        frame_percent(route_passthrough, metrics.total_frames),
        metrics.gpu_transcode_frames,
        frame_percent(metrics.gpu_transcode_frames, metrics.total_frames),
        metrics.resident_gpu_transcode_frames,
        metrics.partial_gpu_transcode_frames,
        metrics.cpu_fallback_frames,
        frame_percent(metrics.cpu_fallback_frames, metrics.total_frames),
        route_unclassified,
        metrics.cpu_input_frames,
        metrics.gpu_input_decode_frames,
        metrics.gpu_encode_frames,
        metrics.gpu_validation_frames,
        metrics.gray_frames,
        metrics.rgb_like_frames,
        metrics.other_component_frames,
        metrics.unknown_pixel_profile_frames,
        metrics.bits8_frames,
        metrics.bits16_frames,
        metrics.other_bit_depth_frames,
        metrics.gpu_input_decode_batches,
        metrics.gpu_compose_batches,
        metrics.gpu_encode_batches,
        micros_to_ms(metrics.gpu_dispatch_micros),
        micros_to_ms(metrics.gpu_encode_hardware_micros),
        micros_to_ms(metrics.gpu_encode_dispatch_overhead_micros),
        metrics.auto_route_probe_frames,
        metrics.auto_route_probe_selected_gpu_input_frames,
        metrics.auto_route_probe_gpu_batches,
        micros_to_ms(metrics.auto_route_probe_cpu_micros),
        micros_to_ms(metrics.auto_route_probe_gpu_micros),
        metrics.jpeg_passthrough_frames,
        metrics.j2k_passthrough_frames,
        metrics.jpeg_decode_fallback_frames,
        metrics.jpeg_cpu_encode_frames,
        metrics.jpeg_metal_encode_frames,
        micros_to_ms(metrics.write_micros),
        micros_to_ms(metrics.input_decode_micros),
        micros_to_ms(metrics.compose_micros),
        micros_to_ms(metrics.encode_micros),
        micros_to_ms(metrics.validation_micros),
        micros_to_ms(report.elapsed_micros),
        format_rss_mb(rss_bytes),
        thermal_state,
        memory_pressure,
    )
}

fn frame_percent(frames: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        frames as f64 * 100.0 / total as f64
    }
}

fn escape_summary_value(value: &str) -> String {
    value.replace('"', "'")
}

fn duration_as_reported_micros(duration: std::time::Duration) -> u128 {
    match duration.as_micros() {
        0 if duration > std::time::Duration::ZERO => 1,
        micros => micros,
    }
}

fn format_rss_mb(rss_bytes: Option<u64>) -> String {
    rss_bytes
        .map(|bytes| format!("{:.1}", bytes as f64 / (1024.0 * 1024.0)))
        .unwrap_or_else(|| "unknown".into())
}

fn process_thermal_state() -> Option<String> {
    let output = std::process::Command::new("pmset")
        .args(["-g", "therm"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let summary = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    (!summary.is_empty()).then_some(summary)
}

fn process_memory_pressure() -> Option<String> {
    let output = std::process::Command::new("memory_pressure")
        .arg("-Q")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    text.lines()
        .map(str::trim)
        .find(|line| line.starts_with("System-wide memory free percentage:"))
        .map(str::to_string)
}

fn process_resident_memory_bytes() -> Option<u64> {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let kib = text.trim().parse::<u64>().ok()?;
    kib.checked_mul(1024)
}

fn load_metadata_source(
    metadata_path: Option<PathBuf>,
    research_placeholder: bool,
) -> Result<MetadataSource, WsiDicomError> {
    if research_placeholder {
        return Ok(MetadataSource::ResearchPlaceholder);
    }

    let Some(path) = metadata_path else {
        return Ok(MetadataSource::ResearchPlaceholder);
    };

    let bytes = std::fs::read(&path).map_err(|source| WsiDicomError::Io {
        path: path.clone(),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|source| WsiDicomError::Json {
            path: path.clone(),
            source,
        })?;
    if looks_like_fhir(&value) {
        Ok(MetadataSource::FhirR4Bundle(value))
    } else {
        let metadata: DicomMetadata =
            serde_json::from_value(value).map_err(|source| WsiDicomError::Json {
                path: path.clone(),
                source,
            })?;
        Ok(MetadataSource::Strict(Box::new(metadata)))
    }
}

fn looks_like_fhir(value: &serde_json::Value) -> bool {
    matches!(
        value
            .get("resourceType")
            .and_then(serde_json::Value::as_str),
        Some("Bundle" | "Patient" | "Specimen" | "ServiceRequest" | "DiagnosticReport")
    )
}

#[cfg(test)]
mod tests {
    use super::{
        effective_max_frames_per_level, format_corpus_coverage_summary_with_memory,
        format_coverage_summary_with_memory, format_profile_summary_with_memory,
        format_report_summary_with_memory, format_sustain_export_iteration_summary,
        format_sustain_iteration_summary, load_metadata_source, max_level_elapsed_from_ms, Cli,
        Command,
    };
    use clap::Parser;
    use std::path::PathBuf;
    use wsi_dicom::MetadataSource;
    use wsi_dicom::{
        DicomExportMetrics, DicomExportReport, DicomRouteCorpusCoverageFailure,
        DicomRouteCorpusCoverageReport, DicomRouteCoverageReport, DicomRouteProfileReport,
    };

    #[test]
    fn cli_uses_research_placeholder_metadata_by_default() {
        let metadata = load_metadata_source(None, false).unwrap();

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

        let Command::Coverage {
            source_device_decode,
            ..
        } = cli.command
        else {
            panic!("expected coverage command");
        };

        assert!(source_device_decode);
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

        let Command::Convert {
            source_device_decode,
            ..
        } = cli.command
        else {
            panic!("expected convert command");
        };

        assert!(source_device_decode);
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
            &DicomExportReport {
                output_dir: PathBuf::from("out"),
                instances: Vec::new(),
                metrics: DicomExportMetrics {
                    total_frames: 27,
                    cpu_input_frames: 1,
                    gpu_input_decode_frames: 2,
                    gpu_encode_frames: 3,
                    gpu_validation_frames: 4,
                    jpeg_passthrough_frames: 5,
                    j2k_passthrough_frames: 6,
                    gpu_transcode_frames: 7,
                    resident_gpu_transcode_frames: 4,
                    partial_gpu_transcode_frames: 3,
                    gpu_input_decode_batches: 2,
                    gpu_compose_batches: 1,
                    gpu_encode_batches: 3,
                    gpu_dispatch_micros: 6_500,
                    gpu_encode_hardware_micros: 2_000,
                    gpu_encode_dispatch_overhead_micros: 4_500,
                    cpu_fallback_frames: 9,
                    jpeg_decode_fallback_frames: 1,
                    jpeg_cpu_encode_frames: 1,
                    jpeg_metal_encode_frames: 2,
                    ..DicomExportMetrics::default()
                },
            },
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
        assert!(summary.contains("gpu_dispatch_ms=6.500"));
        assert!(summary.contains("gpu_encode_hardware_ms=2.000"));
        assert!(summary.contains("gpu_encode_dispatch_overhead_ms=4.500"));
        assert!(summary.contains("route_cpu_fallback=9"));
        assert!(summary.contains("route_cpu_fallback_pct=33.3"));
        assert!(summary.contains("route_unclassified=0"));
        assert!(summary.contains("rss_mb=10.0"));
    }

    #[test]
    fn cli_profile_summary_reports_bounded_route_counts() {
        let summary = format_profile_summary_with_memory(
            &DicomRouteProfileReport {
                source_path: PathBuf::from("source.svs"),
                transfer_syntax_uid: "1.2.840.10008.1.2.4.202",
                level: 2,
                requested_frames: 12,
                available_frames: 20,
                metrics: DicomExportMetrics {
                    total_frames: 10,
                    gpu_transcode_frames: 7,
                    resident_gpu_transcode_frames: 4,
                    partial_gpu_transcode_frames: 3,
                    cpu_fallback_frames: 3,
                    gpu_input_decode_frames: 7,
                    gpu_encode_frames: 7,
                    jpeg_passthrough_frames: 2,
                    jpeg_decode_fallback_frames: 1,
                    jpeg_cpu_encode_frames: 1,
                    jpeg_metal_encode_frames: 2,
                    auto_route_probe_frames: 2,
                    auto_route_probe_gpu_batches: 3,
                    auto_route_probe_cpu_micros: 1_200,
                    auto_route_probe_gpu_micros: 1_300,
                    gpu_dispatch_micros: 6_500,
                    write_micros: 1_250,
                    ..DicomExportMetrics::default()
                },
                elapsed_micros: 42_500,
            },
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
    }

    #[test]
    fn cli_coverage_summary_reports_aggregate_route_counts() {
        let summary = format_coverage_summary_with_memory(
            &DicomRouteCoverageReport {
                source_path: PathBuf::from("source.ndpi"),
                transfer_syntax_uid: "1.2.840.10008.1.2.4.50",
                requested_frames_per_level: 8,
                available_frames: 20,
                complete_frame_coverage: false,
                levels: vec![
                    DicomRouteProfileReport {
                        source_path: PathBuf::from("source.ndpi"),
                        transfer_syntax_uid: "1.2.840.10008.1.2.4.50",
                        level: 0,
                        requested_frames: 8,
                        available_frames: 16,
                        metrics: DicomExportMetrics {
                            total_frames: 8,
                            jpeg_passthrough_frames: 8,
                            ..DicomExportMetrics::default()
                        },
                        elapsed_micros: 1_000,
                    },
                    DicomRouteProfileReport {
                        source_path: PathBuf::from("source.ndpi"),
                        transfer_syntax_uid: "1.2.840.10008.1.2.4.50",
                        level: 1,
                        requested_frames: 8,
                        available_frames: 4,
                        metrics: DicomExportMetrics {
                            total_frames: 4,
                            cpu_fallback_frames: 4,
                            jpeg_decode_fallback_frames: 4,
                            jpeg_cpu_encode_frames: 4,
                            ..DicomExportMetrics::default()
                        },
                        elapsed_micros: 2_000,
                    },
                ],
                metrics: DicomExportMetrics {
                    total_frames: 12,
                    jpeg_passthrough_frames: 8,
                    cpu_fallback_frames: 4,
                    jpeg_decode_fallback_frames: 4,
                    jpeg_cpu_encode_frames: 4,
                    input_decode_micros: 3_000,
                    encode_micros: 4_000,
                    gpu_dispatch_micros: 7_000,
                    ..DicomExportMetrics::default()
                },
                elapsed_micros: 5_000,
            },
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
    }

    #[test]
    fn cli_coverage_summary_formats_full_frame_coverage_request_as_all() {
        let summary = format_coverage_summary_with_memory(
            &DicomRouteCoverageReport {
                source_path: PathBuf::from("source.svs"),
                transfer_syntax_uid: "1.2.840.10008.1.2.4.202",
                requested_frames_per_level: u64::MAX,
                available_frames: 1,
                complete_frame_coverage: true,
                levels: Vec::new(),
                metrics: DicomExportMetrics {
                    total_frames: 1,
                    gpu_transcode_frames: 1,
                    ..DicomExportMetrics::default()
                },
                elapsed_micros: 1_000,
            },
            None,
        );

        assert!(summary.contains("requested_frames_per_level=all"));
        assert!(summary.contains("complete_frame_coverage=true"));
    }

    #[test]
    fn cli_corpus_coverage_summary_reports_sources_failures_and_aggregate_routes() {
        let summary = format_corpus_coverage_summary_with_memory(
            &DicomRouteCorpusCoverageReport {
                source_root: PathBuf::from("corpus"),
                transfer_syntax_uid: "1.2.840.10008.1.2.4.202",
                requested_frames_per_level: 4,
                max_levels: Some(1),
                sources_considered: 3,
                available_frames: 100_000,
                complete_frame_coverage: false,
                reports: vec![DicomRouteCoverageReport {
                    source_path: PathBuf::from("corpus/source.svs"),
                    transfer_syntax_uid: "1.2.840.10008.1.2.4.202",
                    requested_frames_per_level: 4,
                    available_frames: 100_000,
                    complete_frame_coverage: false,
                    levels: vec![DicomRouteProfileReport {
                        source_path: PathBuf::from("corpus/source.svs"),
                        transfer_syntax_uid: "1.2.840.10008.1.2.4.202",
                        level: 0,
                        requested_frames: 4,
                        available_frames: 100_000,
                        metrics: DicomExportMetrics {
                            total_frames: 4,
                            gpu_transcode_frames: 4,
                            resident_gpu_transcode_frames: 4,
                            gpu_input_decode_frames: 4,
                            gpu_encode_frames: 4,
                            ..DicomExportMetrics::default()
                        },
                        elapsed_micros: 10_000,
                    }],
                    metrics: DicomExportMetrics {
                        total_frames: 4,
                        gpu_transcode_frames: 4,
                        resident_gpu_transcode_frames: 4,
                        gpu_input_decode_frames: 4,
                        gpu_encode_frames: 4,
                        ..DicomExportMetrics::default()
                    },
                    elapsed_micros: 10_000,
                }],
                failures: vec![DicomRouteCorpusCoverageFailure {
                    source_path: PathBuf::from("corpus/bad.svs"),
                    message: "unsupported".into(),
                }],
                metrics: DicomExportMetrics {
                    total_frames: 4,
                    gpu_transcode_frames: 4,
                    resident_gpu_transcode_frames: 4,
                    gpu_input_decode_frames: 4,
                    gpu_encode_frames: 4,
                    gpu_dispatch_micros: 9_000,
                    ..DicomExportMetrics::default()
                },
                elapsed_micros: 12_000,
            },
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
    }

    #[test]
    fn cli_sustain_iteration_summary_reports_throughput_memory_and_thermal_state() {
        let summary = format_sustain_iteration_summary(
            2,
            5,
            &DicomRouteCoverageReport {
                source_path: PathBuf::from("source.svs"),
                transfer_syntax_uid: "1.2.840.10008.1.2.4.202",
                requested_frames_per_level: 4,
                available_frames: 12,
                complete_frame_coverage: true,
                levels: Vec::new(),
                metrics: DicomExportMetrics {
                    total_frames: 12,
                    gpu_transcode_frames: 12,
                    resident_gpu_transcode_frames: 12,
                    gpu_input_decode_frames: 12,
                    gpu_encode_frames: 12,
                    gpu_dispatch_micros: 12_500,
                    ..DicomExportMetrics::default()
                },
                elapsed_micros: 2_000_000,
            },
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
    }

    #[test]
    fn cli_sustain_convert_summary_reports_real_export_throughput() {
        let summary = format_sustain_export_iteration_summary(
            1,
            3,
            &DicomExportReport {
                output_dir: PathBuf::from("out/iteration-0001"),
                instances: Vec::new(),
                metrics: DicomExportMetrics {
                    total_frames: 20,
                    j2k_passthrough_frames: 4,
                    gpu_transcode_frames: 12,
                    resident_gpu_transcode_frames: 10,
                    partial_gpu_transcode_frames: 2,
                    cpu_fallback_frames: 4,
                    gpu_input_decode_frames: 12,
                    gpu_encode_frames: 12,
                    write_micros: 3_500,
                    input_decode_micros: 8_000,
                    compose_micros: 2_000,
                    encode_micros: 4_000,
                    validation_micros: 1_000,
                    gpu_dispatch_micros: 15_000,
                    ..DicomExportMetrics::default()
                },
            },
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
    }
}
