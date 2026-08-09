use super::{DicomMetadata, SpecimenIdentifierIssuer, UniversalEntityIdType};
use crate::Error;

pub(super) fn is_supported_json(value: &serde_json::Value) -> bool {
    matches!(
        value
            .get("resourceType")
            .and_then(serde_json::Value::as_str),
        Some("Bundle" | "Patient" | "Specimen" | "ServiceRequest" | "DiagnosticReport")
    )
}

pub(super) fn map_bundle(value: &serde_json::Value) -> Result<DicomMetadata, Error> {
    let mut metadata = DicomMetadata::default();
    let resources = fhir_resources(value)?;
    let report = anchored_diagnostic_report(&resources)?;
    map_fhir_diagnostic_report(report, &mut metadata);

    let subject = required_reference(report, "subject", "FHIR DiagnosticReport")?;
    let patient = resolve_unique_fhir_reference(&resources, subject, "Patient")?;
    map_fhir_patient(patient, &mut metadata);

    let specimen_ref = required_reference_array_item(report, "specimen", "FHIR DiagnosticReport")?;
    let specimen = resolve_unique_fhir_reference(&resources, specimen_ref, "Specimen")?;
    map_fhir_specimen(specimen, &mut metadata);

    let based_on_ref = required_reference_array_item(report, "basedOn", "FHIR DiagnosticReport")?;
    let service_request =
        resolve_unique_fhir_reference(&resources, based_on_ref, "ServiceRequest")?;
    map_fhir_service_request(service_request, &mut metadata);

    Ok(metadata)
}

fn fhir_resources(value: &serde_json::Value) -> Result<Vec<&serde_json::Value>, Error> {
    match value
        .get("resourceType")
        .and_then(serde_json::Value::as_str)
    {
        Some("Bundle") => Ok(value
            .get("entry")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::Metadata {
                reason: "FHIR Bundle is missing entry array".into(),
            })?
            .iter()
            .filter_map(|entry| entry.get("resource"))
            .collect()),
        Some(_) => Ok(vec![value]),
        None => Err(Error::Metadata {
            reason: "FHIR JSON is missing resourceType".into(),
        }),
    }
}

fn anchored_diagnostic_report<'a>(
    resources: &'a [&'a serde_json::Value],
) -> Result<&'a serde_json::Value, Error> {
    let reports = resources
        .iter()
        .copied()
        .filter(|resource| {
            resource
                .get("resourceType")
                .and_then(serde_json::Value::as_str)
                == Some("DiagnosticReport")
        })
        .collect::<Vec<_>>();
    match reports.as_slice() {
        [report] => Ok(*report),
        [] => Err(Error::Metadata {
            reason: "FHIR metadata requires exactly one DiagnosticReport anchor".into(),
        }),
        _ => Err(Error::Metadata {
            reason: "FHIR metadata contains multiple DiagnosticReport resources".into(),
        }),
    }
}

fn required_reference<'a>(
    resource: &'a serde_json::Value,
    field: &str,
    owner: &str,
) -> Result<&'a str, Error> {
    resource
        .get(field)
        .and_then(|value| value.get("reference"))
        .and_then(serde_json::Value::as_str)
        .filter(|reference| !reference.is_empty())
        .ok_or_else(|| Error::Metadata {
            reason: format!("{owner} is missing {field}.reference"),
        })
}

fn required_reference_array_item<'a>(
    resource: &'a serde_json::Value,
    field: &str,
    owner: &str,
) -> Result<&'a str, Error> {
    let values = resource
        .get(field)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::Metadata {
            reason: format!("{owner} is missing {field} reference array"),
        })?;
    if values.len() != 1 {
        return Err(Error::Metadata {
            reason: format!("{owner} requires exactly one {field} reference"),
        });
    }
    values[0]
        .get("reference")
        .and_then(serde_json::Value::as_str)
        .filter(|reference| !reference.is_empty())
        .ok_or_else(|| Error::Metadata {
            reason: format!("{owner} has {field} entry without reference"),
        })
}

