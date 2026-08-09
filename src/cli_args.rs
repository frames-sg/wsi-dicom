use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use wsi_dicom::{
    CodecValidation, EncodeBackendPreference, Error, ExportOptions, ExportPreset,
    JpegDirectHtj2kProfile, TransferSyntax, UidPolicy,
};

use crate::cli_calibration::{CalibrationCommand, ColorManagementArgs};
#[derive(Debug, Parser)]
#[command(name = "wsi-dicom")]
#[command(about = "Convert wsi-rs-readable whole-slide images to DICOM VL WSI")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
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
    /// Inspect or package governed scanner ICC calibration profiles.
    Calibration {
        #[command(subcommand)]
        command: CalibrationCommand,
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
pub(crate) struct SelfTestArgs {
    #[arg(long)]
    pub(crate) out: Option<PathBuf>,
    #[arg(long)]
    pub(crate) keep_output: bool,
    #[arg(long)]
    pub(crate) strict: bool,
    #[arg(long)]
    pub(crate) dcmvalidate_iod: Option<PathBuf>,
    #[arg(long)]
    pub(crate) htj2k_decoder: Option<String>,
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) command_timeout_secs: u64,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Clone, Copy, Args)]
pub(crate) struct EncodeArgs {
    #[arg(long, value_enum, default_value_t = EncodeBackendPreference::Auto)]
    pub(crate) backend: EncodeBackendPreference,
    #[arg(long, default_value_t = 512)]
    pub(crate) tile_size: u32,
    #[arg(long, default_value_t = 90)]
    pub(crate) jpeg_quality: u8,
    #[arg(long, value_enum)]
    pub(crate) transfer_syntax: Option<TransferSyntax>,
    #[arg(long, value_enum)]
    pub(crate) jpeg_direct_htj2k_profile: Option<JpegDirectHtj2kProfile>,
    #[arg(long)]
    pub(crate) j2k_decomposition_levels: Option<u8>,
    #[arg(long, value_enum, default_value_t = CodecValidation::Disabled)]
    pub(crate) codec_validation: CodecValidation,
    #[arg(long)]
    pub(crate) source_device_decode: bool,
    #[command(flatten)]
    pub(crate) gpu_encode: GpuEncodeArgs,
}

impl EncodeArgs {
    pub(crate) fn source_aware_transfer_syntax(self) -> bool {
        self.transfer_syntax.is_none()
    }

    fn defaulted_transfer_syntax(self) -> TransferSyntax {
        self.transfer_syntax
            .unwrap_or(ExportOptions::default().transfer_syntax)
    }

    pub(crate) fn lossless_review_options(self) -> ExportOptions {
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

#[derive(Debug, Clone, Args)]
pub(crate) struct ExportCliArgs {
    #[command(flatten)]
    pub(crate) encode: EncodeArgs,
    #[arg(long, value_enum, default_value_t = ExportPreset::LosslessReview)]
    pub(crate) preset: ExportPreset,
    #[command(flatten)]
    pub(crate) color_management: ColorManagementArgs,
    #[arg(long, value_enum, default_value_t = UidPolicy::Fresh)]
    pub(crate) uid_policy: UidPolicy,
    #[arg(long)]
    pub(crate) overwrite: bool,
    #[arg(long, default_value_t = 256)]
    pub(crate) max_instance_metadata_mib: u64,
    #[arg(long, default_value_t = 1024)]
    pub(crate) max_total_metadata_mib: u64,
}

impl ExportCliArgs {
    pub(crate) fn options(&self) -> Result<ExportOptions, Error> {
        let transfer_syntax =
            resolve_export_transfer_syntax(self.preset, self.encode.transfer_syntax)?;
        let mut options = self.encode.options_with_transfer_syntax(
            self.preset
                .options(self.encode.tile_size, self.encode.jpeg_quality),
            transfer_syntax,
        );
        options.uid_policy = self.uid_policy;
        options.overwrite = self.overwrite;
        options.max_instance_metadata_bytes = checked_metadata_mib_to_bytes(
            "max_instance_metadata_mib",
            self.max_instance_metadata_mib,
        )?;
        options.max_total_metadata_bytes =
            checked_metadata_mib_to_bytes("max_total_metadata_mib", self.max_total_metadata_mib)?;
        options.validate()?;
        Ok(options)
    }
}

fn checked_metadata_mib_to_bytes(field: &str, value: u64) -> Result<u64, Error> {
    value
        .checked_mul(1024 * 1024)
        .ok_or_else(|| Error::InvalidOptions {
            reason: format!("{field} exceeds the u64 byte range"),
        })
}

#[derive(Debug, Clone, Copy, Default, Args)]
pub(crate) struct GpuEncodeArgs {
    #[arg(long)]
    pub(crate) gpu_encode_inflight_tiles: Option<usize>,
    #[arg(long)]
    pub(crate) gpu_encode_memory_mib: Option<u64>,
    #[arg(long)]
    pub(crate) gpu_pipeline_depth: Option<usize>,
    #[arg(long)]
    pub(crate) gpu_row_batch_rows: Option<usize>,
    #[arg(long)]
    pub(crate) gpu_row_batch_target_tiles: Option<usize>,
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

pub(crate) fn resolve_export_transfer_syntax(
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
