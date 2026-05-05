use serde::{Deserialize, Serialize};

use crate::WsiDicomError;

/// Metadata accepted by the DICOM writer after strict JSON or FHIR mapping.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DicomMetadata {
    pub patient_name: Option<String>,
    pub patient_id: Option<String>,
    pub patient_birth_date: Option<String>,
    pub patient_sex: Option<String>,
    pub accession_number: Option<String>,
    pub study_instance_uid: Option<String>,
    pub study_id: Option<String>,
    pub study_date: Option<String>,
    pub study_time: Option<String>,
    pub study_description: Option<String>,
    pub referring_physician_name: Option<String>,
    pub laterality: Option<String>,
    pub manufacturer: Option<String>,
    pub manufacturer_model_name: Option<String>,
    pub device_serial_number: Option<String>,
    pub software_versions: Option<String>,
    pub content_date: Option<String>,
    pub content_time: Option<String>,
    pub acquisition_date_time: Option<String>,
    pub container_identifier: Option<String>,
    pub specimen_identifier: Option<String>,
    pub specimen_description: Option<String>,
    pub imaged_volume_depth_mm: Option<f64>,
    pub focus_method: Option<String>,
}

impl DicomMetadata {
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

    pub fn from_fhir_r4_bundle(value: &serde_json::Value) -> Result<Self, WsiDicomError> {
        let mut metadata = Self::default();
        let resources = fhir_resources(value)?;
        for resource in resources {
            match resource
                .get("resourceType")
                .and_then(serde_json::Value::as_str)
            {
                Some("Patient") => map_fhir_patient(resource, &mut metadata),
                Some("Specimen") => map_fhir_specimen(resource, &mut metadata),
                Some("ServiceRequest") => map_fhir_service_request(resource, &mut metadata),
                Some("DiagnosticReport") => map_fhir_diagnostic_report(resource, &mut metadata),
                _ => {}
            }
        }
        metadata.validate_strict()?;
        Ok(metadata)
    }

    pub fn validate_strict(&self) -> Result<(), WsiDicomError> {
        if self.patient_id.as_deref().unwrap_or_default().is_empty() {
            return Err(WsiDicomError::Metadata {
                reason: "strict metadata requires patient_id".into(),
            });
        }
        if self.patient_name.as_deref().unwrap_or_default().is_empty() {
            return Err(WsiDicomError::Metadata {
                reason: "strict metadata requires patient_name".into(),
            });
        }
        Ok(())
    }
}

/// Source of metadata for the DICOM export request.
#[derive(Debug, Clone, PartialEq)]
pub enum MetadataSource {
    Strict(Box<DicomMetadata>),
    ResearchPlaceholder,
    FhirR4Bundle(serde_json::Value),
}

impl MetadataSource {
    pub(crate) fn resolve(&self) -> Result<DicomMetadata, WsiDicomError> {
        match self {
            Self::Strict(metadata) => {
                metadata.validate_strict()?;
                Ok(metadata.as_ref().clone())
            }
            Self::ResearchPlaceholder => Ok(DicomMetadata::research_placeholder()),
            Self::FhirR4Bundle(bundle) => DicomMetadata::from_fhir_r4_bundle(bundle),
        }
    }
}

fn fhir_resources(value: &serde_json::Value) -> Result<Vec<&serde_json::Value>, WsiDicomError> {
    match value
        .get("resourceType")
        .and_then(serde_json::Value::as_str)
    {
        Some("Bundle") => Ok(value
            .get("entry")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| WsiDicomError::Metadata {
                reason: "FHIR Bundle is missing entry array".into(),
            })?
            .iter()
            .filter_map(|entry| entry.get("resource"))
            .collect()),
        Some(_) => Ok(vec![value]),
        None => Err(WsiDicomError::Metadata {
            reason: "FHIR JSON is missing resourceType".into(),
        }),
    }
}

fn map_fhir_patient(resource: &serde_json::Value, metadata: &mut DicomMetadata) {
    metadata.patient_id = first_identifier(resource).or_else(|| json_string(resource, "/id"));
    metadata.patient_name = resource
        .get("name")
        .and_then(serde_json::Value::as_array)
        .and_then(|names| names.first())
        .and_then(fhir_human_name_to_pn);
    metadata.patient_birth_date =
        json_string(resource, "/birthDate").map(|date| date.replace('-', ""));
    metadata.patient_sex =
        json_string(resource, "/gender").and_then(|gender| match gender.as_str() {
            "male" => Some("M".to_string()),
            "female" => Some("F".to_string()),
            "other" => Some("O".to_string()),
            "unknown" => Some("U".to_string()),
            _ => None,
        });
}

fn map_fhir_specimen(resource: &serde_json::Value, metadata: &mut DicomMetadata) {
    metadata.specimen_identifier = json_string(resource, "/accessionIdentifier/value")
        .or_else(|| first_identifier(resource))
        .or_else(|| json_string(resource, "/id"));
    if metadata.container_identifier.is_none() {
        metadata.container_identifier = metadata.specimen_identifier.clone();
    }
    metadata.specimen_description = json_string(resource, "/type/text");
}

fn map_fhir_service_request(resource: &serde_json::Value, metadata: &mut DicomMetadata) {
    metadata.accession_number = first_identifier(resource)
        .or_else(|| json_string(resource, "/requisition/value"))
        .or_else(|| json_string(resource, "/id"));
    if metadata.study_description.is_none() {
        metadata.study_description = json_string(resource, "/code/text");
    }
}

fn map_fhir_diagnostic_report(resource: &serde_json::Value, metadata: &mut DicomMetadata) {
    if metadata.study_id.is_none() {
        metadata.study_id = first_identifier(resource).or_else(|| json_string(resource, "/id"));
    }
    metadata.study_description = json_string(resource, "/code/text");
}

fn first_identifier(resource: &serde_json::Value) -> Option<String> {
    resource
        .get("identifier")
        .and_then(serde_json::Value::as_array)
        .and_then(|ids| ids.first())
        .and_then(|id| json_string(id, "/value"))
}

fn fhir_human_name_to_pn(name: &serde_json::Value) -> Option<String> {
    let family = name.get("family").and_then(serde_json::Value::as_str)?;
    let given = name
        .get("given")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    if given.is_empty() {
        Some(family.to_string())
    } else {
        Some(format!("{family}^{given}"))
    }
}

fn json_string(value: &serde_json::Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}
