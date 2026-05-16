use serde::{Deserialize, Serialize};

use crate::WsiDicomError;

/// Runtime preference for JPEG 2000 Lossless encode backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncodeBackendPreference {
    Auto,
    CpuOnly,
    PreferDevice,
    RequireDevice,
}

impl EncodeBackendPreference {
    pub(crate) fn to_signinum(self) -> signinum_j2k::EncodeBackendPreference {
        match self {
            Self::Auto => signinum_j2k::EncodeBackendPreference::Auto,
            Self::CpuOnly => signinum_j2k::EncodeBackendPreference::CpuOnly,
            Self::PreferDevice => signinum_j2k::EncodeBackendPreference::PreferDevice,
            Self::RequireDevice => signinum_j2k::EncodeBackendPreference::RequireDevice,
        }
    }
}

/// Runtime validation policy for newly encoded compressed frame bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodecValidation {
    Disabled,
    RoundTrip,
}

impl CodecValidation {
    pub(crate) fn to_j2k_validation(self) -> signinum_j2k::J2kEncodeValidation {
        match self {
            Self::Disabled => signinum_j2k::J2kEncodeValidation::External,
            Self::RoundTrip => signinum_j2k::J2kEncodeValidation::CpuRoundTrip,
        }
    }
}

/// DICOM transfer syntax choices for exported VL Whole Slide Microscopy files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferSyntax {
    JpegBaseline8Bit,
    Jpeg2000,
    Jpeg2000Lossless,
    Htj2kLossless,
    Htj2kLosslessRpcl,
    ExplicitVrLittleEndian,
}

impl TransferSyntax {
    pub fn uid(self) -> &'static str {
        match self {
            Self::JpegBaseline8Bit => "1.2.840.10008.1.2.4.50",
            Self::Jpeg2000 => "1.2.840.10008.1.2.4.91",
            Self::Jpeg2000Lossless => "1.2.840.10008.1.2.4.90",
            Self::Htj2kLossless => "1.2.840.10008.1.2.4.201",
            Self::Htj2kLosslessRpcl => "1.2.840.10008.1.2.4.202",
            Self::ExplicitVrLittleEndian => "1.2.840.10008.1.2.1",
        }
    }

    pub(crate) fn is_j2k_family(self) -> bool {
        matches!(
            self,
            Self::Jpeg2000 | Self::Jpeg2000Lossless | Self::Htj2kLossless | Self::Htj2kLosslessRpcl
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

/// Options controlling how a source WSI should be converted into DICOM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DicomExportOptions {
    pub tile_size: u32,
    pub transfer_syntax: TransferSyntax,
    pub jpeg_quality: u8,
    pub encode_backend: EncodeBackendPreference,
    pub codec_validation: CodecValidation,
    pub source_device_decode: bool,
    pub j2k_decomposition_levels: Option<u8>,
    pub gpu_encode_inflight_tiles: Option<usize>,
    pub gpu_encode_memory_mib: Option<u64>,
    pub gpu_pipeline_depth: Option<usize>,
    pub gpu_row_batch_rows: Option<usize>,
    pub gpu_row_batch_target_tiles: Option<usize>,
}

impl Default for DicomExportOptions {
    fn default() -> Self {
        Self {
            tile_size: 512,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            jpeg_quality: 90,
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

impl DicomExportOptions {
    pub fn validate(&self) -> Result<(), WsiDicomError> {
        if self.tile_size == 0 {
            return Err(WsiDicomError::InvalidOptions {
                reason: "tile_size must be greater than zero".into(),
            });
        }
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(WsiDicomError::InvalidOptions {
                reason: "jpeg_quality must be in the range 1..=100".into(),
            });
        }
        if self.gpu_encode_inflight_tiles == Some(0) {
            return Err(WsiDicomError::InvalidOptions {
                reason: "gpu_encode_inflight_tiles must be greater than zero when provided".into(),
            });
        }
        if self.gpu_encode_memory_mib == Some(0) {
            return Err(WsiDicomError::InvalidOptions {
                reason: "gpu_encode_memory_mib must be greater than zero when provided".into(),
            });
        }
        if self.gpu_pipeline_depth == Some(0) {
            return Err(WsiDicomError::InvalidOptions {
                reason: "gpu_pipeline_depth must be greater than zero when provided".into(),
            });
        }
        if self.gpu_row_batch_rows == Some(0) {
            return Err(WsiDicomError::InvalidOptions {
                reason: "gpu_row_batch_rows must be greater than zero when provided".into(),
            });
        }
        if self.gpu_row_batch_target_tiles == Some(0) {
            return Err(WsiDicomError::InvalidOptions {
                reason: "gpu_row_batch_target_tiles must be greater than zero when provided".into(),
            });
        }
        if let Some(memory_mib) = self.gpu_encode_memory_mib {
            let _ = usize::try_from(memory_mib)
                .ok()
                .and_then(|mib| mib.checked_mul(1024 * 1024))
                .ok_or_else(|| WsiDicomError::InvalidOptions {
                    reason: "gpu_encode_memory_mib exceeds platform addressable memory".into(),
                })?;
        }
        Ok(())
    }
}
