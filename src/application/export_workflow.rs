use std::error::Error as StdError;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;

use super::MetadataInput;
use crate::{
    export_dicom, validate_dicom_path, AnnotationCoordinateSpace, ColorManagement, Error,
    ExportOptions, ExportReport, ExportRequest, QuPathAnnotationOptions, ValidationOptions,
    ValidationReport,
};

/// Stage of the application workflow responsible for an outcome or failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExportWorkflowStage {
    /// Resolve and validate the selected metadata input.
    Metadata,
    /// Read and normalize annotation inputs before the WSI export.
    Annotations,
    /// Open the source, encode instances, and publish the WSI generation.
    Export,
    /// Validate the completed output when requested.
    Validation,
    /// Serialize or persist the combined machine report.
    Report,
}

impl fmt::Display for ExportWorkflowStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Metadata => "metadata",
            Self::Annotations => "annotations",
            Self::Export => "export",
            Self::Validation => "validation",
            Self::Report => "report",
        };
        formatter.write_str(label)
    }
}

/// Structured error preserving the application stage and underlying library error.
#[derive(Debug)]
pub struct ExportWorkflowError {
    stage: ExportWorkflowStage,
    source: Error,
}

impl ExportWorkflowError {
    pub(crate) const fn new(stage: ExportWorkflowStage, source: Error) -> Self {
        Self { stage, source }
    }

    /// Return the stage that failed.
    #[must_use]
    pub const fn stage(&self) -> ExportWorkflowStage {
        self.stage
    }

    /// Return the structured library error that caused the failure.
    #[must_use]
    pub const fn source_error(&self) -> &Error {
        &self.source
    }
}

impl fmt::Display for ExportWorkflowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} stage failed: {}", self.stage, self.source)
    }
}

impl StdError for ExportWorkflowError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.source)
    }
}

/// Coarse progress event emitted at application workflow boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportWorkflowProgressEvent {
    /// Metadata input is being resolved.
    ResolvingMetadata,
    /// Annotation inputs are being prepared.
    PreparingAnnotations,
    /// WSI export is running.
    Exporting,
    /// Optional DICOM validation is running.
    Validating,
    /// Optional machine report persistence is running.
    PersistingReport,
    /// Every requested workflow stage completed.
    Complete,
}

/// Thread-safe receiver for coarse application workflow progress.
pub trait ExportWorkflowProgress: Send + Sync {
    /// Receive a progress event. Implementations should return promptly.
    fn on_progress(&self, event: ExportWorkflowProgressEvent);
}

/// Frontend-independent request for export, annotations, validation, and reporting.
pub struct ExportWorkflowRequest {
    /// Source slide path.
    pub source_path: PathBuf,
    /// Destination directory for the exported DICOM generation.
    pub output_dir: PathBuf,
    /// Flat public export options; normalized by the export preparation boundary.
    pub options: ExportOptions,
    /// Required color-management behavior.
    pub color_management: ColorManagement,
    /// Exactly one metadata selection.
    pub metadata: MetadataInput,
    /// Optional source pyramid level filter.
    pub level_filter: Option<u32>,
    /// Optional prepared-before-export QuPath annotation conversion.
    pub annotations: Option<QuPathAnnotationOptions>,
    /// Optional validation to run against the completed output.
    pub validation: Option<ValidationOptions>,
    /// Optional destination for the combined JSON report.
    pub report_path: Option<PathBuf>,
    /// Optional coarse progress receiver.
    pub progress: Option<Arc<dyn ExportWorkflowProgress>>,
}

impl ExportWorkflowRequest {
    /// Create a workflow request with optional stages disabled.
    #[must_use]
    pub fn new(
        source_path: impl Into<PathBuf>,
        output_dir: impl Into<PathBuf>,
        options: ExportOptions,
        color_management: ColorManagement,
        metadata: MetadataInput,
    ) -> Self {
        Self {
            source_path: source_path.into(),
            output_dir: output_dir.into(),
            options,
            color_management,
            metadata,
            level_filter: None,
            annotations: None,
            validation: None,
            report_path: None,
            progress: None,
        }
    }
}

