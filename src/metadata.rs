use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Error;

mod dicom_validation;
mod fhir_r4;

pub(crate) use dicom_validation::ValidatedDicomMetadata;

/// Maximum accepted metadata JSON file size.
pub const METADATA_JSON_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Metadata accepted by the DICOM writer after strict JSON or FHIR mapping.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DicomMetadata {
    /// DICOM Patient Name.
    pub patient_name: Option<String>,
    /// DICOM Patient ID.
    pub patient_id: Option<String>,
    /// DICOM Patient Birth Date in DA format.
    pub patient_birth_date: Option<String>,
    /// DICOM Patient Sex.
    pub patient_sex: Option<String>,
    /// DICOM Accession Number.
    pub accession_number: Option<String>,
    /// Optional caller-supplied Study Instance UID.
    pub study_instance_uid: Option<String>,
    /// DICOM Study ID.
    pub study_id: Option<String>,
    /// DICOM Study Date in DA format.
    pub study_date: Option<String>,
    /// DICOM Study Time in TM format.
    pub study_time: Option<String>,
    /// DICOM Study Description.
    pub study_description: Option<String>,
    /// DICOM Referring Physician Name.
    pub referring_physician_name: Option<String>,
    /// DICOM Laterality.
    pub laterality: Option<String>,
    /// Equipment manufacturer.
    pub manufacturer: Option<String>,
    /// Equipment model name.
    pub manufacturer_model_name: Option<String>,
    /// Equipment serial number.
    pub device_serial_number: Option<String>,
    /// Software version string recorded in generated instances.
    pub software_versions: Option<String>,
    /// DICOM Content Date in DA format.
    pub content_date: Option<String>,
    /// DICOM Content Time in TM format.
    pub content_time: Option<String>,
    /// DICOM Acquisition DateTime in DT format.
    pub acquisition_date_time: Option<String>,
    /// Container identifier for the specimen container.
    pub container_identifier: Option<String>,
    /// Specimen identifier.
    pub specimen_identifier: Option<String>,
    /// Human-readable specimen description.
    pub specimen_description: Option<String>,
    /// Imaged volume depth in millimeters.
    pub imaged_volume_depth_mm: Option<f64>,
    /// DICOM focus method value.
    pub focus_method: Option<String>,
}

impl DicomMetadata {
    /// Return deterministic placeholder metadata for non-clinical research exports.
    pub fn research_placeholder() -> Self {
        Self {
            patient_name: Some("RESEARCH^PLACEHOLDER".into()),
            patient_id: Some("RESEARCH".into()),
            patient_birth_date: Some(String::new()),
            patient_sex: Some(String::new()),
            accession_number: Some("RESEARCH".into()),
            study_id: Some("1".into()),
            study_date: Some("19700101".into()),
            study_time: Some("000000".into()),
            study_description: Some("Research placeholder WSI export".into()),
            referring_physician_name: Some(String::new()),
            laterality: Some(String::new()),
            manufacturer: Some("wsi-dicom".into()),
            manufacturer_model_name: Some("wsi-dicom".into()),
            device_serial_number: Some("RESEARCH".into()),
            software_versions: Some(env!("CARGO_PKG_VERSION").into()),
            content_date: Some("19700101".into()),
            content_time: Some("000000".into()),
            acquisition_date_time: Some("19700101000000".into()),
            container_identifier: Some("RESEARCH-CONTAINER".into()),
            specimen_identifier: Some("RESEARCH-SPECIMEN".into()),
            specimen_description: Some("Research placeholder specimen".into()),
            imaged_volume_depth_mm: Some(0.001),
            focus_method: Some("AUTO".into()),
            study_instance_uid: None,
        }
    }

    /// Map supported Patient, Specimen, ServiceRequest, and DiagnosticReport fields from FHIR R4 JSON.
    pub fn from_fhir_r4_bundle(value: &serde_json::Value) -> Result<Self, Error> {
        let metadata = fhir_r4::map_bundle(value)?;
        metadata.validate_for_export()?;
        Ok(metadata)
    }

    /// Validate that required strict metadata fields are present.
    pub fn validate_strict(&self) -> Result<(), Error> {
        if self.patient_id.as_deref().unwrap_or_default().is_empty() {
            return Err(Error::Metadata {
                reason: "strict metadata requires patient_id".into(),
            });
        }
        if self.patient_name.as_deref().unwrap_or_default().is_empty() {
            return Err(Error::Metadata {
                reason: "strict metadata requires patient_name".into(),
            });
        }
        Ok(())
    }

    /// Validate all metadata constraints required before beginning an export.
    ///
    /// This extends [`Self::validate_strict`] with DICOM VR, person-name,
    /// delimiter, character-set, and imaged-volume-depth checks. Validation is
    /// side-effect free so callers can reject invalid clinical metadata before
    /// opening a slide or creating output state.
    pub fn validate_for_export(&self) -> Result<(), Error> {
        self.validated_for_writer().map(|_| ())
    }

    pub(crate) fn validated_for_writer(&self) -> Result<ValidatedDicomMetadata<'_>, Error> {
        dicom_validation::validate(self)
    }
}

/// Source of metadata for the DICOM export request.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MetadataSource {
    /// Caller-provided metadata that must satisfy strict validation.
    Strict(Box<DicomMetadata>),
    /// Deterministic non-clinical metadata suitable for tests and research placeholders.
    ResearchPlaceholder,
    /// FHIR R4 JSON mapped into DICOM metadata before strict validation.
    FhirR4Bundle(serde_json::Value),
}