fn resolve_unique_fhir_reference<'a>(
    resources: &'a [&'a serde_json::Value],
    reference: &str,
    expected_type: &str,
) -> Result<&'a serde_json::Value, Error> {
    let Some((reference_type, reference_id)) = reference.split_once('/') else {
        return Err(Error::Metadata {
            reason: format!("FHIR reference {reference:?} must use ResourceType/id form"),
        });
    };
    if reference_type != expected_type {
        return Err(Error::Metadata {
            reason: format!(
                "FHIR reference {reference:?} points to {reference_type}, expected {expected_type}"
            ),
        });
    }
    let matches = resources
        .iter()
        .copied()
        .filter(|resource| {
            resource
                .get("resourceType")
                .and_then(serde_json::Value::as_str)
                == Some(expected_type)
                && resource.get("id").and_then(serde_json::Value::as_str) == Some(reference_id)
        })
        .collect::<Vec<_>>();
    let same_type_count = resources
        .iter()
        .filter(|resource| {
            resource
                .get("resourceType")
                .and_then(serde_json::Value::as_str)
                == Some(expected_type)
        })
        .count();
    if same_type_count > matches.len() {
        return Err(Error::Metadata {
            reason: format!(
                "FHIR metadata contains unreferenced {expected_type} resources beside {reference:?}"
            ),
        });
    }
    match matches.as_slice() {
        [resource] => Ok(*resource),
        [] => Err(Error::Metadata {
            reason: format!("FHIR reference {reference:?} did not match any bundled resource"),
        }),
        _ => Err(Error::Metadata {
            reason: format!("FHIR reference {reference:?} matched multiple resources"),
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
    let identifier = if let Some(value) = json_string(resource, "/accessionIdentifier/value") {
        Some((value, json_string(resource, "/accessionIdentifier/system")))
    } else if let Some(identifier) = first_identifier_with_system(resource) {
        Some(identifier)
    } else {
        json_string(resource, "/id").map(|value| (value, None))
    };
    metadata.specimen_identifier = identifier.as_ref().map(|(value, _)| value.clone());
    metadata.specimen_identifier_issuer =
        identifier
            .and_then(|(_, system)| system)
            .map(|universal_entity_id| SpecimenIdentifierIssuer {
                local_namespace_entity_id: None,
                universal_entity_id: Some(universal_entity_id),
                universal_entity_id_type: Some(UniversalEntityIdType::Uri),
            });
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
    first_identifier_with_system(resource).map(|(value, _)| value)
}

fn first_identifier_with_system(resource: &serde_json::Value) -> Option<(String, Option<String>)> {
    resource
        .get("identifier")
        .and_then(serde_json::Value::as_array)
        .and_then(|ids| ids.first())
        .and_then(|id| json_string(id, "/value").map(|value| (value, json_string(id, "/system"))))
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

#[cfg(test)]
mod tests {
    use super::DicomMetadata;

    #[test]
    fn bundle_rejects_multiple_reports_and_unreferenced_same_type_resources() {
        let base = serde_json::json!({
            "resourceType": "Bundle",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "pat-1", "identifier": [{"value": "MRN123"}], "name": [{"family": "Doe"}]}},
                {"resource": {"resourceType": "Specimen", "id": "spec-1", "identifier": [{"value": "S-42"}]}},
                {"resource": {"resourceType": "ServiceRequest", "id": "sr-1", "identifier": [{"value": "ORDER-7"}]}},
                {"resource": {"resourceType": "DiagnosticReport", "id": "dr-1", "subject": {"reference": "Patient/pat-1"}, "specimen": [{"reference": "Specimen/spec-1"}], "basedOn": [{"reference": "ServiceRequest/sr-1"}]}}
            ]
        });

        let mut two_reports = base.clone();
        two_reports["entry"].as_array_mut().unwrap().push(
            serde_json::json!({"resource": {"resourceType": "DiagnosticReport", "id": "dr-2"}}),
        );
        let err = DicomMetadata::from_fhir_r4_bundle(&two_reports).unwrap_err();
        assert!(err.to_string().contains("multiple DiagnosticReport"));

        let mut two_patients = base;
        two_patients["entry"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"resource": {"resourceType": "Patient", "id": "pat-2"}}));
        let err = DicomMetadata::from_fhir_r4_bundle(&two_patients).unwrap_err();
        assert!(err.to_string().contains("unreferenced Patient"));
    }
}
