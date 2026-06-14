use serde::{Deserialize, Serialize};

use crate::Error;

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
        let mut metadata = Self::default();
        let resources = fhir_resources(value)?;
        let report = anchored_diagnostic_report(&resources)?;
        map_fhir_diagnostic_report(report, &mut metadata);

        let subject = required_reference(report, "subject", "FHIR DiagnosticReport")?;
        let patient = resolve_unique_fhir_reference(&resources, subject, "Patient")?;
        map_fhir_patient(patient, &mut metadata);

        let specimen_ref =
            required_reference_array_item(report, "specimen", "FHIR DiagnosticReport")?;
        let specimen = resolve_unique_fhir_reference(&resources, specimen_ref, "Specimen")?;
        map_fhir_specimen(specimen, &mut metadata);

        let based_on_ref =
            required_reference_array_item(report, "basedOn", "FHIR DiagnosticReport")?;
        let service_request =
            resolve_unique_fhir_reference(&resources, based_on_ref, "ServiceRequest")?;
        map_fhir_service_request(service_request, &mut metadata);

        metadata.validate_strict()?;
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

    pub(crate) fn validated_for_writer(&self) -> Result<ValidatedDicomMetadata<'_>, Error> {
        self.validate_strict()?;
        validate_optional_vr("patient_name", "PN", self.patient_name.as_deref(), 64)?;
        validate_optional_vr("patient_id", "LO", self.patient_id.as_deref(), 64)?;
        validate_optional_da("patient_birth_date", self.patient_birth_date.as_deref())?;
        validate_optional_cs("patient_sex", self.patient_sex.as_deref())?;
        validate_optional_vr(
            "accession_number",
            "SH",
            self.accession_number.as_deref(),
            16,
        )?;
        validate_optional_ui("study_instance_uid", self.study_instance_uid.as_deref())?;
        validate_optional_vr("study_id", "SH", self.study_id.as_deref(), 16)?;
        validate_optional_da("study_date", self.study_date.as_deref())?;
        validate_optional_tm("study_time", self.study_time.as_deref())?;
        validate_optional_vr(
            "study_description",
            "LO",
            self.study_description.as_deref(),
            64,
        )?;
        validate_optional_vr(
            "referring_physician_name",
            "PN",
            self.referring_physician_name.as_deref(),
            64,
        )?;
        validate_optional_cs("laterality", self.laterality.as_deref())?;
        validate_optional_vr("manufacturer", "LO", self.manufacturer.as_deref(), 64)?;
        validate_optional_vr(
            "manufacturer_model_name",
            "LO",
            self.manufacturer_model_name.as_deref(),
            64,
        )?;
        validate_optional_vr(
            "device_serial_number",
            "LO",
            self.device_serial_number.as_deref(),
            64,
        )?;
        validate_optional_vr(
            "software_versions",
            "LO",
            self.software_versions.as_deref(),
            64,
        )?;
        validate_optional_da("content_date", self.content_date.as_deref())?;
        validate_optional_tm("content_time", self.content_time.as_deref())?;
        validate_optional_dt(
            "acquisition_date_time",
            self.acquisition_date_time.as_deref(),
        )?;
        validate_optional_vr(
            "container_identifier",
            "LO",
            self.container_identifier.as_deref(),
            64,
        )?;
        validate_optional_vr(
            "specimen_identifier",
            "LO",
            self.specimen_identifier.as_deref(),
            64,
        )?;
        validate_optional_vr(
            "specimen_description",
            "LO",
            self.specimen_description.as_deref(),
            64,
        )?;
        validate_optional_cs("focus_method", self.focus_method.as_deref())?;
        Ok(ValidatedDicomMetadata { metadata: self })
    }
}

#[derive(Debug)]
pub(crate) struct ValidatedDicomMetadata<'a> {
    metadata: &'a DicomMetadata,
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
    /// Map metadata JSON into either FHIR R4 or strict DICOM metadata input.
    pub fn from_json_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if metadata_json_is_supported_fhir(&value) {
            Ok(Self::FhirR4Bundle(value))
        } else {
            let metadata: DicomMetadata = serde_json::from_value(value)?;
            Ok(Self::Strict(Box::new(metadata)))
        }
    }

    pub(crate) fn resolve(&self) -> Result<DicomMetadata, Error> {
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

fn metadata_json_is_supported_fhir(value: &serde_json::Value) -> bool {
    matches!(
        value
            .get("resourceType")
            .and_then(serde_json::Value::as_str),
        Some("Bundle" | "Patient" | "Specimen" | "ServiceRequest" | "DiagnosticReport")
    )
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

fn validate_optional_vr(
    field: &str,
    vr: &str,
    value: Option<&str>,
    max_chars: usize,
) -> Result<(), Error> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.chars().any(is_disallowed_text_control) {
        return Err(Error::Metadata {
            reason: format!("{field} contains control characters not allowed in DICOM {vr}"),
        });
    }
    if value.chars().count() > max_chars {
        return Err(Error::Metadata {
            reason: format!("{field} exceeds DICOM {vr} limit of {max_chars} characters"),
        });
    }
    Ok(())
}

fn validate_optional_ui(field: &str, value: Option<&str>) -> Result<(), Error> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if value.len() > 64
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
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
    ch.is_control() && ch != '\t' && ch != '\n' && ch != '\r'
}

#[cfg(test)]
mod tests {
    use super::{DicomMetadata, MetadataSource};

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
    fn fhir_bundle_rejects_multiple_reports_and_unreferenced_same_type_resources() {
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

    #[test]
    fn writer_metadata_validation_rejects_invalid_vr_values() {
        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_instance_uid = Some("1..2".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("study_instance_uid"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_date = Some("2026-06-14".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("study_date"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.patient_sex = Some("female".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("patient_sex"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_description = Some("A".repeat(65));
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("study_description"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.patient_name = Some("BAD\u{0007}NAME".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("patient_name"));
    }

    #[test]
    fn writer_metadata_validation_accepts_semantic_da_tm_dt_values() {
        let mut metadata = DicomMetadata::research_placeholder();
        metadata.patient_birth_date = Some("20240229".to_string());
        metadata.study_date = Some("20260614".to_string());
        metadata.study_time = Some("235959.123456".to_string());
        metadata.content_time = Some("00".to_string());
        metadata.acquisition_date_time = Some("20240229235959.123456+0530".to_string());

        metadata.validated_for_writer().unwrap();
    }

    #[test]
    fn validate_strict_preserves_required_field_contract() {
        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_date = Some("20261301".to_string());

        metadata.validate_strict().unwrap();
    }

    #[test]
    fn writer_metadata_validation_rejects_invalid_semantic_da_tm_dt_values() {
        let mut metadata = DicomMetadata::research_placeholder();
        metadata.patient_birth_date = Some("20230229".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("patient_birth_date"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_date = Some("20261301".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("study_date"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_time = Some("240000".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("study_time"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.content_time = Some("235960".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("content_time"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_time = Some("1200.1".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("study_time"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.acquisition_date_time = Some("20260229235959".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("acquisition_date_time"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.acquisition_date_time = Some("20260614235959.1234567".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("acquisition_date_time"));

        let mut metadata = DicomMetadata::research_placeholder();
        metadata.acquisition_date_time = Some("20260614235959+1401".to_string());
        assert!(metadata
            .validated_for_writer()
            .unwrap_err()
            .to_string()
            .contains("acquisition_date_time"));
    }
}
