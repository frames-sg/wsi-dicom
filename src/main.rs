#![forbid(unsafe_code)]

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use wsi_dicom::{
    export_dicom, DicomExportOptions, DicomExportRequest, DicomMetadata, EncodeBackendPreference,
    MetadataSource, TransferSyntax, WsiDicomError,
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
    Jpeg2000Lossless,
    Htj2kLossless,
    Htj2kLosslessRpcl,
}

impl TransferSyntaxArg {
    fn into_transfer_syntax(self) -> TransferSyntax {
        match self {
            Self::JpegBaseline8Bit => TransferSyntax::JpegBaseline8Bit,
            Self::Jpeg2000Lossless => TransferSyntax::Jpeg2000Lossless,
            Self::Htj2kLossless => TransferSyntax::Htj2kLossless,
            Self::Htj2kLosslessRpcl => TransferSyntax::Htj2kLosslessRpcl,
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
        } => {
            let metadata = load_metadata_source(metadata, research_placeholder)?;
            let report = export_dicom(DicomExportRequest {
                source_path: source,
                output_dir: out,
                options: DicomExportOptions {
                    tile_size,
                    transfer_syntax: transfer_syntax.into_transfer_syntax(),
                    encode_backend: backend.into_preference(),
                },
                metadata,
            })?;
            println!(
                "wrote {} DICOM instance(s) to {}; frames total={} cpu_input={} gpu_input_decode={} gpu_encode={} gpu_validation={} input_decode_ms={:.3} compose_ms={:.3} encode_ms={:.3} validation_ms={:.3} write_ms={:.3}",
                report.instances.len(),
                report.output_dir.display(),
                report.metrics.total_frames,
                report.metrics.cpu_input_frames,
                report.metrics.gpu_input_decode_frames,
                report.metrics.gpu_encode_frames,
                report.metrics.gpu_validation_frames,
                micros_to_ms(report.metrics.input_decode_micros),
                micros_to_ms(report.metrics.compose_micros),
                micros_to_ms(report.metrics.encode_micros),
                micros_to_ms(report.metrics.validation_micros),
                micros_to_ms(report.metrics.write_micros),
            );
            Ok(())
        }
    }
}

fn micros_to_ms(micros: u128) -> f64 {
    micros as f64 / 1_000.0
}

fn load_metadata_source(
    metadata_path: Option<PathBuf>,
    research_placeholder: bool,
) -> Result<MetadataSource, WsiDicomError> {
    if research_placeholder {
        return Ok(MetadataSource::ResearchPlaceholder);
    }

    let Some(path) = metadata_path else {
        return Ok(MetadataSource::Strict(DicomMetadata::default()));
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
        Ok(MetadataSource::Strict(metadata))
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