impl MetadataSource {
    /// Read a bounded JSON file and map it into FHIR R4 or strict DICOM metadata.
    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let file = std::fs::File::open(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut limited = file.take(METADATA_JSON_MAX_BYTES.saturating_add(1));
        let mut bytes = Vec::new();
        limited
            .read_to_end(&mut bytes)
            .map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > METADATA_JSON_MAX_BYTES {
            return Err(Error::Metadata {
                reason: format!(
                    "metadata JSON {} exceeds {} byte limit",
                    path.display(),
                    METADATA_JSON_MAX_BYTES
                ),
            });
        }
        let value = serde_json::from_slice(&bytes).map_err(|source| Error::Json {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_json_value(value).map_err(|source| Error::Json {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Map metadata JSON into either FHIR R4 or strict DICOM metadata input.
    pub fn from_json_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if fhir_r4::is_supported_json(&value) {
            Ok(Self::FhirR4Bundle(value))
        } else {
            let metadata: DicomMetadata = serde_json::from_value(value)?;
            Ok(Self::Strict(Box::new(metadata)))
        }
    }

    pub(crate) fn resolve(&self) -> Result<DicomMetadata, Error> {
        let metadata = match self {
            Self::Strict(metadata) => metadata.as_ref().clone(),
            Self::ResearchPlaceholder => DicomMetadata::research_placeholder(),
            Self::FhirR4Bundle(bundle) => DicomMetadata::from_fhir_r4_bundle(bundle)?,
        };
        metadata.validate_for_export()?;
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::{DicomMetadata, MetadataSource, METADATA_JSON_MAX_BYTES};

    #[test]
    fn metadata_source_from_json_file_reads_valid_input_and_rejects_oversize_input() {
        let directory = tempfile::tempdir().unwrap();
        let valid = directory.path().join("metadata.json");
        std::fs::write(&valid, br#"{"patient_id":"P-1","patient_name":"DOE^JANE"}"#).unwrap();
        assert!(matches!(
            MetadataSource::from_json_file(&valid).unwrap(),
            MetadataSource::Strict(_)
        ));

        let oversized = directory.path().join("oversized.json");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(METADATA_JSON_MAX_BYTES + 1).unwrap();
        let error = MetadataSource::from_json_file(&oversized).unwrap_err();
        assert!(error.to_string().contains("exceeds"));
    }

    #[test]
    fn metadata_source_from_json_value_detects_supported_fhir_resources() {
        let value = serde_json::json!({
            "resourceType": "Patient",
            "id": "patient-1",
            "name": [{"family": "Doe", "given": ["Jane"]}]
        });

        let source = MetadataSource::from_json_value(value.clone()).unwrap();

        assert_eq!(source, MetadataSource::FhirR4Bundle(value));
    }

    #[test]
    fn metadata_source_from_json_value_parses_strict_dicom_metadata() {
        let value = serde_json::json!({
            "patient_id": "P-1",
            "patient_name": "DOE^JANE",
            "study_id": "S-1"
        });

        let source = MetadataSource::from_json_value(value).unwrap();

        let MetadataSource::Strict(metadata) = source else {
            panic!("expected strict DICOM metadata");
        };
        assert_eq!(
            metadata.as_ref(),
            &DicomMetadata {
                patient_id: Some("P-1".to_string()),
                patient_name: Some("DOE^JANE".to_string()),
                study_id: Some("S-1".to_string()),
                ..DicomMetadata::default()
            }
        );
    }

    #[test]
    fn json_and_fhir_metadata_paths_apply_export_depth_validation() {
        let mut strict = serde_json::to_value(DicomMetadata::research_placeholder()).unwrap();
        strict["imaged_volume_depth_mm"] = serde_json::json!(0.001);
        let source = MetadataSource::from_json_value(strict).unwrap();
        let resolved = source.resolve().unwrap();
        let validated = resolved.validated_for_writer().unwrap();
        assert_eq!(validated.imaged_volume_depth_um(), 1.0);
        assert_eq!(validated.slice_thickness_mm_ds(), "0.001");

        let mut invalid = serde_json::to_value(DicomMetadata::research_placeholder()).unwrap();
        invalid["imaged_volume_depth_mm"] = serde_json::json!(0.0);
        let source = MetadataSource::from_json_value(invalid).unwrap();
        assert!(source.resolve().unwrap_err().to_string().contains("depth"));

        let fhir = serde_json::json!({
            "resourceType": "Bundle",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "pat-1", "identifier": [{"value": "MRN123"}], "name": [{"family": "Doe", "given": ["Jane"]}]}},
                {"resource": {"resourceType": "Specimen", "id": "spec-1", "identifier": [{"value": "S-42"}]}},
                {"resource": {"resourceType": "ServiceRequest", "id": "sr-1", "identifier": [{"value": "ORDER-7"}]}},
                {"resource": {"resourceType": "DiagnosticReport", "id": "dr-1", "subject": {"reference": "Patient/pat-1"}, "specimen": [{"reference": "Specimen/spec-1"}], "basedOn": [{"reference": "ServiceRequest/sr-1"}]}}
            ]
        });
        let resolved = MetadataSource::FhirR4Bundle(fhir).resolve().unwrap();
        let validated = resolved.validated_for_writer().unwrap();
        assert_eq!(validated.imaged_volume_depth_um(), 1.0);
        assert_eq!(validated.slice_thickness_mm_ds(), "0.001");
    }

    #[test]
    fn validate_strict_preserves_required_field_contract() {
        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_date = Some("20261301".to_string());

        metadata.validate_strict().unwrap();
    }
}
