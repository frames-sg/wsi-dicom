//! QuPath GeoJSON conversion tied to a completed VL WSI export.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use wsi_dicom_annotations::{
    DiagnosticDisposition, DiagnosticSeverity, DicomAnnotationContext, DicomBundlePublication,
    PathologyAnnotationSet, PathologyCoordinateSpace, PathologyDicomDocuments,
    PathologyDicomTarget, PathologyDocumentWriteError,
};

use crate::{Error, ExportReport, InstanceReport};

const MAX_GEOJSON_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MAPPING_BYTES: u64 = 4 * 1024 * 1024;
const ANN_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.91.1";
const SEG_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.66.4";
const COMPREHENSIVE_3D_SR_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.88.34";

/// Coordinate convention used by a QuPath GeoJSON annotation export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationCoordinateSpace {
    /// Coordinates are pixels in the highest-resolution source image.
    #[default]
    Level0Pixels,
    /// Coordinates are pixels in the selected DICOM WSI instance.
    SourcePixels,
    /// Coordinates are millimetres in the DICOM slide coordinate system.
    SlideMillimeters,
}

impl From<AnnotationCoordinateSpace> for PathologyCoordinateSpace {
    fn from(value: AnnotationCoordinateSpace) -> Self {
        match value {
            AnnotationCoordinateSpace::Level0Pixels => Self::Level0Pixels,
            AnnotationCoordinateSpace::SourcePixels => Self::SourcePixels,
            AnnotationCoordinateSpace::SlideMillimeters => Self::SlideMillimeters,
        }
    }
}

/// DICOM derived-object representation requested for mapped QuPath content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationTarget {
    /// Microscopy Bulk Simple Annotations for points and simple polygons.
    Ann,
    /// DICOM Segmentation for mask-style regions, components, and holes.
    Seg,
    /// Comprehensive 3D SR for mapped measurements and coded evaluations.
    Sr,
}

impl AnnotationTarget {
    const fn pathology_target(self) -> PathologyDicomTarget {
        match self {
            Self::Ann => PathologyDicomTarget::Ann,
            Self::Seg => PathologyDicomTarget::Seg,
            Self::Sr => PathologyDicomTarget::Sr,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Ann => "ann",
            Self::Seg => "seg",
            Self::Sr => "sr",
        }
    }

    const fn file_name(self) -> &'static str {
        match self {
            Self::Ann => "ann.dcm",
            Self::Seg => "seg.dcm",
            Self::Sr => "sr.dcm",
        }
    }

    const fn sop_class_uid(self) -> &'static str {
        match self {
            Self::Ann => ANN_STORAGE_UID,
            Self::Seg => SEG_STORAGE_UID,
            Self::Sr => COMPREHENSIVE_3D_SR_STORAGE_UID,
        }
    }
}

/// Inputs and semantic choices for converting one QuPath GeoJSON export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuPathAnnotationOptions {
    /// QuPath GeoJSON file to convert.
    pub geojson_path: PathBuf,
    /// Explicit mapping from QuPath classes and properties to coded DICOM concepts.
    pub mapping_path: PathBuf,
    /// Coordinate convention used in the GeoJSON file.
    pub coordinate_space: AnnotationCoordinateSpace,
    /// Derived DICOM object types to create; at least one unique target is required.
    pub targets: Vec<AnnotationTarget>,
    /// Permit only the lossy normalizations explicitly supported by the mapping converter.
    pub allow_lossy: bool,
}

impl QuPathAnnotationOptions {
    /// Create options using the usual QuPath level-zero pixel convention.
    #[must_use]
    pub fn new(
        geojson_path: impl Into<PathBuf>,
        mapping_path: impl Into<PathBuf>,
        targets: Vec<AnnotationTarget>,
    ) -> Self {
        Self {
            geojson_path: geojson_path.into(),
            mapping_path: mapping_path.into(),
            coordinate_space: AnnotationCoordinateSpace::Level0Pixels,
            targets,
            allow_lossy: false,
        }
    }

    pub(crate) fn prepare(&self) -> Result<PreparedQuPathAnnotations, Error> {
        validate_targets(&self.targets)?;
        Ok(PreparedQuPathAnnotations {
            options: self.clone(),
            geojson: read_bounded_file(&self.geojson_path, MAX_GEOJSON_BYTES, "QuPath GeoJSON")?,
            mapping: read_bounded_file(
                &self.mapping_path,
                MAX_MAPPING_BYTES,
                "annotation mapping",
            )?,
        })
    }
}

