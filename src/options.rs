use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::Error;

/// Explicit level-zero source pixel spacing in DICOM row/column order, in millimeters.
///
/// This is intended for governed metadata supplied when a proprietary source
/// reader cannot surface its physical calibration. Export rejects a supplied
/// value that conflicts with calibration already present in the source.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SourcePixelSpacingMm {
    /// Vertical pixel spacing (row spacing), in millimeters.
    pub row: f64,
    /// Horizontal pixel spacing (column spacing), in millimeters.
    pub column: f64,
}

impl PartialEq for SourcePixelSpacingMm {
    fn eq(&self, other: &Self) -> bool {
        self.row.to_bits() == other.row.to_bits() && self.column.to_bits() == other.column.to_bits()
    }
}

impl Eq for SourcePixelSpacingMm {}

impl SourcePixelSpacingMm {
    /// Construct a finite, positive row/column spacing pair.
    pub fn new(row: f64, column: f64) -> Result<Self, Error> {
        let spacing = Self { row, column };
        spacing.validate()?;
        Ok(spacing)
    }

    fn validate(self) -> Result<(), Error> {
        if !self.row.is_finite()
            || !self.column.is_finite()
            || self.row <= 0.0
            || self.column <= 0.0
        {
            return Err(Error::InvalidOptions {
                reason:
                    "source_pixel_spacing_mm row and column must be finite and greater than zero"
                        .into(),
            });
        }
        Ok(())
    }
}

impl FromStr for SourcePixelSpacingMm {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (row, column) = value.split_once(',').ok_or_else(|| {
            "source pixel spacing must be ROW_MM,COLUMN_MM in DICOM order".to_string()
        })?;
        if column.contains(',') {
            return Err("source pixel spacing must contain exactly two values".into());
        }
        let row = row
            .parse::<f64>()
            .map_err(|_| "source pixel row spacing is not a number".to_string())?;
        let column = column
            .parse::<f64>()
            .map_err(|_| "source pixel column spacing is not a number".to_string())?;
        Self::new(row, column).map_err(|error| error.to_string())
    }
}

/// Runtime preference for JPEG 2000 Lossless encode backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[non_exhaustive]
pub enum EncodeBackendPreference {
    /// Let the crate choose the safest measured backend for the request.
    Auto,
    /// Always use CPU encoding.
    #[value(name = "cpu")]
    CpuOnly,
    /// Prefer a device backend, but fall back to CPU when unavailable.
    PreferDevice,
    /// Require a device backend and fail when it cannot be used.
    RequireDevice,
}

impl EncodeBackendPreference {
    pub(crate) fn requires_device(self) -> bool {
        matches!(self, Self::RequireDevice)
    }

    pub(crate) fn cpu_batch_safe(self) -> bool {
        match self {
            Self::CpuOnly => true,
            Self::Auto => !cfg!(any(feature = "metal", feature = "cuda")),
            Self::PreferDevice | Self::RequireDevice => false,
        }
    }

    pub(crate) fn to_j2k(self) -> j2k::EncodeBackendPreference {
        match self {
            Self::Auto => j2k::EncodeBackendPreference::Auto,
            Self::CpuOnly => j2k::EncodeBackendPreference::CpuOnly,
            Self::PreferDevice => j2k::EncodeBackendPreference::Auto,
            Self::RequireDevice => j2k::EncodeBackendPreference::RequireDevice,
        }
    }
}

/// Runtime validation policy for newly encoded compressed frame bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[non_exhaustive]
pub enum CodecValidation {
    /// Do not run an encode-time validation decode.
    Disabled,
    /// Decode encoded frames during export to catch codec regressions.
    RoundTrip,
}

impl CodecValidation {
    pub(crate) fn to_j2k_validation(self) -> j2k::J2kEncodeValidation {
        match self {
            Self::Disabled => j2k::J2kEncodeValidation::External,
            Self::RoundTrip => j2k::J2kEncodeValidation::CpuRoundTrip,
        }
    }
}

/// When compressed source frames may be decoded and re-encoded.
///
/// Compressed-domain routes (passthrough, retiling, direct transcoding) are
/// always tried first. This policy only governs frames those routes reject.
/// Re-encoded frames keep the source's lossy compression history and are
/// counted in `RouteMetrics::compressed_source_reencoded_frames`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum PixelReencodePolicy {
    /// Fail when a compressed source frame cannot be preserved in the compressed domain.
    Never,
    /// Re-encode into lossless transfer syntaxes only; this adds no further loss.
    #[default]
    Lossless,
    /// Also re-encode into lossy transfer syntaxes, adding a lossy generation.
    Any,
}

