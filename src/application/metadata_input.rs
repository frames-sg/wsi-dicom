use std::path::PathBuf;

use crate::{Error, ExportWorkflowError, ExportWorkflowStage, MetadataSource};

/// User-facing selection of exactly one metadata source for an export workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MetadataInput {
    /// Load DICOM JSON or a supported FHIR resource from this bounded file.
    JsonFile(PathBuf),
    /// Use explicit deterministic non-clinical placeholder metadata.
    ResearchPlaceholder,
}

impl MetadataInput {
    /// Resolve mutually exclusive frontend inputs into one metadata selection.
    pub fn from_parts(
        metadata_path: Option<PathBuf>,
        research_placeholder: bool,
    ) -> Result<Self, ExportWorkflowError> {
        match (metadata_path, research_placeholder) {
            (Some(_), true) => Err(ExportWorkflowError::new(
                ExportWorkflowStage::Metadata,
                Error::Metadata {
                    reason: "metadata JSON cannot be combined with research placeholder metadata"
                        .into(),
                },
            )),
            (Some(path), false) => Ok(Self::JsonFile(path)),
            (None, true) => Ok(Self::ResearchPlaceholder),
            (None, false) => Err(ExportWorkflowError::new(
                ExportWorkflowStage::Metadata,
                Error::Metadata {
                    reason:
                        "provide metadata JSON or explicitly select research placeholder metadata"
                            .into(),
                },
            )),
        }
    }

    pub(crate) fn resolve(self) -> Result<MetadataSource, ExportWorkflowError> {
        match self {
            Self::JsonFile(path) => MetadataSource::from_json_file(path)
                .map_err(|error| ExportWorkflowError::new(ExportWorkflowStage::Metadata, error)),
            Self::ResearchPlaceholder => Ok(MetadataSource::ResearchPlaceholder),
        }
    }
}
