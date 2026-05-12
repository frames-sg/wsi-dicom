//! Builder-first Rust API for DICOM export.

use std::path::PathBuf;

use crate::{
    default_transfer_syntax_for_source, export_dicom, DefaultTransferSyntaxRequest,
    DicomExportOptions, DicomExportReport, DicomExportRequest, MetadataSource, TransferSyntax,
    WsiDicomError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferSyntaxSelection {
    SourceAware,
    Explicit,
}

/// Builder for exporting one statumen-readable whole-slide image to DICOM VL WSI.
#[derive(Debug, Clone)]
pub struct DicomExport {
    source_path: PathBuf,
    output_dir: Option<PathBuf>,
    options: DicomExportOptions,
    metadata: MetadataSource,
    level_filter: Option<u32>,
    transfer_syntax: TransferSyntaxSelection,
}

impl DicomExport {
    /// Start an export builder from a source slide path.
    pub fn from_slide(source_path: impl Into<PathBuf>) -> Self {
        Self {
            source_path: source_path.into(),
            output_dir: None,
            options: DicomExportOptions::default(),
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
            transfer_syntax: TransferSyntaxSelection::SourceAware,
        }
    }

    /// Set the output directory for generated DICOM instances.
    pub fn to_directory(mut self, output_dir: impl Into<PathBuf>) -> Self {
        self.output_dir = Some(output_dir.into());
        self
    }

    /// Replace export options and treat their transfer syntax as explicit.
    pub fn with_options(mut self, options: DicomExportOptions) -> Self {
        self.options = options;
        self.transfer_syntax = TransferSyntaxSelection::Explicit;
        self
    }

    /// Replace metadata used for DICOM conformance fields.
    pub fn with_metadata(mut self, metadata: MetadataSource) -> Self {
        self.metadata = metadata;
        self
    }

    /// Export only one source pyramid level.
    pub fn level(mut self, level: u32) -> Self {
        self.level_filter = Some(level);
        self
    }

    /// Explicitly set the output transfer syntax.
    pub fn transfer_syntax(mut self, transfer_syntax: TransferSyntax) -> Self {
        self.options.transfer_syntax = transfer_syntax;
        self.transfer_syntax = TransferSyntaxSelection::Explicit;
        self
    }

    /// Resolve the transfer syntax from the source at run time.
    pub fn source_aware_transfer_syntax(mut self) -> Self {
        self.transfer_syntax = TransferSyntaxSelection::SourceAware;
        self
    }

    /// Convert the builder into an export request, resolving source-aware defaults.
    pub fn build_request(mut self) -> Result<DicomExportRequest, WsiDicomError> {
        let output_dir = self
            .output_dir
            .take()
            .ok_or_else(|| WsiDicomError::InvalidOptions {
                reason: "output directory must be configured with to_directory".into(),
            })?;
        if self.transfer_syntax == TransferSyntaxSelection::SourceAware {
            self.options.transfer_syntax =
                default_transfer_syntax_for_source(DefaultTransferSyntaxRequest {
                    source_path: self.source_path.clone(),
                    tile_size: self.options.tile_size,
                    level_filter: self.level_filter,
                    max_levels: None,
                })?;
        }
        self.options.validate()?;
        Ok(DicomExportRequest {
            source_path: self.source_path,
            output_dir,
            options: self.options,
            metadata: self.metadata,
            level_filter: self.level_filter,
        })
    }

    /// Run the export and return a report.
    pub fn run(self) -> Result<DicomExportReport, WsiDicomError> {
        export_dicom(self.build_request()?)
    }
}
