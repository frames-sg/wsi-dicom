use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::fields::{optional_string, required_string, required_type_two_string, sequence_items};
use crate::uid::is_valid_dicom_uid;

pub(super) fn validate_specimen_container_rule(object: &DefaultDicomObject) -> Result<(), String> {
    required_string(object, tags::CONTAINER_IDENTIFIER, "Container Identifier")?;
    validate_issuer_sequence(
        object,
        tags::ISSUER_OF_THE_CONTAINER_IDENTIFIER_SEQUENCE,
        "Issuer of the Container Identifier Sequence",
    )?;
    let container_types = sequence_items(
        object,
        tags::CONTAINER_TYPE_CODE_SEQUENCE,
        "Container Type Code Sequence",
    )?;
    if container_types.len() != 1 {
        return Err(format!(
            "Container Type Code Sequence has {} items, expected one microscope-slide item",
            container_types.len()
        ));
    }
    let code_value = required_string(&container_types[0], tags::CODE_VALUE, "Container Type Code")?;
    if code_value != "433466003" {
        return Err(format!(
            "Container Type Code is {code_value}, expected microscope slide 433466003"
        ));
    }
    let specimens = sequence_items(
        object,
        tags::SPECIMEN_DESCRIPTION_SEQUENCE,
        "Specimen Description Sequence",
    )?;
    if specimens.is_empty() {
        return Err("Specimen Description Sequence is empty".into());
    }
    for (index, specimen) in specimens.iter().enumerate() {
        let steps = sequence_items(
            specimen,
            tags::SPECIMEN_PREPARATION_SEQUENCE,
            "Specimen Preparation Sequence",
        )?;
        for (step_index, step) in steps.iter().enumerate() {
            let content = sequence_items(
                step,
                tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
                "Specimen Preparation Step Content Item Sequence",
            )?;
            if content.is_empty() {
                return Err(format!(
                    "Specimen Description item {index} preparation step {step_index} has no content items"
                ));
            }
        }
    }
    Ok(())
}

fn validate_issuer_sequence(
    object: &InMemDicomObject,
    tag: dicom_core::Tag,
    name: &str,
) -> Result<(), String> {
    read_issuer_sequence(object, tag, name).map(|_| ())
}

fn read_issuer_sequence(
    object: &InMemDicomObject,
    tag: dicom_core::Tag,
    name: &str,
) -> Result<Option<IssuerIdentity>, String> {
    let items = sequence_items(object, tag, name)?;
    if items.len() > 1 {
        return Err(format!(
            "{name} has {} items, expected at most one",
            items.len()
        ));
    }
    let Some(item) = items.first() else {
        return Ok(None);
    };
    let local = optional_string(item, tags::LOCAL_NAMESPACE_ENTITY_ID)?;
    let universal = optional_string(item, tags::UNIVERSAL_ENTITY_ID)?;
    let universal_type = optional_string(item, tags::UNIVERSAL_ENTITY_ID_TYPE)?;
    if local.is_none() && universal.is_none() {
        return Err(format!("{name} contains an empty issuer item"));
    }
    if universal.is_some() != universal_type.is_some() {
        return Err(format!(
            "{name} must pair Universal Entity ID with Universal Entity ID Type"
        ));
    }
    Ok(Some(IssuerIdentity {
        local,
        universal,
        universal_type,
    }))
}