impl PixelReencodePolicy {
    /// Whether this policy admits decoded-pixel re-encoding into `transfer_syntax`.
    pub fn permits(self, transfer_syntax: TransferSyntax) -> bool {
        match self {
            Self::Never => false,
            Self::Lossless => transfer_syntax.is_lossless(),
            Self::Any => true,
        }
    }

    /// Why this policy refused a re-encode into `transfer_syntax`, naming the
    /// setting that would admit it.
    pub(crate) fn refusal(self, transfer_syntax: TransferSyntax) -> &'static str {
        match self {
            Self::Never => "pixel re-encoding fallback is disabled by pixel_reencode=never",
            Self::Lossless | Self::Any if transfer_syntax == TransferSyntax::JpegBaseline8Bit => {
                "pixel re-encoding fallback is disabled for lossy output; pixel_reencode=any permits a lossy JPEG Baseline re-encode"
            }
            Self::Lossless | Self::Any => {
                "pixel re-encoding fallback is disabled; lossy JPEG 2000 and HTJ2K output has no pixel re-encoding route"
            }
        }
    }
}

/// Policy for identifiers generated when the caller does not supply them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum UidPolicy {
    /// Generate a fresh identity namespace for every export invocation.
    Fresh,
    /// Derive repeatable identifiers from source content, metadata, and export options.
    Deterministic,
}

/// DICOM transfer syntax choices for exported VL Whole Slide Microscopy files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TransferSyntax {
    /// JPEG Baseline 8-bit transfer syntax.
    JpegBaseline8Bit = 0,
    /// JPEG 2000 Image Compression transfer syntax.
    Jpeg2000 = 1,
    /// JPEG 2000 Image Compression Lossless Only transfer syntax.
    Jpeg2000Lossless = 2,
    /// High-Throughput JPEG 2000 Image Compression transfer syntax.
    Htj2k = 6,
    /// High-Throughput JPEG 2000 Image Compression Lossless Only transfer syntax.
    Htj2kLossless = 3,
    /// High-Throughput JPEG 2000 with RPCL Options Image Compression Lossless Only transfer syntax.
    Htj2kLosslessRpcl = 4,
    /// JPEG Lossless, Non-Hierarchical, First-Order Prediction (Process 14,
    /// Selection Value 1) transfer syntax.
    JpegLosslessSv1 = 7,
    /// Explicit VR Little Endian transfer syntax for uncompressed input fixtures.
    #[value(skip)]
    ExplicitVrLittleEndian = 5,
}

/// User-facing export presets for common conversion goals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ExportPreset {
    /// Reviewer-focused output that preserves the default lossless HTJ2K target.
    LosslessReview,
    /// Fast JPEG Baseline output that preserves compatible native JPEG geometry.
    FastJpeg,
}

impl ExportPreset {
    /// Return export options for this preset with caller-supplied geometry and quality knobs.
    pub fn options(self, tile_size: u32, jpeg_quality: u8) -> ExportOptions {
        match self {
            Self::LosslessReview => ExportOptions {
                tile_size,
                jpeg_quality,
                ..ExportOptions::lossless_review()
            },
            Self::FastJpeg => ExportOptions::fast_jpeg(tile_size, jpeg_quality),
        }
    }
}

impl TransferSyntax {
    /// All transfer syntax variants supported by this crate.
    pub const ALL: [Self; 8] = [
        Self::JpegBaseline8Bit,
        Self::Jpeg2000,
        Self::Jpeg2000Lossless,
        Self::Htj2k,
        Self::Htj2kLossless,
        Self::Htj2kLosslessRpcl,
        Self::JpegLosslessSv1,
        Self::ExplicitVrLittleEndian,
    ];

    /// Return the DICOM transfer syntax UID.
    pub fn uid(self) -> &'static str {
        match self {
            Self::JpegBaseline8Bit => "1.2.840.10008.1.2.4.50",
            Self::Jpeg2000 => "1.2.840.10008.1.2.4.91",
            Self::Jpeg2000Lossless => "1.2.840.10008.1.2.4.90",
            Self::Htj2k => "1.2.840.10008.1.2.4.203",
            Self::Htj2kLossless => "1.2.840.10008.1.2.4.201",
            Self::Htj2kLosslessRpcl => "1.2.840.10008.1.2.4.202",
            Self::JpegLosslessSv1 => "1.2.840.10008.1.2.4.70",
            Self::ExplicitVrLittleEndian => "1.2.840.10008.1.2.1",
        }
    }

    pub(crate) fn is_j2k_family(self) -> bool {
        matches!(
            self,
            Self::Jpeg2000
                | Self::Jpeg2000Lossless
                | Self::Htj2k
                | Self::Htj2kLossless
                | Self::Htj2kLosslessRpcl
        )
    }

    pub(crate) fn is_lossless_j2k_family(self) -> bool {
        matches!(
            self,
            Self::Jpeg2000Lossless | Self::Htj2kLossless | Self::Htj2kLosslessRpcl
        )
    }

    pub(crate) fn is_jpeg2000_passthrough_only(self) -> bool {
        self == Self::Jpeg2000
    }

    /// Whether encoding into this transfer syntax is mathematically lossless.
    pub(crate) fn is_lossless(self) -> bool {
        matches!(
            self,
            Self::Jpeg2000Lossless
                | Self::Htj2kLossless
                | Self::Htj2kLosslessRpcl
                | Self::JpegLosslessSv1
                | Self::ExplicitVrLittleEndian
        )
    }
}

