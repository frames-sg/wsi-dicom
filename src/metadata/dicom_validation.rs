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
    validate_optional_da("patient_birth_date", metadata.patient_birth_date.as_deref())?;
    validate_optional_cs("patient_sex", metadata.patient_sex.as_deref())?;
    validate_optional_vr(
        "accession_number",
        "SH",
        metadata.accession_number.as_deref(),
        16,
    )?;
    validate_optional_ui("study_instance_uid", metadata.study_instance_uid.as_deref())?;
    validate_optional_vr("study_id", "SH", metadata.study_id.as_deref(), 16)?;
    validate_optional_da("study_date", metadata.study_date.as_deref())?;
    validate_optional_tm("study_time", metadata.study_time.as_deref())?;
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
    validate_optional_cs("laterality", metadata.laterality.as_deref())?;
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
    validate_optional_da("content_date", metadata.content_date.as_deref())?;
    validate_optional_tm("content_time", metadata.content_time.as_deref())?;
    validate_optional_dt(
        "acquisition_date_time",
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
    validate_optional_cs("focus_method", metadata.focus_method.as_deref())?;
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
        return validate_person_name(field, value, max_chars);
    }
    if value.chars().count() > max_chars || value.len() > usize::from(u16::MAX) {
        return Err(Error::Metadata {
            reason: format!("{field} exceeds DICOM {vr} limit of {max_chars} characters"),
        });
    }
    Ok(())
}

fn validate_person_name(field: &str, value: &str, max_chars: usize) -> Result<(), Error> {
    if value.split('=').count() > 3 {
        return Err(Error::Metadata {
            reason: format!("{field} exceeds the three DICOM PN representation groups"),
        });
    }
    for group in value.split('=') {
        if group.split('^').count() > 5 {
            return Err(Error::Metadata {
                reason: format!("{field} exceeds the five DICOM PN components"),
            });
        }
        if group.chars().count() > max_chars {
            return Err(Error::Metadata {
                reason: format!(
                    "{field} exceeds DICOM PN limit of {max_chars} characters per representation group"
                ),
            });
        }
    }
    if value.len() > usize::from(u16::MAX) {
        return Err(Error::Metadata {
            reason: format!("{field} exceeds the encoded DICOM PN value length"),
        });
    }
    Ok(())
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

    let slice_thickness_mm_ds = format_depth_ds(value_mm).ok_or_else(|| Error::Metadata {
        reason: "imaged_volume_depth_mm cannot be represented as a DICOM DS value".into(),
    })?;
    if slice_thickness_mm_ds == "0" || slice_thickness_mm_ds == "-0" {
        return Err(Error::Metadata {
            reason: "imaged_volume_depth_mm underflows the DICOM DS representation".into(),
        });
    }
    Ok((value_um_fl, slice_thickness_mm_ds))
}

fn format_depth_ds(value: f64) -> Option<String> {
    for precision in (0..=12).rev() {
        let mut text = format!("{value:.precision$}");
        while text.contains('.') && text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
        if text.len() <= 16 {
            return Some(text);
        }
    }
    let text = format!("{value:.8e}");
    (text.len() <= 16).then_some(text)
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

fn validate_optional_da(field: &str, value: Option<&str>) -> Result<(), Error> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Metadata {
            reason: format!("{field} must use DICOM DA format YYYYMMDD"),
        });
    }
    let year = parse_decimal_component(&value[0..4]);
    let month = parse_decimal_component(&value[4..6]);
    let day = parse_decimal_component(&value[6..8]);
    validate_date_components(field, year, Some(month), Some(day), "DA")?;
    Ok(())
}

fn validate_optional_tm(field: &str, value: Option<&str>) -> Result<(), Error> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let Some((time, fraction)) = split_fraction(value) else {
        return Err(Error::Metadata {
            reason: format!("{field} must use DICOM TM format HH[MM[SS[.FFFFFF]]]"),
        });
    };
    if fraction.is_some_and(invalid_fraction)
        || fraction.is_some() && time.len() != 6
        || !(2..=6).contains(&time.len())
        || time.len() % 2 != 0
        || !time.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Error::Metadata {
            reason: format!("{field} must use DICOM TM format HH[MM[SS[.FFFFFF]]]"),
        });
    }
    validate_time_components(field, time, "TM")?;
    Ok(())
}

fn validate_optional_dt(field: &str, value: Option<&str>) -> Result<(), Error> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let Some((date_time_with_fraction, timezone)) = split_dt_timezone(value) else {
        return Err(Error::Metadata {
            reason: format!(
                "{field} must use DICOM DT format YYYY[MM[DD[HH[MM[SS[.FFFFFF]]]]]][+/-ZZZZ]"
            ),
        });
    };
    if let Some(timezone) = timezone {
        validate_dt_timezone(field, timezone)?;
    }
    let Some((date_time, fraction)) = split_fraction(date_time_with_fraction) else {
        return Err(Error::Metadata {
            reason: format!(
                "{field} must use DICOM DT format YYYY[MM[DD[HH[MM[SS[.FFFFFF]]]]]][+/-ZZZZ]"
            ),
        });
    };
    if !(4..=14).contains(&date_time.len())
        || date_time.len() % 2 != 0
        || !date_time.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(invalid_fraction)
        || fraction.is_some() && date_time.len() != 14
        || value.chars().any(is_disallowed_text_control)
    {
        return Err(Error::Metadata {
            reason: format!(
                "{field} must use DICOM DT format YYYY[MM[DD[HH[MM[SS[.FFFFFF]]]]]][+/-ZZZZ]"
            ),
        });
    }
    validate_dt_components(field, date_time)?;
    Ok(())
}