pub(super) fn validate_file_meta_identity(object: &DefaultDicomObject) -> Result<(), String> {
    let sop_class = required_string(object, tags::SOP_CLASS_UID, "SOP Class UID")?;
    let sop_instance = required_string(object, tags::SOP_INSTANCE_UID, "SOP Instance UID")?;
    let meta = object.meta();
    if meta.media_storage_sop_class_uid.trim_end_matches('\0') != sop_class {
        return Err("Media Storage SOP Class UID differs from data-set SOP Class UID".into());
    }
    if meta.media_storage_sop_instance_uid.trim_end_matches('\0') != sop_instance {
        return Err("Media Storage SOP Instance UID differs from data-set SOP Instance UID".into());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SpecimenIdentity {
    pub(super) uid: String,
    identifier: String,
    issuer: Option<IssuerIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IssuerIdentity {
    local: Option<String>,
    universal: Option<String>,
    universal_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClinicalIdentity {
    pub(super) patient_name: String,
    pub(super) patient_id: String,
    pub(super) study_uid: String,
    pub(super) series_uid: String,
    pub(super) frame_of_reference_uid: String,
    pub(super) container_identifier: String,
    pub(super) container_issuer: Option<IssuerIdentity>,
}

pub(super) fn clinical_identity(object: &DefaultDicomObject) -> Result<ClinicalIdentity, String> {
    for (tag, name) in [
        (tags::PATIENT_NAME, "Patient Name"),
        (tags::PATIENT_ID, "Patient ID"),
        (tags::PATIENT_BIRTH_DATE, "Patient Birth Date"),
        (tags::PATIENT_SEX, "Patient Sex"),
        (tags::STUDY_DATE, "Study Date"),
        (tags::STUDY_TIME, "Study Time"),
        (tags::REFERRING_PHYSICIAN_NAME, "Referring Physician Name"),
        (tags::STUDY_ID, "Study ID"),
        (tags::ACCESSION_NUMBER, "Accession Number"),
        (tags::SERIES_NUMBER, "Series Number"),
    ] {
        required_type_two_string(object, tag, name)?;
    }
    let study_uid = required_string(object, tags::STUDY_INSTANCE_UID, "Study Instance UID")?;
    let series_uid = required_string(object, tags::SERIES_INSTANCE_UID, "Series Instance UID")?;
    let frame_of_reference_uid = required_string(
        object,
        tags::FRAME_OF_REFERENCE_UID,
        "Frame of Reference UID",
    )?;
    for (name, value) in [
        ("Study Instance UID", &study_uid),
        ("Series Instance UID", &series_uid),
        ("Frame of Reference UID", &frame_of_reference_uid),
    ] {
        if !is_valid_dicom_uid(value) {
            return Err(format!("{name} is not a valid DICOM UID: {value:?}"));
        }
    }
    Ok(ClinicalIdentity {
        patient_name: required_type_two_string(object, tags::PATIENT_NAME, "Patient Name")?,
        patient_id: required_type_two_string(object, tags::PATIENT_ID, "Patient ID")?,
        study_uid,
        series_uid,
        frame_of_reference_uid,
        container_identifier: required_string(
            object,
            tags::CONTAINER_IDENTIFIER,
            "Container Identifier",
        )?,
        container_issuer: read_issuer_sequence(
            object,
            tags::ISSUER_OF_THE_CONTAINER_IDENTIFIER_SEQUENCE,
            "Issuer of the Container Identifier Sequence",
        )?,
    })
}

pub(super) fn specimen_identities(
    object: &DefaultDicomObject,
) -> Result<Vec<SpecimenIdentity>, String> {
    let items = sequence_items(
        object,
        tags::SPECIMEN_DESCRIPTION_SEQUENCE,
        "Specimen Description Sequence",
    )?;
    if items.is_empty() {
        return Err("Specimen Description Sequence is empty".into());
    }
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let identifier =
                required_string(item, tags::SPECIMEN_IDENTIFIER, "Specimen Identifier")?;
            let uid = required_string(item, tags::SPECIMEN_UID, "Specimen UID")?;
            if !is_valid_dicom_uid(&uid) {
                return Err(format!(
                    "Specimen Description item {index} has invalid Specimen UID {uid:?}"
                ));
            }
            let issuer = specimen_issuer(item, index)?;
            Ok(SpecimenIdentity {
                uid,
                identifier,
                issuer,
            })
        })
        .collect()
}

fn specimen_issuer(
    specimen: &InMemDicomObject,
    specimen_index: usize,
) -> Result<Option<IssuerIdentity>, String> {
    let identity = read_issuer_sequence(
        specimen,
        tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE,
        &format!("Specimen Description item {specimen_index} issuer sequence"),
    )?;
    let Some(issuer) = identity.as_ref() else {
        return Ok(None);
    };
    // The specimen macro constrains the identifier type beyond the common issuer structure.
    if issuer.universal_type.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "DNS" | "EUI64" | "ISO" | "URI" | "UUID" | "X400" | "X500"
        )
    }) {
        return Err(format!(
            "Specimen Description item {specimen_index} has an invalid Universal Entity ID Type"
        ));
    }
    Ok(identity)
}