/// Structured diagnostic emitted while normalizing profiled annotation content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnnotationDiagnosticReport {
    /// Stable diagnostic code.
    pub code: String,
    /// `info`, `warning`, or `error`.
    pub severity: &'static str,
    /// JSON path or semantic location associated with the diagnostic.
    pub path: String,
    /// `normalized`, `would_drop`, or `unsupported`.
    pub disposition: &'static str,
    /// Human-readable explanation.
    pub message: String,
}

/// One verified DICOM annotation object created by the combined conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnnotationInstanceReport {
    /// DICOM representation written for this object.
    pub target: AnnotationTarget,
    /// Published DICOM file path.
    pub path: PathBuf,
    /// SOP Class UID of the derived object.
    pub sop_class_uid: &'static str,
    /// SOP Instance UID assigned to this export.
    pub sop_instance_uid: String,
    /// Series Instance UID assigned to this export.
    pub series_instance_uid: String,
}

/// Verified annotation sidecars produced for one WSI conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnnotationExportReport {
    /// Atomically published sidecar directory.
    pub output_dir: PathBuf,
    /// Unique level-zero VL WSI instance referenced by every sidecar.
    pub source_wsi: PathBuf,
    /// Number of profiled GeoJSON features converted.
    pub feature_count: usize,
    /// SHA-256 of normalized annotation semantics and source-space geometry.
    pub semantic_sha256: String,
    /// SHA-256 of the exact QuPath GeoJSON input bytes.
    pub geojson_sha256: String,
    /// SHA-256 of the exact mapping input bytes.
    pub mapping_sha256: String,
    /// Explicit coordinate convention used for conversion.
    pub coordinate_space: AnnotationCoordinateSpace,
    /// Normalization and interoperability diagnostics.
    pub diagnostics: Vec<AnnotationDiagnosticReport>,
    /// Verified DICOM annotation objects.
    pub instances: Vec<AnnotationInstanceReport>,
}

pub(crate) struct PreparedQuPathAnnotations {
    options: QuPathAnnotationOptions,
    geojson: Vec<u8>,
    mapping: Vec<u8>,
}

impl PreparedQuPathAnnotations {
    pub(crate) fn export_for(&self, wsi: &ExportReport) -> Result<AnnotationExportReport, Error> {
        let source_instance = select_annotation_source(wsi, self.options.coordinate_space)?;
        let source =
            DicomAnnotationContext::from_source(&source_instance.path).map_err(annotation_error)?;
        let annotations = PathologyAnnotationSet::from_json(
            &self.geojson,
            &self.mapping,
            &source,
            &source,
            self.options.coordinate_space.into(),
            self.options.allow_lossy,
        )
        .map_err(annotation_error)?;
        let targets = self
            .options
            .targets
            .iter()
            .copied()
            .map(AnnotationTarget::pathology_target)
            .collect::<Vec<_>>();
        let documents =
            PathologyDicomDocuments::build(&annotations, &targets).map_err(annotation_error)?;
        let output_dir = wsi.output_dir.join("annotations");
        let protected_inputs = [
            source_instance.path.as_path(),
            self.options.geojson_path.as_path(),
            self.options.mapping_path.as_path(),
        ];
        let publication = DicomBundlePublication::new(&output_dir, &protected_inputs)
            .map_err(publication_error)?;
        let mut instances = Vec::with_capacity(self.options.targets.len());
        for target in self.options.targets.iter().copied() {
            let staged = publication.staging_path().join(target.file_name());
            documents
                .write_and_verify(target.pathology_target(), &staged, &source)
                .map_err(document_write_error)?;
            let (sop_instance_uid, series_instance_uid) = document_identity(&documents, target)?;
            instances.push(AnnotationInstanceReport {
                target,
                path: output_dir.join(target.file_name()),
                sop_class_uid: target.sop_class_uid(),
                sop_instance_uid: sop_instance_uid.to_string(),
                series_instance_uid: series_instance_uid.to_string(),
            });
        }
        let report = AnnotationExportReport {
            output_dir: output_dir.clone(),
            source_wsi: source_instance.path.clone(),
            feature_count: annotations.feature_count(),
            semantic_sha256: annotations.semantic_sha256(),
            geojson_sha256: sha256(&self.geojson),
            mapping_sha256: sha256(&self.mapping),
            coordinate_space: self.options.coordinate_space,
            diagnostics: annotations
                .diagnostics()
                .iter()
                .map(diagnostic_report)
                .collect(),
            instances,
        };
        let manifest = publication.staging_path().join("manifest.json");
        let bytes = serde_json::to_vec_pretty(&report).map_err(|source| Error::JsonSerialize {
            message: source.to_string(),
        })?;
        fs::write(&manifest, bytes).map_err(|source| Error::Io {
            path: manifest.clone(),
            source,
        })?;
        publication
            .sync_staged_file(&manifest)
            .map_err(publication_error)?;
        publication.publish().map_err(publication_error)?;
        Ok(report)
    }
}