/// Direct JPEG-to-HTJ2K coefficient path used for HTJ2K export from JPEG tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum JpegDirectHtj2kProfile {
    /// Reversible 5/3 transform for HTJ2K lossless transfer syntaxes.
    #[value(name = "53")]
    Lossless53,
    /// Backwards-compatible alias for the balanced irreversible 9/7 profile.
    #[value(name = "97", alias = "lossy97")]
    Lossy97,
    /// Near-lossless irreversible 9/7 profile, quantization scale 2.
    #[value(name = "lossy97-near", alias = "97-near")]
    Lossy97Near,
    /// Balanced irreversible 9/7 profile, quantization scale 5.
    #[value(name = "lossy97-balanced", alias = "97-balanced")]
    Lossy97Balanced,
    /// Aggressive irreversible 9/7 profile, quantization scale 10.
    #[value(name = "lossy97-aggressive", alias = "97-aggressive")]
    Lossy97Aggressive,
    /// Preview-oriented irreversible 9/7 profile, quantization scale 20.
    #[value(name = "lossy97-preview", alias = "97-preview")]
    Lossy97Preview,
    /// Thumbnail-oriented irreversible 9/7 profile, quantization scale 50.
    #[value(name = "lossy97-thumbnail", alias = "97-thumbnail")]
    Lossy97Thumbnail,
}

impl JpegDirectHtj2kProfile {
    /// Return the profile normally paired with the requested transfer syntax.
    pub fn default_for_transfer_syntax(transfer_syntax: TransferSyntax) -> Self {
        match transfer_syntax {
            TransferSyntax::Htj2k => Self::Lossy97,
            _ => Self::Lossless53,
        }
    }

    /// Whether this profile uses the irreversible 9/7 transform.
    pub const fn is_lossy_97(self) -> bool {
        matches!(
            self,
            Self::Lossy97
                | Self::Lossy97Near
                | Self::Lossy97Balanced
                | Self::Lossy97Aggressive
                | Self::Lossy97Preview
                | Self::Lossy97Thumbnail
        )
    }

    /// Quantization scale used by irreversible 9/7 profiles.
    pub const fn irreversible_quantization_scale(self) -> Option<f32> {
        match self {
            Self::Lossless53 => None,
            Self::Lossy97Near => Some(2.0),
            Self::Lossy97 | Self::Lossy97Balanced => Some(5.0),
            Self::Lossy97Aggressive => Some(10.0),
            Self::Lossy97Preview => Some(20.0),
            Self::Lossy97Thumbnail => Some(50.0),
        }
    }
}

/// Options controlling how a source WSI should be converted into DICOM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ExportOptions {
    /// Target DICOM tile size in pixels for generated frames.
    pub tile_size: u32,
    /// Whether generated DICOM files may replace existing files.
    pub overwrite: bool,
    /// Maximum prepared uncompressed frame buffer size in bytes.
    pub max_prepared_frame_bytes: u64,
    /// Maximum streamed per-frame and frame-index metadata bytes for one instance.
    pub max_instance_metadata_bytes: u64,
    /// Maximum streamed per-frame and frame-index metadata bytes across one export.
    pub max_total_metadata_bytes: u64,
    /// Requested output transfer syntax.
    pub transfer_syntax: TransferSyntax,
    /// Direct JPEG-to-HTJ2K profile used when eligible.
    pub jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile,
    /// JPEG quality used for JPEG Baseline fallback encoding.
    pub jpeg_quality: u8,
    /// Whether re-encoded JPEG Baseline frames use per-frame optimal Huffman
    /// tables (T.81 Annex K.2) instead of the Annex K standard tables.
    ///
    /// Pixels are identical either way; only the entropy-coded size changes.
    /// Source JPEG frames copied by passthrough or retile keep their original
    /// bytes. Optimized tables are a CPU encode: `PreferDevice` encodes these
    /// frames on the CPU, and `RequireDevice` rejects the combination.
    pub jpeg_optimize_huffman: bool,
    /// When compressed source frames rejected by every compressed-domain route
    /// may be decoded and re-encoded. Defaults to lossless targets only.
    pub pixel_reencode: PixelReencodePolicy,
    /// Policy for generated Study, Series, SOP, and related DICOM UIDs.
    pub uid_policy: UidPolicy,
    /// Optional governed level-zero source spacing when the reader lacks calibration metadata.
    pub source_pixel_spacing_mm: Option<SourcePixelSpacingMm>,
    /// Runtime encoder backend preference.
    pub encode_backend: EncodeBackendPreference,
    /// Runtime codec validation policy.
    pub codec_validation: CodecValidation,
    /// Whether source tile decode may use a device backend when available.
    pub source_device_decode: bool,
    /// Optional maximum JPEG 2000 decomposition level override.
    pub j2k_decomposition_levels: Option<u8>,
    /// Optional cap on concurrently in-flight GPU encode tiles.
    pub gpu_encode_inflight_tiles: Option<usize>,
    /// Optional GPU encode memory budget in MiB.
    pub gpu_encode_memory_mib: Option<u64>,
    /// Optional GPU pipeline depth.
    pub gpu_pipeline_depth: Option<usize>,
    /// Optional maximum rows per GPU row batch.
    pub gpu_row_batch_rows: Option<usize>,
    /// Optional target tile count per GPU row batch.
    pub gpu_row_batch_target_tiles: Option<usize>,
}

