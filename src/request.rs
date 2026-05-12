//! Request types accepted by the public export, profile, coverage, and encode APIs.

use std::path::PathBuf;
use std::time::Duration;

use signinum_j2k::J2kLosslessSamples;

use crate::{
    CodecValidation, DicomExportOptions, EncodeBackendPreference, MetadataSource, TransferSyntax,
    WsiDicomError,
};

/// A validated request to export one vendor WSI into one DICOM output directory.
#[derive(Debug, Clone, PartialEq)]
pub struct DicomExportRequest {
    pub source_path: PathBuf,
    pub output_dir: PathBuf,
    pub options: DicomExportOptions,
    pub metadata: MetadataSource,
    pub level_filter: Option<u32>,
}

impl DicomExportRequest {
    pub fn new(
        source_path: PathBuf,
        output_dir: PathBuf,
        options: DicomExportOptions,
    ) -> Result<Self, WsiDicomError> {
        options.validate()?;
        Ok(Self {
            source_path,
            output_dir,
            options,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
    }

    pub fn validate(&self) -> Result<(), WsiDicomError> {
        self.options.validate()
    }
}

/// Request to encode one already-composed tile into DICOM-ready J2K/HTJ2K frame bytes.
#[derive(Debug, Clone, Copy)]
pub struct DicomJ2kFrameEncodeRequest<'a> {
    pub samples: J2kLosslessSamples<'a>,
    pub transfer_syntax: TransferSyntax,
    pub encode_backend: EncodeBackendPreference,
    pub codec_validation: CodecValidation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DicomRouteProfileRequest {
    pub source_path: PathBuf,
    pub options: DicomExportOptions,
    pub level: u32,
    pub max_frames: u64,
}

/// Request for source-aware default transfer syntax selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultTransferSyntaxRequest {
    pub source_path: PathBuf,
    pub tile_size: u32,
    pub level_filter: Option<u32>,
    pub max_levels: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DicomRouteCoverageRequest {
    pub source_path: PathBuf,
    pub options: DicomExportOptions,
    pub max_frames_per_level: u64,
    pub max_levels: Option<u32>,
    pub max_level_elapsed: Option<Duration>,
    pub progress: Option<DicomRouteCoverageProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DicomRouteCoverageProgress {
    Stderr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DicomRouteCorpusCoverageRequest {
    pub source_root: PathBuf,
    pub options: DicomExportOptions,
    pub max_frames_per_level: u64,
    pub max_levels: Option<u32>,
    pub max_level_elapsed: Option<Duration>,
    pub progress: Option<DicomRouteCorpusCoverageProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DicomRouteCorpusCoverageProgress {
    Stderr,
}