/// Combined machine report from the shared application workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ExportWorkflowReport {
    /// WSI export and optional annotation sidecar report.
    pub export: ExportReport,
    /// Optional DICOM validation report.
    pub validation: Option<ValidationReport>,
}

impl ExportWorkflowReport {
    /// Serialize the stable combined report shape as pretty JSON.
    pub fn to_pretty_json(&self) -> Result<String, Error> {
        serde_json::to_string_pretty(self).map_err(|error| Error::JsonSerialize {
            message: error.to_string(),
        })
    }

    /// Build a concise frontend-neutral status summary.
    #[must_use]
    pub fn summary(&self) -> String {
        let annotation_instances = self
            .export
            .annotations
            .as_ref()
            .map_or(0, |annotations| annotations.instances.len());
        match &self.validation {
            Some(validation) => format!(
                "Exported {} WSI instance(s) and {} annotation sidecar(s); validation passed={} failed={} skipped={}.",
                self.export.instances.len(),
                annotation_instances,
                validation.passed_checks(),
                validation.failed_checks(),
                validation.skipped_checks(),
            ),
            None => format!(
                "Exported {} WSI instance(s) and {} annotation sidecar(s).",
                self.export.instances.len(),
                annotation_instances,
            ),
        }
    }
}

/// Execute the frontend-independent export workflow.
pub fn run_export_workflow(
    request: ExportWorkflowRequest,
) -> Result<ExportWorkflowReport, ExportWorkflowError> {
    emit(
        &request.progress,
        ExportWorkflowProgressEvent::ResolvingMetadata,
    );
    let metadata = request.metadata.resolve()?;

    emit(
        &request.progress,
        ExportWorkflowProgressEvent::PreparingAnnotations,
    );
    let prepared_annotations = match request.annotations {
        Some(annotations) => {
            if annotations.coordinate_space == AnnotationCoordinateSpace::Level0Pixels
                && request.level_filter.is_some_and(|level| level != 0)
            {
                return Err(ExportWorkflowError::new(
                    ExportWorkflowStage::Annotations,
                    Error::InvalidOptions {
                        reason: "level-zero QuPath coordinates require exporting level 0".into(),
                    },
                ));
            }
            Some(annotations.prepare().map_err(|error| {
                ExportWorkflowError::new(ExportWorkflowStage::Annotations, error)
            })?)
        }
        None => None,
    };

    emit(&request.progress, ExportWorkflowProgressEvent::Exporting);
    let mut export = export_dicom(ExportRequest {
        source_path: request.source_path,
        output_dir: request.output_dir.clone(),
        options: request.options,
        color_management: request.color_management,
        metadata,
        level_filter: request.level_filter,
    })
    .map_err(|error| ExportWorkflowError::new(ExportWorkflowStage::Export, error))?;

    if let Some(annotations) = prepared_annotations {
        export.annotations =
            Some(annotations.export_for(&export).map_err(|error| {
                ExportWorkflowError::new(ExportWorkflowStage::Annotations, error)
            })?);
    }

    let validation = match request.validation {
        Some(options) => {
            emit(&request.progress, ExportWorkflowProgressEvent::Validating);
            Some(
                validate_dicom_path(&request.output_dir, &options).map_err(|error| {
                    ExportWorkflowError::new(ExportWorkflowStage::Validation, error)
                })?,
            )
        }
        None => None,
    };

    let report = ExportWorkflowReport { export, validation };
    if let Some(path) = request.report_path {
        emit(
            &request.progress,
            ExportWorkflowProgressEvent::PersistingReport,
        );
        let json = report
            .to_pretty_json()
            .map_err(|error| ExportWorkflowError::new(ExportWorkflowStage::Report, error))?;
        std::fs::write(&path, json).map_err(|source| {
            ExportWorkflowError::new(ExportWorkflowStage::Report, Error::Io { path, source })
        })?;
    }
    emit(&request.progress, ExportWorkflowProgressEvent::Complete);
    Ok(report)
}

fn emit(progress: &Option<Arc<dyn ExportWorkflowProgress>>, event: ExportWorkflowProgressEvent) {
    if let Some(progress) = progress {
        progress.on_progress(event);
    }
}
