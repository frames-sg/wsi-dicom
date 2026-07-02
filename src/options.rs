use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::Error;

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

/// Policy for DICOM Optical Path ICC profile handling when source color
/// metadata is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[non_exhaustive]
pub enum IccProfilePolicy {
    /// Require a real source or embedded JPEG ICC profile.
    Strict,
    /// Preserve source ICC when available; otherwise embed a synthesized sRGB
    /// ICC profile and report it as an assumption.
    FallbackSrgb,
    /// Preserve source ICC when available; otherwise embed a synthesized
    /// Display P3 ICC profile and report it as an assumption.
    FallbackDisplayP3,
    /// Preserve source ICC when available; otherwise omit the ICC Profile
    /// attribute.
    OmitIfMissing,
}

/// DICOM transfer syntax choices for exported VL Whole Slide Microscopy files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TransferSyntax {
    /// JPEG Baseline 8-bit transfer syntax.
    JpegBaseline8Bit,
    /// JPEG 2000 Image Compression transfer syntax.
    Jpeg2000,
    /// JPEG 2000 Image Compression Lossless Only transfer syntax.
    Jpeg2000Lossless,
    /// High-Throughput JPEG 2000 Image Compression transfer syntax.
    Htj2k,
    /// High-Throughput JPEG 2000 Image Compression Lossless Only transfer syntax.
    Htj2kLossless,
    /// High-Throughput JPEG 2000 with RPCL Options Image Compression Lossless Only transfer syntax.
    Htj2kLosslessRpcl,
    /// Explicit VR Little Endian transfer syntax for uncompressed input fixtures.
    #[value(skip)]
    ExplicitVrLittleEndian,
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
    pub const ALL: [Self; 7] = [
        Self::JpegBaseline8Bit,
        Self::Jpeg2000,
        Self::Jpeg2000Lossless,
        Self::Htj2k,
        Self::Htj2kLossless,
        Self::Htj2kLosslessRpcl,
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
    /// Requested output transfer syntax.
    pub transfer_syntax: TransferSyntax,
    /// Direct JPEG-to-HTJ2K profile used when eligible.
    pub jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile,
    /// JPEG quality used for JPEG Baseline fallback encoding.
    pub jpeg_quality: u8,
    /// ICC profile policy for missing source color metadata.
    pub icc_profile_policy: IccProfilePolicy,
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

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            tile_size: 512,
            overwrite: false,
            max_prepared_frame_bytes: 256 * 1024 * 1024,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile::Lossless53,
            jpeg_quality: 90,
            icc_profile_policy: IccProfilePolicy::FallbackSrgb,
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
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(Error::InvalidOptions {
                reason: "jpeg_quality must be in the range 1..=100".into(),
            });
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
    fn export_options_validation_rejects_invalid_options() {
        let options = ExportOptions {
            jpeg_quality: 0,
            ..ExportOptions::default()
        };

        let err = options.validate().expect_err("invalid quality");
        assert!(err.to_string().contains("jpeg_quality"));
    }

    #[test]
    fn export_options_preserve_every_field_through_json() {
        let options = ExportOptions {
            tile_size: 384,
            overwrite: true,
            max_prepared_frame_bytes: 128 * 1024 * 1024,
            transfer_syntax: TransferSyntax::Htj2k,
            jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile::Lossy97Aggressive,
            jpeg_quality: 77,
            icc_profile_policy: IccProfilePolicy::OmitIfMissing,
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
        assert_eq!(uids.len(), 7);
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
