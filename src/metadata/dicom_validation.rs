use dicom_core::VR;

use super::{DicomMetadata, SpecimenIdentifierIssuer};
use crate::uid::is_valid_dicom_uid;
use crate::Error;

const DEFAULT_IMAGED_VOLUME_DEPTH_MM: f64 = 0.001;

#[derive(Debug)]
pub(crate) struct ValidatedDicomMetadata<'a> {
    metadata: &'a DicomMetadata,
    imaged_volume_depth_um: f32,
    slice_thickness_mm_ds: String,
}

impl std::ops::Deref for ValidatedDicomMetadata<'_> {
    type Target = DicomMetadata;

    fn deref(&self) -> &Self::Target {
        self.metadata
    }
}

impl<'a> ValidatedDicomMetadata<'a> {
    pub(crate) fn as_metadata(&self) -> &'a DicomMetadata {
        self.metadata
    }

    pub(crate) fn imaged_volume_depth_um(&self) -> f32 {
        self.imaged_volume_depth_um
    }

    pub(crate) fn slice_thickness_mm_ds(&self) -> &str {
        &self.slice_thickness_mm_ds
    }

    pub(crate) fn requires_utf8(&self) -> bool {
        metadata_strings(self.metadata).any(|value| !value.is_ascii())
    }
}

pub(super) fn validate(metadata: &DicomMetadata) -> Result<ValidatedDicomMetadata<'_>, Error> {
    metadata.validate_strict()?;
    validate_optional_vr("patient_name", "PN", metadata.patient_name.as_deref(), 64)?;
    validate_optional_vr("patient_id", "LO", metadata.patient_id.as_deref(), 64)?;
    validate_optional_format(
        "patient_birth_date",
        VR::DA,
        metadata.patient_birth_date.as_deref(),
    )?;
    validate_optional_format("patient_sex", VR::CS, metadata.patient_sex.as_deref())?;
    validate_optional_vr(
        "accession_number",
        "SH",
        metadata.accession_number.as_deref(),
        16,
    )?;
    validate_optional_ui("study_instance_uid", metadata.study_instance_uid.as_deref())?;
    validate_optional_vr("study_id", "SH", metadata.study_id.as_deref(), 16)?;
    validate_optional_format("study_date", VR::DA, metadata.study_date.as_deref())?;
    validate_optional_format("study_time", VR::TM, metadata.study_time.as_deref())?;
    validate_optional_vr(
        "study_description",
        "LO",
        metadata.study_description.as_deref(),
        64,
    )?;
    validate_optional_vr(
        "referring_physician_name",
        "PN",
        metadata.referring_physician_name.as_deref(),
        64,
    )?;
    validate_optional_format("laterality", VR::CS, metadata.laterality.as_deref())?;
    validate_optional_vr("manufacturer", "LO", metadata.manufacturer.as_deref(), 64)?;
    validate_optional_vr(
        "manufacturer_model_name",
        "LO",
        metadata.manufacturer_model_name.as_deref(),
        64,
    )?;
    validate_optional_vr(
        "device_serial_number",
        "LO",
        metadata.device_serial_number.as_deref(),
        64,
    )?;
    validate_optional_vr(
        "software_versions",
        "LO",
        metadata.software_versions.as_deref(),
        64,
    )?;
    validate_optional_format("content_date", VR::DA, metadata.content_date.as_deref())?;
    validate_optional_format("content_time", VR::TM, metadata.content_time.as_deref())?;
    validate_optional_format(
        "acquisition_date_time",
        VR::DT,
        metadata.acquisition_date_time.as_deref(),
    )?;
    validate_optional_vr(
        "container_identifier",
        "LO",
        metadata.container_identifier.as_deref(),
        64,
    )?;
    validate_optional_vr(
        "specimen_identifier",
        "LO",
        metadata.specimen_identifier.as_deref(),
        64,
    )?;
    if metadata.specimen_uid.as_deref() == Some("") {
        return Err(Error::Metadata {
            reason: "specimen_uid must not be empty when supplied".into(),
        });
    }
    validate_optional_ui("specimen_uid", metadata.specimen_uid.as_deref())?;
    validate_specimen_identifier_issuer(metadata.specimen_identifier_issuer.as_ref())?;
    validate_optional_vr(
        "specimen_description",
        "LO",
        metadata.specimen_description.as_deref(),
        64,
    )?;
    validate_optional_format("focus_method", VR::CS, metadata.focus_method.as_deref())?;
    let (imaged_volume_depth_um, slice_thickness_mm_ds) =
        validate_imaged_volume_depth(metadata.imaged_volume_depth_mm)?;
    Ok(ValidatedDicomMetadata {
        metadata,
        imaged_volume_depth_um,
        slice_thickness_mm_ds,
    })
}

fn validate_optional_vr(
    field: &str,
    vr: &str,
    value: Option<&str>,
    max_chars: usize,
) -> Result<(), Error> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.contains('\\') || value.chars().any(is_disallowed_text_control) {
        return Err(Error::Metadata {
            reason: format!(
                "{field} contains a delimiter or control character not allowed in scalar DICOM {vr}"
            ),
        });
    }
    if vr == "PN" {
        return validate_person_name(field, value);
    }
    if value.chars().count() > max_chars || value.len() > usize::from(u16::MAX) {
        return Err(Error::Metadata {
            reason: format!("{field} exceeds DICOM {vr} limit of {max_chars} characters"),
        });
    }
    Ok(())
}

