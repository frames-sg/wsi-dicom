use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use wsi_dicom::{
    AnnotationCoordinateSpace, AnnotationTarget, AssociatedImageExclusions, AssociatedImageKind,
    CodecValidation, EncodeBackendPreference, Error, ExportOptions, ExportPreset,
    JpegDirectHtj2kProfile, PixelReencodePolicy, QuPathAnnotationOptions, SourcePixelSpacingMm,
    TransferSyntax, UidPolicy,
};

use crate::cli_calibration::{CalibrationCommand, ColorManagementArgs};
#[derive(Debug, Parser)]
#[command(name = "wsi-dicom", version)]
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
        #[command(flatten)]
        annotations: AnnotationCliArgs,
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
    Validate(ValidateArgs),
    Doctor {
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        dcmvalidate_iod: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    SelfTest(SelfTestArgs),
    /// Lists every normative statement of the pinned DICOM edition in the VL
    /// WSI scope and how validation checks it.
    Requirements {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum AnnotationTargetArg {
    Ann,
    Seg,
    Sr,
}

impl From<AnnotationTargetArg> for AnnotationTarget {
    fn from(value: AnnotationTargetArg) -> Self {
        match value {
            AnnotationTargetArg::Ann => Self::Ann,
            AnnotationTargetArg::Seg => Self::Seg,
            AnnotationTargetArg::Sr => Self::Sr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum AnnotationCoordinateSpaceArg {
    Level0Pixels,
    SourcePixels,
    SlideMm,
}

impl From<AnnotationCoordinateSpaceArg> for AnnotationCoordinateSpace {
    fn from(value: AnnotationCoordinateSpaceArg) -> Self {
        match value {
            AnnotationCoordinateSpaceArg::Level0Pixels => Self::Level0Pixels,
            AnnotationCoordinateSpaceArg::SourcePixels => Self::SourcePixels,
            AnnotationCoordinateSpaceArg::SlideMm => Self::SlideMillimeters,
        }
    }
}

#[derive(Debug, Clone, Default, Args)]
pub(crate) struct AnnotationCliArgs {
    /// QuPath GeoJSON export to convert after the WSI export succeeds.
    #[arg(
        long,
        value_name = "GEOJSON",
        requires_all = ["annotation_mapping", "annotation_target"]
    )]
    pub(crate) qupath_annotations: Option<PathBuf>,
    /// Explicit QuPath class/property to DICOM terminology mapping.
    #[arg(long, value_name = "JSON", requires = "qupath_annotations")]
    pub(crate) annotation_mapping: Option<PathBuf>,
    /// DICOM sidecar type to create; repeat to request multiple targets.
    #[arg(long, value_enum, requires = "qupath_annotations")]
    pub(crate) annotation_target: Vec<AnnotationTargetArg>,
    /// Coordinate convention used by the QuPath GeoJSON file.
    #[arg(long, value_enum, requires = "qupath_annotations")]
    pub(crate) annotation_coordinate_space: Option<AnnotationCoordinateSpaceArg>,
    /// Permit converter-supported lossy annotation normalizations.
    #[arg(long, requires = "qupath_annotations")]
    pub(crate) allow_lossy_annotations: bool,
}

impl AnnotationCliArgs {
    pub(crate) fn options(self) -> Result<Option<QuPathAnnotationOptions>, Error> {
        let Some(geojson_path) = self.qupath_annotations else {
            return Ok(None);
        };
        let mapping_path = self
            .annotation_mapping
            .ok_or_else(|| Error::InvalidOptions {
                reason: "--qupath-annotations requires --annotation-mapping".into(),
            })?;
        if self.annotation_target.is_empty() {
            return Err(Error::InvalidOptions {
                reason: "--qupath-annotations requires at least one --annotation-target".into(),
            });
        }
        let mut options = QuPathAnnotationOptions::new(
            geojson_path,
            mapping_path,
            self.annotation_target
                .into_iter()
                .map(AnnotationTarget::from)
                .collect(),
        );
        options.coordinate_space = self
            .annotation_coordinate_space
            .map(AnnotationCoordinateSpace::from)
            .unwrap_or_default();
        options.allow_lossy = self.allow_lossy_annotations;
        Ok(Some(options))
    }
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
    /// Use optimal per-frame Huffman tables for re-encoded JPEG Baseline frames.
    #[arg(long)]
    pub(crate) jpeg_optimize_huffman: bool,
    /// Decode and re-encode compressed source frames that no compressed-domain
    /// route can preserve: never, only into lossless transfer syntaxes, or also
    /// into lossy ones (adds a lossy generation).
    #[arg(long, value_enum, default_value_t = PixelReencodePolicy::Lossless)]
    pub(crate) pixel_reencode: PixelReencodePolicy,
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
        options.jpeg_optimize_huffman = self.jpeg_optimize_huffman;
        options.pixel_reencode = self.pixel_reencode;
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
    /// Governed level-zero spacing as ROW_MM,COLUMN_MM when source calibration is unavailable.
    #[arg(long, value_name = "ROW_MM,COLUMN_MM")]
    pub(crate) source_pixel_spacing_mm: Option<SourcePixelSpacingMm>,
    #[arg(long)]
    pub(crate) overwrite: bool,
    /// Associated images to leave out (comma-separated): label, overview,
    /// thumbnail, or all. All are written by default. The label and the
    /// overview, which shows part of the label, can carry patient identifiers.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "KIND")]
    pub(crate) exclude_associated: Vec<AssociatedImageArg>,
    #[command(flatten)]
    pub(crate) conformance: ConformanceArgs,
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
        options.source_pixel_spacing_mm = self.source_pixel_spacing_mm;
        options.overwrite = self.overwrite;
        options.exclude_associated = self.exclude_associated.iter().fold(
            AssociatedImageExclusions::NONE,
            |exclusions, kind| match kind {
                AssociatedImageArg::All => AssociatedImageExclusions::ALL,
                AssociatedImageArg::Label => exclusions.with(AssociatedImageKind::Label),
                AssociatedImageArg::Overview => exclusions.with(AssociatedImageKind::Overview),
                AssociatedImageArg::Thumbnail => exclusions.with(AssociatedImageKind::Thumbnail),
            },
        );
        options.conformance = self.conformance.level();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum AssociatedImageArg {
    Label,
    Overview,
    Thumbnail,
    All,
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

#[derive(Debug, Args)]
pub(crate) struct ValidateArgs {
    pub(crate) path: PathBuf,
    #[arg(long)]
    pub(crate) strict: bool,
    #[arg(long)]
    pub(crate) dcmvalidate_iod: Option<PathBuf>,
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) command_timeout_secs: u64,
    #[arg(long, value_enum, default_value = "general")]
    pub(crate) profile: wsi_dicom::ValidationProfile,
    #[arg(long, default_value_t = 1024 * 1024 * 1024)]
    pub(crate) max_input_bytes: u64,
    #[command(flatten)]
    pub(crate) conformance: ConformanceArgs,
    #[arg(long)]
    pub(crate) json: bool,
}

/// `--optional` (the default) or `--required`.
#[derive(Debug, Clone, Copy, Default, Args)]
pub(crate) struct ConformanceArgs {
    /// Also cover the optional VL WSI modules and attributes (the default).
    #[arg(long, conflicts_with = "required")]
    pub(crate) optional: bool,
    /// Cover only what the standard requires.
    #[arg(long)]
    pub(crate) required: bool,
}

impl ConformanceArgs {
    pub(crate) fn level(self) -> wsi_dicom::ConformanceLevel {
        if self.required {
            wsi_dicom::ConformanceLevel::Required
        } else {
            wsi_dicom::ConformanceLevel::Optional
        }
    }
}