/// Validated export semantics used by internal planning and writing code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NormalizedExportSemantics {
    pub(crate) tile_size: u32,
    pub(crate) overwrite: bool,
    pub(crate) transfer_syntax: TransferSyntax,
    pub(crate) jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile,
    pub(crate) jpeg_quality: u8,
    pub(crate) jpeg_optimize_huffman: bool,
    pub(crate) pixel_reencode: PixelReencodePolicy,
    pub(crate) uid_policy: UidPolicy,
    pub(crate) source_pixel_spacing_mm: Option<SourcePixelSpacingMm>,
}

/// Validated memory and metadata bounds used by internal execution code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResourceLimits {
    pub(crate) max_prepared_frame_bytes: u64,
    pub(crate) max_instance_metadata_bytes: u64,
    pub(crate) max_total_metadata_bytes: u64,
}

/// Validated GPU tuning values kept separate from durable export semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GpuTuning {
    pub(crate) encode_inflight_tiles: Option<usize>,
    pub(crate) encode_memory_mib: Option<u64>,
    pub(crate) pipeline_depth: Option<usize>,
    pub(crate) row_batch_rows: Option<usize>,
    pub(crate) row_batch_target_tiles: Option<usize>,
}

/// Validated execution policy used by route and backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutionOptions {
    pub(crate) encode_backend: EncodeBackendPreference,
    pub(crate) codec_validation: CodecValidation,
    pub(crate) source_device_decode: bool,
    pub(crate) j2k_decomposition_levels: Option<u8>,
    pub(crate) gpu: GpuTuning,
}

/// Patch-compatible internal representation of the public flat option surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NormalizedExportOptions {
    pub(crate) semantics: NormalizedExportSemantics,
    pub(crate) resources: ResourceLimits,
    pub(crate) execution: ExecutionOptions,
}

impl NormalizedExportOptions {
    pub(crate) fn new(options: &ExportOptions) -> Result<Self, Error> {
        options.validate()?;
        Ok(Self::from_validated(options))
    }

    pub(crate) fn from_validated(options: &ExportOptions) -> Self {
        Self {
            semantics: NormalizedExportSemantics {
                tile_size: options.tile_size,
                overwrite: options.overwrite,
                transfer_syntax: options.transfer_syntax,
                jpeg_direct_htj2k_profile: options.jpeg_direct_htj2k_profile,
                jpeg_quality: options.jpeg_quality,
                jpeg_optimize_huffman: options.jpeg_optimize_huffman,
                pixel_reencode: options.pixel_reencode,
                uid_policy: options.uid_policy,
                source_pixel_spacing_mm: options.source_pixel_spacing_mm,
            },
            resources: ResourceLimits {
                max_prepared_frame_bytes: options.max_prepared_frame_bytes,
                max_instance_metadata_bytes: options.max_instance_metadata_bytes,
                max_total_metadata_bytes: options.max_total_metadata_bytes,
            },
            execution: ExecutionOptions {
                encode_backend: options.encode_backend,
                codec_validation: options.codec_validation,
                source_device_decode: options.source_device_decode,
                j2k_decomposition_levels: options.j2k_decomposition_levels,
                gpu: GpuTuning {
                    encode_inflight_tiles: options.gpu_encode_inflight_tiles,
                    encode_memory_mib: options.gpu_encode_memory_mib,
                    pipeline_depth: options.gpu_pipeline_depth,
                    row_batch_rows: options.gpu_row_batch_rows,
                    row_batch_target_tiles: options.gpu_row_batch_target_tiles,
                },
            },
        }
    }
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            tile_size: 512,
            overwrite: false,
            max_prepared_frame_bytes: 256 * 1024 * 1024,
            max_instance_metadata_bytes: 256 * 1024 * 1024,
            max_total_metadata_bytes: 1024 * 1024 * 1024,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile::Lossless53,
            jpeg_quality: 90,
            jpeg_optimize_huffman: false,
            pixel_reencode: PixelReencodePolicy::Lossless,
            uid_policy: UidPolicy::Fresh,
            source_pixel_spacing_mm: None,
            encode_backend: EncodeBackendPreference::Auto,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            j2k_decomposition_levels: None,
            gpu_encode_inflight_tiles: None,
            gpu_encode_memory_mib: None,
            gpu_pipeline_depth: None,
            gpu_row_batch_rows: None,
            gpu_row_batch_target_tiles: None,
        }
    }
}

