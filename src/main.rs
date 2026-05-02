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
        } => {
            let metadata = load_metadata_source(metadata, research_placeholder)?;
            let report = export_dicom(DicomExportRequest {
                source_path: source,
                output_dir: out,
                options: DicomExportOptions {
                    tile_size,
                    transfer_syntax: TransferSyntax::Jpeg2000Lossless,
                    encode_backend: backend.into_preference(),
                },
                metadata,
            })?;
            println!(
                "wrote {} DICOM instance(s) to {}",
                report.instances.len(),
                report.output_dir.display()
            );
            Ok(())
        }
    }
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