fn validate_person_name(field: &str, value: &str) -> Result<(), Error> {
    validate_value_format(field, VR::PN, value)?;
    if value.len() > usize::from(u16::MAX) {
        return Err(Error::Metadata {
            reason: format!("{field} exceeds the encoded DICOM PN value length"),
        });
    }
    Ok(())
}

/// PS3.5 6.2 Value format, as the validator checks it.
fn validate_value_format(field: &str, vr: VR, value: &str) -> Result<(), Error> {
    crate::validation::check_value_format(vr, value).map_err(|reason| Error::Metadata {
        reason: format!("{field} is not a valid DICOM {vr:?} Value: {reason}"),
    })
}

fn validate_optional_format(field: &str, vr: VR, value: Option<&str>) -> Result<(), Error> {
    match value.filter(|value| !value.is_empty()) {
        Some(value) => validate_value_format(field, vr, value),
        None => Ok(()),
    }
}

fn validate_imaged_volume_depth(value_mm: Option<f64>) -> Result<(f32, String), Error> {
    let value_mm = value_mm.unwrap_or(DEFAULT_IMAGED_VOLUME_DEPTH_MM);
    if !value_mm.is_finite() || value_mm <= 0.0 {
        return Err(Error::Metadata {
            reason: "imaged_volume_depth_mm must be finite and greater than zero".into(),
        });
    }

    let value_um = value_mm * 1_000.0;
    let value_um_fl = value_um as f32;
    if !value_um.is_finite() || !value_um_fl.is_finite() || value_um_fl <= 0.0 {
        return Err(Error::Metadata {
            reason: "imaged_volume_depth_mm cannot be represented as a positive DICOM FL value in micrometers"
                .into(),
        });
    }

    let slice_thickness_mm_ds = wsi_dicom_annotations::format_dicom_decimal(value_mm);
    Ok((value_um_fl, slice_thickness_mm_ds))
}

fn metadata_strings(metadata: &DicomMetadata) -> impl Iterator<Item = &str> {
    [
        metadata.patient_name.as_deref(),
        metadata.patient_id.as_deref(),
        metadata.patient_birth_date.as_deref(),
        metadata.patient_sex.as_deref(),
        metadata.accession_number.as_deref(),
        metadata.study_instance_uid.as_deref(),
        metadata.study_id.as_deref(),
        metadata.study_date.as_deref(),
        metadata.study_time.as_deref(),
        metadata.study_description.as_deref(),
        metadata.referring_physician_name.as_deref(),
        metadata.laterality.as_deref(),
        metadata.manufacturer.as_deref(),
        metadata.manufacturer_model_name.as_deref(),
        metadata.device_serial_number.as_deref(),
        metadata.software_versions.as_deref(),
        metadata.content_date.as_deref(),
        metadata.content_time.as_deref(),
        metadata.acquisition_date_time.as_deref(),
        metadata.container_identifier.as_deref(),
        metadata.specimen_identifier.as_deref(),
        metadata.specimen_uid.as_deref(),
        metadata
            .specimen_identifier_issuer
            .as_ref()
            .and_then(|issuer| issuer.local_namespace_entity_id.as_deref()),
        metadata
            .specimen_identifier_issuer
            .as_ref()
            .and_then(|issuer| issuer.universal_entity_id.as_deref()),
        metadata.specimen_description.as_deref(),
        metadata.focus_method.as_deref(),
    ]
    .into_iter()
    .flatten()
}

fn validate_specimen_identifier_issuer(
    issuer: Option<&SpecimenIdentifierIssuer>,
) -> Result<(), Error> {
    let Some(issuer) = issuer else {
        return Ok(());
    };
    let local = non_empty_issuer_value(
        "specimen_identifier_issuer.local_namespace_entity_id",
        issuer.local_namespace_entity_id.as_deref(),
    )?;
    let universal = non_empty_issuer_value(
        "specimen_identifier_issuer.universal_entity_id",
        issuer.universal_entity_id.as_deref(),
    )?;
    if local.is_none() && universal.is_none() {
        return Err(Error::Metadata {
            reason: "specimen_identifier_issuer requires a local or universal entity ID".into(),
        });
    }
    if universal.is_some() != issuer.universal_entity_id_type.is_some() {
        return Err(Error::Metadata {
            reason: "specimen_identifier_issuer.universal_entity_id and universal_entity_id_type must be supplied together"
                .into(),
        });
    }
    Ok(())
}

fn non_empty_issuer_value<'a>(
    field: &str,
    value: Option<&'a str>,
) -> Result<Option<&'a str>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() || value.contains('\\') || value.chars().any(is_disallowed_text_control) {
        return Err(Error::Metadata {
            reason: format!("{field} must be a non-empty scalar DICOM UT value"),
        });
    }
    Ok(Some(value))
}

fn validate_optional_ui(field: &str, value: Option<&str>) -> Result<(), Error> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if !is_valid_dicom_uid(value) {
        return Err(Error::Metadata {
            reason: format!("{field} must be a valid DICOM UI"),
        });
    }
    Ok(())
}

fn is_disallowed_text_control(ch: char) -> bool {
    ch.is_control()
}

#[cfg(test)]
mod tests;
