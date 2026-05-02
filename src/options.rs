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

/// DICOM transfer syntax choices for exported VL Whole Slide Microscopy files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferSyntax {
    JpegBaseline8Bit,
    Jpeg2000Lossless,
    ExplicitVrLittleEndian,
}

impl TransferSyntax {
    pub fn uid(self) -> &'static str {
        match self {
            Self::JpegBaseline8Bit => "1.2.840.10008.1.2.4.50",
            Self::Jpeg2000Lossless => "1.2.840.10008.1.2.4.90",
            Self::ExplicitVrLittleEndian => "1.2.840.10008.1.2.1",
        }
    }
}

/// Options controlling how a source WSI should be converted into DICOM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DicomExportOptions {
    pub tile_size: u32,
    pub transfer_syntax: TransferSyntax,
    pub encode_backend: EncodeBackendPreference,
}

impl Default for DicomExportOptions {
    fn default() -> Self {
        Self {
            tile_size: 512,
            transfer_syntax: TransferSyntax::Jpeg2000Lossless,
            encode_backend: EncodeBackendPreference::Auto,
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
        Ok(())
    }
}