impl ExportOptions {
    /// Return reviewer-focused lossless export options.
    pub fn lossless_review() -> Self {
        Self::default()
    }

    /// Return fast JPEG Baseline export options for speed-oriented comparisons.
    pub fn fast_jpeg(tile_size: u32, jpeg_quality: u8) -> Self {
        Self {
            tile_size,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            jpeg_quality,
            ..Self::default()
        }
    }
}

impl ExportOptions {
    /// Validate option combinations before running an export.
    pub fn validate(&self) -> Result<(), Error> {
        if self.tile_size == 0 {
            return Err(Error::InvalidOptions {
                reason: "tile_size must be greater than zero".into(),
            });
        }
        if self.max_prepared_frame_bytes == 0 {
            return Err(Error::InvalidOptions {
                reason: "max_prepared_frame_bytes must be greater than zero".into(),
            });
        }
        if self.max_instance_metadata_bytes == 0 {
            return Err(Error::InvalidOptions {
                reason: "max_instance_metadata_bytes must be greater than zero".into(),
            });
        }
        if self.max_total_metadata_bytes == 0 {
            return Err(Error::InvalidOptions {
                reason: "max_total_metadata_bytes must be greater than zero".into(),
            });
        }
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(Error::InvalidOptions {
                reason: "jpeg_quality must be in the range 1..=100".into(),
            });
        }
        if self.jpeg_optimize_huffman
            && self.transfer_syntax == TransferSyntax::JpegBaseline8Bit
            && self.encode_backend == EncodeBackendPreference::RequireDevice
        {
            return Err(Error::InvalidOptions {
                reason: "jpeg_optimize_huffman is a CPU encode and cannot be combined with encode_backend=RequireDevice".into(),
            });
        }
        if self.transfer_syntax == TransferSyntax::JpegLosslessSv1
            && self.encode_backend == EncodeBackendPreference::RequireDevice
        {
            return Err(Error::InvalidOptions {
                reason: "JPEG Lossless SV1 export encodes on the CPU and cannot be combined with encode_backend=RequireDevice".into(),
            });
        }
        if let Some(spacing) = self.source_pixel_spacing_mm {
            spacing.validate()?;
        }
        let profile = self.jpeg_direct_htj2k_profile;
        if self.transfer_syntax == TransferSyntax::Htj2k {
            if profile == JpegDirectHtj2kProfile::Lossless53 {
                return Err(Error::InvalidOptions {
                    reason: "HTJ2K transfer syntax 1.2.840.10008.1.2.4.203 requires an irreversible 9/7 jpeg_direct_htj2k_profile; use an HTJ2K Lossless transfer syntax for 5/3".into(),
                });
            }
        } else if profile.is_lossy_97() {
            return Err(Error::InvalidOptions {
                reason: format!(
                    "jpeg_direct_htj2k_profile={profile:?} requires transfer_syntax=Htj2k"
                ),
            });
        }
        if self.gpu_encode_inflight_tiles == Some(0) {
            return Err(Error::InvalidOptions {
                reason: "gpu_encode_inflight_tiles must be greater than zero when provided".into(),
            });
        }
        if self.gpu_encode_memory_mib == Some(0) {
            return Err(Error::InvalidOptions {
                reason: "gpu_encode_memory_mib must be greater than zero when provided".into(),
            });
        }
        if self.gpu_pipeline_depth == Some(0) {
            return Err(Error::InvalidOptions {
                reason: "gpu_pipeline_depth must be greater than zero when provided".into(),
            });
        }
        if self.gpu_row_batch_rows == Some(0) {
            return Err(Error::InvalidOptions {
                reason: "gpu_row_batch_rows must be greater than zero when provided".into(),
            });
        }
        if self.gpu_row_batch_target_tiles == Some(0) {
            return Err(Error::InvalidOptions {
                reason: "gpu_row_batch_target_tiles must be greater than zero when provided".into(),
            });
        }
        if let Some(memory_mib) = self.gpu_encode_memory_mib {
            let _ = usize::try_from(memory_mib)
                .ok()
                .and_then(|mib| mib.checked_mul(1024 * 1024))
                .ok_or_else(|| Error::InvalidOptions {
                    reason: "gpu_encode_memory_mib exceeds platform addressable memory".into(),
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_backend_requires_device_only_for_strict_device_preference() {
        assert!(!EncodeBackendPreference::Auto.requires_device());
        assert!(!EncodeBackendPreference::CpuOnly.requires_device());
        assert!(!EncodeBackendPreference::PreferDevice.requires_device());
        assert!(EncodeBackendPreference::RequireDevice.requires_device());
    }

    #[test]
    fn encode_backend_cpu_batch_safety_matches_backend_features() {
        assert!(EncodeBackendPreference::CpuOnly.cpu_batch_safe());
        assert_eq!(
            EncodeBackendPreference::Auto.cpu_batch_safe(),
            !cfg!(any(feature = "metal", feature = "cuda"))
        );
        assert!(!EncodeBackendPreference::PreferDevice.cpu_batch_safe());
        assert!(!EncodeBackendPreference::RequireDevice.cpu_batch_safe());
    }

    #[test]
    fn jpeg_direct_htj2k_profiles_expose_97_quality_scales() {
        assert_eq!(
            JpegDirectHtj2kProfile::Lossless53.irreversible_quantization_scale(),
            None
        );
        assert_eq!(
            JpegDirectHtj2kProfile::Lossy97Near.irreversible_quantization_scale(),
            Some(2.0)
        );
        assert_eq!(
            JpegDirectHtj2kProfile::Lossy97.irreversible_quantization_scale(),
            Some(5.0)
        );
        assert_eq!(
            JpegDirectHtj2kProfile::Lossy97Balanced.irreversible_quantization_scale(),
            Some(5.0)
        );
        assert_eq!(
            JpegDirectHtj2kProfile::Lossy97Aggressive.irreversible_quantization_scale(),
            Some(10.0)
        );
        assert_eq!(
            JpegDirectHtj2kProfile::Lossy97Preview.irreversible_quantization_scale(),
            Some(20.0)
        );
        assert_eq!(
            JpegDirectHtj2kProfile::Lossy97Thumbnail.irreversible_quantization_scale(),
            Some(50.0)
        );
    }

    #[test]
    fn validation_accepts_all_97_profiles_only_with_general_htj2k() {
        for profile in [
            JpegDirectHtj2kProfile::Lossy97Near,
            JpegDirectHtj2kProfile::Lossy97,
            JpegDirectHtj2kProfile::Lossy97Balanced,
            JpegDirectHtj2kProfile::Lossy97Aggressive,
            JpegDirectHtj2kProfile::Lossy97Preview,
            JpegDirectHtj2kProfile::Lossy97Thumbnail,
        ] {
            ExportOptions {
                transfer_syntax: TransferSyntax::Htj2k,
                jpeg_direct_htj2k_profile: profile,
                ..ExportOptions::default()
            }
            .validate()
            .unwrap();

            assert!(ExportOptions {
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                jpeg_direct_htj2k_profile: profile,
                ..ExportOptions::default()
            }
            .validate()
            .is_err());
        }

        assert!(ExportOptions {
            transfer_syntax: TransferSyntax::Htj2k,
            jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile::Lossless53,
            ..ExportOptions::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn export_options_round_trip_through_json_and_validate() {
        let options = ExportOptions {
            transfer_syntax: TransferSyntax::Htj2k,
            jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile::Lossy97Balanced,
            ..ExportOptions::default()
        };

        let json = serde_json::to_string(&options).expect("serialize options");
        assert!(json.contains("htj2k"));
        assert!(json.contains("lossy97-balanced"));

        let decoded: ExportOptions = serde_json::from_str(&json).expect("deserialize options");
        decoded.validate().expect("valid export options");

        assert_eq!(decoded.transfer_syntax, TransferSyntax::Htj2k);
        assert_eq!(
            decoded.jpeg_direct_htj2k_profile,
            JpegDirectHtj2kProfile::Lossy97Balanced
        );
    }

    #[test]
    fn internal_options_normalize_into_semantic_and_execution_groups() {
        let public = ExportOptions {
            tile_size: 256,
            max_prepared_frame_bytes: 64 * 1024 * 1024,
            max_instance_metadata_bytes: 32 * 1024 * 1024,
            max_total_metadata_bytes: 128 * 1024 * 1024,
            encode_backend: EncodeBackendPreference::PreferDevice,
            codec_validation: CodecValidation::RoundTrip,
            source_device_decode: true,
            gpu_encode_inflight_tiles: Some(8),
            gpu_encode_memory_mib: Some(2048),
            gpu_pipeline_depth: Some(3),
            gpu_row_batch_rows: Some(4),
            gpu_row_batch_target_tiles: Some(64),
            ..ExportOptions::default()
        };

        let normalized = NormalizedExportOptions::new(&public).expect("valid options");

        assert_eq!(normalized.semantics.tile_size, 256);
        assert_eq!(
            normalized.resources.max_prepared_frame_bytes,
            64 * 1024 * 1024
        );
        assert_eq!(
            normalized.resources.max_instance_metadata_bytes,
            32 * 1024 * 1024
        );
        assert_eq!(
            normalized.resources.max_total_metadata_bytes,
            128 * 1024 * 1024
        );
        assert_eq!(
            normalized.execution.encode_backend,
            EncodeBackendPreference::PreferDevice
        );
        assert_eq!(
            normalized.execution.codec_validation,
            CodecValidation::RoundTrip
        );
        assert!(normalized.execution.source_device_decode);
        assert_eq!(normalized.execution.gpu.encode_inflight_tiles, Some(8));
        assert_eq!(normalized.execution.gpu.encode_memory_mib, Some(2048));
        assert_eq!(normalized.execution.gpu.pipeline_depth, Some(3));
        assert_eq!(normalized.execution.gpu.row_batch_rows, Some(4));
        assert_eq!(normalized.execution.gpu.row_batch_target_tiles, Some(64));
    }

    #[test]
    fn internal_option_normalization_is_the_validation_boundary() {
        let invalid = ExportOptions {
            gpu_pipeline_depth: Some(0),
            ..ExportOptions::default()
        };

        let error = NormalizedExportOptions::new(&invalid).expect_err("invalid GPU tuning");
        assert!(error.to_string().contains("gpu_pipeline_depth"));
    }

    #[test]
    fn export_options_validation_rejects_zero_metadata_budgets() {
        for options in [
            ExportOptions {
                max_instance_metadata_bytes: 0,
                ..ExportOptions::default()
            },
            ExportOptions {
                max_total_metadata_bytes: 0,
                ..ExportOptions::default()
            },
        ] {
            let error = options.validate().expect_err("zero metadata budget");
            assert!(error.to_string().contains("metadata_bytes"));
        }
    }

    #[test]
    fn jpeg_optimize_huffman_requires_a_cpu_capable_backend_for_jpeg_baseline() {
        let optimized = ExportOptions {
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            jpeg_optimize_huffman: true,
            ..ExportOptions::default()
        };
        for backend in [
            EncodeBackendPreference::Auto,
            EncodeBackendPreference::CpuOnly,
            EncodeBackendPreference::PreferDevice,
        ] {
            ExportOptions {
                encode_backend: backend,
                ..optimized.clone()
            }
            .validate()
            .expect("CPU-capable backend accepts optimized Huffman tables");
        }
        let error = ExportOptions {
            encode_backend: EncodeBackendPreference::RequireDevice,
            ..optimized
        }
        .validate()
        .expect_err("RequireDevice cannot produce optimized Huffman tables");
        assert!(error.to_string().contains("jpeg_optimize_huffman"));
        assert!(!ExportOptions::default().jpeg_optimize_huffman);
    }

    #[test]
    fn jpeg_lossless_sv1_rejects_require_device_backend() {
        let options = ExportOptions {
            transfer_syntax: TransferSyntax::JpegLosslessSv1,
            ..ExportOptions::default()
        };
        options.validate().expect("CPU-capable backend is accepted");
        let error = ExportOptions {
            encode_backend: EncodeBackendPreference::RequireDevice,
            ..options
        }
        .validate()
        .expect_err("RequireDevice has no JPEG Lossless device encoder");
        assert!(error.to_string().contains("JPEG Lossless SV1"));
        assert_eq!(
            TransferSyntax::JpegLosslessSv1.uid(),
            "1.2.840.10008.1.2.4.70"
        );
    }

    #[test]
    fn pixel_reencode_policy_admits_lossy_targets_only_when_any() {
        assert_eq!(
            ExportOptions::default().pixel_reencode,
            PixelReencodePolicy::Lossless
        );
        for transfer_syntax in TransferSyntax::ALL {
            let lossless = matches!(
                transfer_syntax,
                TransferSyntax::Jpeg2000Lossless
                    | TransferSyntax::Htj2kLossless
                    | TransferSyntax::Htj2kLosslessRpcl
                    | TransferSyntax::JpegLosslessSv1
                    | TransferSyntax::ExplicitVrLittleEndian
            );
            assert!(!PixelReencodePolicy::Never.permits(transfer_syntax));
            assert_eq!(
                PixelReencodePolicy::Lossless.permits(transfer_syntax),
                lossless,
                "{transfer_syntax:?}"
            );
            assert!(PixelReencodePolicy::Any.permits(transfer_syntax));
        }
    }

    #[test]
    fn export_options_default_to_bounded_metadata_budgets() {
        let options = ExportOptions::default();
        assert_eq!(options.max_instance_metadata_bytes, 256 * 1024 * 1024);
        assert_eq!(options.max_total_metadata_bytes, 1024 * 1024 * 1024);
    }

    #[test]
    fn export_options_preserve_every_field_through_json() {
        let options = ExportOptions {
            tile_size: 384,
            overwrite: true,
            max_prepared_frame_bytes: 128 * 1024 * 1024,
            max_instance_metadata_bytes: 64 * 1024 * 1024,
            max_total_metadata_bytes: 512 * 1024 * 1024,
            transfer_syntax: TransferSyntax::Htj2k,
            jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile::Lossy97Aggressive,
            jpeg_quality: 77,
            jpeg_optimize_huffman: true,
            pixel_reencode: PixelReencodePolicy::Any,
            uid_policy: UidPolicy::Deterministic,
            source_pixel_spacing_mm: Some(SourcePixelSpacingMm::new(0.0005, 0.00025).unwrap()),
            encode_backend: EncodeBackendPreference::PreferDevice,
            codec_validation: CodecValidation::RoundTrip,
            source_device_decode: true,
            j2k_decomposition_levels: Some(4),
            gpu_encode_inflight_tiles: Some(8),
            gpu_encode_memory_mib: Some(4096),
            gpu_pipeline_depth: Some(3),
            gpu_row_batch_rows: Some(6),
            gpu_row_batch_target_tiles: Some(96),
        };

        let json = serde_json::to_string(&options).expect("serialize options");
        let round_tripped: ExportOptions =
            serde_json::from_str(&json).expect("deserialize options");
        round_tripped
            .validate()
            .expect("valid options should validate");

        assert_eq!(round_tripped, options);
    }

    #[test]
    fn export_options_deserialize_missing_fields_from_defaults() {
        let options: ExportOptions =
            serde_json::from_str(r#"{"transfer_syntax":"jpeg-baseline8-bit"}"#)
                .expect("partial persisted options should use defaults");

        assert_eq!(options.transfer_syntax, TransferSyntax::JpegBaseline8Bit);
        assert_eq!(options.tile_size, ExportOptions::default().tile_size);
        assert_eq!(options.jpeg_quality, ExportOptions::default().jpeg_quality);
        assert_eq!(
            options.jpeg_direct_htj2k_profile,
            ExportOptions::default().jpeg_direct_htj2k_profile
        );
    }

    #[test]
    fn export_option_presets_select_expected_transfer_syntaxes() {
        let lossless = ExportOptions::lossless_review();
        assert_eq!(lossless.transfer_syntax, TransferSyntax::Htj2kLosslessRpcl);
        assert_eq!(lossless.tile_size, 512);
        assert_eq!(lossless.jpeg_quality, 90);
        let preset_lossless = ExportPreset::LosslessReview.options(384, 85);
        assert_eq!(
            preset_lossless.transfer_syntax,
            TransferSyntax::Htj2kLosslessRpcl
        );
        assert_eq!(preset_lossless.tile_size, 384);
        assert_eq!(preset_lossless.jpeg_quality, 85);

        let fast = ExportOptions::fast_jpeg(256, 80);
        assert_eq!(fast.transfer_syntax, TransferSyntax::JpegBaseline8Bit);
        assert_eq!(fast.tile_size, 256);
        assert_eq!(fast.jpeg_quality, 80);
        assert_eq!(
            ExportPreset::FastJpeg.options(256, 80),
            ExportOptions::fast_jpeg(256, 80)
        );
    }

    #[test]
    fn transfer_syntax_all_contains_each_uid_once() {
        let mut uids = std::collections::BTreeSet::new();
        for transfer_syntax in TransferSyntax::ALL {
            assert!(
                uids.insert(transfer_syntax.uid()),
                "duplicate transfer syntax UID {}",
                transfer_syntax.uid()
            );
        }
        assert_eq!(uids.len(), 8);
    }

    #[test]
    fn dicom_export_preset_serializes_as_kebab_case() {
        assert_eq!(
            serde_json::to_string(&ExportPreset::LosslessReview).unwrap(),
            "\"lossless-review\""
        );
        assert_eq!(
            serde_json::to_string(&ExportPreset::FastJpeg).unwrap(),
            "\"fast-jpeg\""
        );
    }
}