/// Convert profiled QuPath GeoJSON into verified sidecars for an existing WSI export report.
///
/// This lower-level form is useful when the WSI conversion was run separately. Most callers
/// should use [`crate::Export::run_with_qupath_annotations`] to snapshot annotation inputs before
/// starting the WSI export.
pub fn export_qupath_annotations(
    wsi: &ExportReport,
    options: &QuPathAnnotationOptions,
) -> Result<AnnotationExportReport, Error> {
    options.prepare()?.export_for(wsi)
}

fn validate_targets(targets: &[AnnotationTarget]) -> Result<(), Error> {
    if targets.is_empty() {
        return Err(Error::InvalidOptions {
            reason: "QuPath annotation conversion requires at least one annotation target".into(),
        });
    }
    if targets
        .iter()
        .enumerate()
        .any(|(index, target)| targets[..index].contains(target))
    {
        return Err(Error::InvalidOptions {
            reason: "QuPath annotation targets must be unique".into(),
        });
    }
    Ok(())
}

fn select_annotation_source(
    report: &ExportReport,
    coordinate_space: AnnotationCoordinateSpace,
) -> Result<&InstanceReport, Error> {
    let level_zero = report
        .instances
        .iter()
        .filter(|instance| instance.level == 0)
        .collect::<Vec<_>>();
    if level_zero.len() == 1 {
        return Ok(level_zero[0]);
    }
    if level_zero.is_empty()
        && coordinate_space != AnnotationCoordinateSpace::Level0Pixels
        && report.instances.len() == 1
    {
        return Ok(&report.instances[0]);
    }
    Err(Error::InvalidOptions {
        reason: format!(
            "QuPath annotation conversion requires one unambiguous level-zero WSI instance; export produced {}",
            level_zero.len()
        ),
    })
}

fn read_bounded_file(path: &Path, maximum_bytes: u64, description: &str) -> Result<Vec<u8>, Error> {
    let metadata = fs::metadata(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() || metadata.len() > maximum_bytes {
        return Err(Error::InvalidOptions {
            reason: format!(
                "{description} {} must be a file no larger than {maximum_bytes} bytes",
                path.display()
            ),
        });
    }
    let capacity = usize::try_from(metadata.len()).map_err(|_| Error::InvalidOptions {
        reason: format!("{description} length does not fit this platform"),
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    fs::File::open(path)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?
        .take(maximum_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(Error::InvalidOptions {
            reason: format!("{description} changed while being read or exceeds its size limit"),
        });
    }
    Ok(bytes)
}

fn document_identity(
    documents: &PathologyDicomDocuments,
    target: AnnotationTarget,
) -> Result<(&str, &str), Error> {
    match target {
        AnnotationTarget::Ann => documents
            .ann()
            .map(|document| (document.sop_instance_uid(), document.series_instance_uid())),
        AnnotationTarget::Seg => documents
            .seg()
            .map(|document| (document.sop_instance_uid(), document.series_instance_uid())),
        AnnotationTarget::Sr => documents
            .sr()
            .map(|document| (document.sop_instance_uid(), document.series_instance_uid())),
    }
    .ok_or_else(|| Error::Annotation {
        reason: format!("{} document was not constructed", target.label()),
    })
}

fn diagnostic_report(
    diagnostic: &wsi_dicom_annotations::InteroperabilityDiagnostic,
) -> AnnotationDiagnosticReport {
    AnnotationDiagnosticReport {
        code: diagnostic.code().to_string(),
        severity: match diagnostic.severity() {
            DiagnosticSeverity::Info => "info",
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Error => "error",
        },
        path: diagnostic.path().to_string(),
        disposition: match diagnostic.disposition() {
            DiagnosticDisposition::Normalized => "normalized",
            DiagnosticDisposition::WouldDrop => "would_drop",
            DiagnosticDisposition::Unsupported => "unsupported",
        },
        message: diagnostic.message().to_string(),
    }
}

fn document_write_error(error: PathologyDocumentWriteError) -> Error {
    Error::Annotation {
        reason: error.to_string(),
    }
}

fn annotation_error(error: wsi_dicom_annotations::Error) -> Error {
    Error::Annotation {
        reason: error.to_string(),
    }
}

fn publication_error(error: wsi_dicom_annotations::DicomPublicationError) -> Error {
    Error::Annotation {
        reason: format!("{}: {error}", error.code()),
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
#[path = "annotation_export/tests.rs"]
mod tests;