fn split_fraction(value: &str) -> Option<(&str, Option<&str>)> {
    let mut parts = value.split('.');
    let main = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some() {
        return None;
    }
    Some((main, fraction))
}

fn invalid_fraction(fraction: &str) -> bool {
    fraction.is_empty() || fraction.len() > 6 || !fraction.bytes().all(|byte| byte.is_ascii_digit())
}

fn split_dt_timezone(value: &str) -> Option<(&str, Option<&str>)> {
    let mut timezone_start = None;
    for (idx, byte) in value.bytes().enumerate() {
        if (byte == b'+' || byte == b'-') && timezone_start.replace(idx).is_some() {
            return None;
        }
    }
    match timezone_start {
        Some(idx) if idx > 0 => Some((&value[..idx], Some(&value[idx..]))),
        Some(_) => None,
        None => Some((value, None)),
    }
}

fn validate_date_components(
    field: &str,
    year: u32,
    month: Option<u32>,
    day: Option<u32>,
    vr: &str,
) -> Result<(), Error> {
    if year == 0 {
        return Err(Error::Metadata {
            reason: format!("{field} has invalid DICOM {vr} year"),
        });
    }
    let Some(month) = month else {
        return Ok(());
    };
    if !(1..=12).contains(&month) {
        return Err(Error::Metadata {
            reason: format!("{field} has invalid DICOM {vr} month"),
        });
    }
    let Some(day) = day else {
        return Ok(());
    };
    let max_day = days_in_month(year, month);
    if day == 0 || day > max_day {
        return Err(Error::Metadata {
            reason: format!("{field} has invalid DICOM {vr} day"),
        });
    }
    Ok(())
}

fn validate_time_components(field: &str, time: &str, vr: &str) -> Result<(), Error> {
    let hour = parse_decimal_component(&time[0..2]);
    if hour > 23 {
        return Err(Error::Metadata {
            reason: format!("{field} has invalid DICOM {vr} hour"),
        });
    }
    if time.len() >= 4 {
        let minute = parse_decimal_component(&time[2..4]);
        if minute > 59 {
            return Err(Error::Metadata {
                reason: format!("{field} has invalid DICOM {vr} minute"),
            });
        }
    }
    if time.len() >= 6 {
        let second = parse_decimal_component(&time[4..6]);
        if second > 59 {
            return Err(Error::Metadata {
                reason: format!("{field} has invalid DICOM {vr} second"),
            });
        }
    }
    Ok(())
}

fn validate_dt_components(field: &str, date_time: &str) -> Result<(), Error> {
    let year = parse_decimal_component(&date_time[0..4]);
    let month = (date_time.len() >= 6).then(|| parse_decimal_component(&date_time[4..6]));
    let day = (date_time.len() >= 8).then(|| parse_decimal_component(&date_time[6..8]));
    validate_date_components(field, year, month, day, "DT")?;
    if date_time.len() >= 10 {
        validate_time_components(field, &date_time[8..], "DT")?;
    }
    Ok(())
}

fn validate_dt_timezone(field: &str, timezone: &str) -> Result<(), Error> {
    let bytes = timezone.as_bytes();
    if timezone.len() != 5
        || !matches!(bytes.first(), Some(b'+' | b'-'))
        || !bytes[1..].iter().all(u8::is_ascii_digit)
    {
        return Err(Error::Metadata {
            reason: format!("{field} must use DICOM DT timezone format +/-ZZZZ"),
        });
    }
    let hour = parse_decimal_component(&timezone[1..3]);
    let minute = parse_decimal_component(&timezone[3..5]);
    if hour > 14 || (hour == 14 && minute != 0) || minute > 59 {
        return Err(Error::Metadata {
            reason: format!("{field} has invalid DICOM DT timezone offset"),
        });
    }
    Ok(())
}

fn parse_decimal_component(value: &str) -> u32 {
    value.parse::<u32>().unwrap_or(0)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400)
}

fn validate_optional_cs(field: &str, value: Option<&str>) -> Result<(), Error> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if value.chars().count() > 16
        || !value.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_' || byte == b' '
        })
    {
        return Err(Error::Metadata {
            reason: format!("{field} must be a valid DICOM CS value"),
        });
    }
    Ok(())
}

fn is_disallowed_text_control(ch: char) -> bool {
    ch.is_control()
}

#[cfg(test)]
mod tests;
