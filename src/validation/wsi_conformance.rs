use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::{ValidationCheck, ValidationStatus};
use crate::icc::validate_dicom_icc_profile;
use crate::lossy::method_for_lossy_transfer_syntax;
use crate::uid::is_valid_dicom_uid;
use crate::VL_WSI_SOP_CLASS_UID;

const ICC_RULE: &str = "intrinsic-wsi-dicom-2026c-icc-profile";
const MONOCHROME_RULE: &str = "intrinsic-wsi-dicom-2026c-monochrome-presentation";
const LOSSY_RULE: &str = "intrinsic-wsi-dicom-2026c-lossy-history";
const SPECIMEN_RULE: &str = "intrinsic-wsi-dicom-2026c-specimen-identity";
const SPECIMEN_SET_RULE: &str = "intrinsic-wsi-dicom-2026c-specimen-uid-set";
const DIMENSION_RULE: &str = "intrinsic-wsi-dicom-2026c-dimension-order";

pub(super) fn run_intrinsic_wsi_conformance_checks(path: &Path) -> Vec<ValidationCheck> {
    let Ok(object) = dicom_object::open_file(path) else {
        return Vec::new();
    };
    if !is_vl_wsi(&object) {
        return Vec::new();
    }

    vec![
        rule_check(
            path,
            ICC_RULE,
            "DICOM PS3.3 2026c C.11.15 and C.8.12.5",
            validate_icc_rule(&object),
        ),
        rule_check(
            path,
            MONOCHROME_RULE,
            "DICOM PS3.3 2026c C.8.12.4",
            validate_monochrome_rule(&object),
        ),
        rule_check(
            path,
            LOSSY_RULE,
            "DICOM PS3.3 2026c C.8.12.4 and C.7.6",
            validate_lossy_rule(&object),
        ),
        rule_check(
            path,
            SPECIMEN_RULE,
            "DICOM PS3.3 2026c C.7.6.22 and 10.14",
            specimen_identities(&object).map(|_| ()),
        ),
        rule_check(
            path,
            DIMENSION_RULE,
            "DICOM PS3.3 2026c 7.5.2",
            validate_dimension_rule(&object),
        ),
    ]
}

pub(super) fn run_specimen_uid_set_check(files: &[PathBuf]) -> Option<ValidationCheck> {
    let mut identities = BTreeMap::<String, (SpecimenIdentity, PathBuf)>::new();
    let mut saw_wsi = false;
    let mut failure = None;
    for path in files {
        let Ok(object) = dicom_object::open_file(path) else {
            continue;
        };
        if !is_vl_wsi(&object) {
            continue;
        }
        saw_wsi = true;
        match specimen_identities(&object) {
            Ok(specimens) => {
                for specimen in specimens {
                    if let Some((existing, existing_path)) = identities.get(&specimen.uid) {
                        if existing != &specimen {
                            failure = Some(format!(
                                "Specimen UID {} maps to conflicting identifiers or issuers in {} and {}",
                                specimen.uid,
                                existing_path.display(),
                                path.display()
                            ));
                            break;
                        }
                    } else {
                        identities.insert(specimen.uid.clone(), (specimen, path.clone()));
                    }
                }
            }
            Err(reason) => {
                failure = Some(format!(
                    "cannot compare specimen identities in {}: {reason}",
                    path.display()
                ));
            }
        }
        if failure.is_some() {
            break;
        }
    }
    if !saw_wsi {
        return None;
    }
    Some(rule_check(
        Path::new(""),
        SPECIMEN_SET_RULE,
        "DICOM PS3.3 2026c C.7.6.22",
        failure.map_or(Ok(()), Err),
    ))
}

fn is_vl_wsi(object: &DefaultDicomObject) -> bool {
    object
        .element(tags::SOP_CLASS_UID)
        .ok()
        .and_then(|element| element.to_str().ok())
        .is_some_and(|uid| uid.trim_end_matches('\0') == VL_WSI_SOP_CLASS_UID)
}

fn validate_icc_rule(object: &DefaultDicomObject) -> Result<(), String> {
    let photometric = required_string(
        object,
        tags::PHOTOMETRIC_INTERPRETATION,
        "Photometric Interpretation",
    )?;
    let optical_paths =
        sequence_items(object, tags::OPTICAL_PATH_SEQUENCE, "Optical Path Sequence")?;
    if optical_paths.is_empty() {
        return Err("Optical Path Sequence is empty".into());
    }
    for (index, optical_path) in optical_paths.iter().enumerate() {
        match optical_path.element(tags::ICC_PROFILE) {
            Ok(element) => {
                let bytes = element.to_bytes().map_err(|err| {
                    format!("Optical Path item {index} ICC Profile is not binary: {err}")
                })?;
                validate_dicom_icc_profile(bytes.as_ref()).map_err(|err| {
                    format!("Optical Path item {index} has an invalid ICC Profile: {err}")
                })?;
            }
            Err(_) if photometric != "MONOCHROME2" => {
                return Err(format!(
                    "color Optical Path item {index} is missing ICC Profile"
                ));
            }
            Err(_) => {}
        }
    }
    Ok(())
}

fn validate_monochrome_rule(object: &DefaultDicomObject) -> Result<(), String> {
    let photometric = required_string(
        object,
        tags::PHOTOMETRIC_INTERPRETATION,
        "Photometric Interpretation",
    )?;
    if photometric != "MONOCHROME2" {
        return Ok(());
    }
    let shape = required_string(
        object,
        tags::PRESENTATION_LUT_SHAPE,
        "Presentation LUT Shape",
    )?;
    if shape != "IDENTITY" {
        return Err(format!(
            "Presentation LUT Shape is {shape:?}, expected IDENTITY"
        ));
    }
    require_numeric_value(object, tags::RESCALE_INTERCEPT, "Rescale Intercept", 0.0)?;
    require_numeric_value(object, tags::RESCALE_SLOPE, "Rescale Slope", 1.0)?;
    Ok(())
}

fn validate_lossy_rule(object: &DefaultDicomObject) -> Result<(), String> {
    let flag = required_string(
        object,
        tags::LOSSY_IMAGE_COMPRESSION,
        "Lossy Image Compression",
    )?;
    match flag.as_str() {
        "00" => {
            if method_for_lossy_transfer_syntax(&object.meta().transfer_syntax).is_some() {
                return Err(
                    "lossy transfer syntax is paired with Lossy Image Compression = 00".into(),
                );
            }
        }
        "01" => {
            let methods = object
                .element(tags::LOSSY_IMAGE_COMPRESSION_METHOD)
                .map_err(|_| "Lossy Image Compression Method is missing".to_string())?
                .to_multi_str()
                .map_err(|err| format!("Lossy Image Compression Method is invalid: {err}"))?;
            let ratios = object
                .element(tags::LOSSY_IMAGE_COMPRESSION_RATIO)
                .map_err(|_| "Lossy Image Compression Ratio is missing".to_string())?
                .to_multi_float64()
                .map_err(|err| format!("Lossy Image Compression Ratio is invalid: {err}"))?;
            if methods.is_empty() || methods.len() != ratios.len() {
                return Err(
                    "lossy method and ratio histories have different value multiplicities".into(),
                );
            }
            if methods.iter().any(|method| method.trim().is_empty()) {
                return Err("Lossy Image Compression Method contains an empty value".into());
            }
            if ratios
                .iter()
                .any(|ratio| !ratio.is_finite() || *ratio <= 0.0)
            {
                return Err(
                    "Lossy Image Compression Ratio must contain positive finite values".into(),
                );
            }
            if let Some(expected_method) =
                method_for_lossy_transfer_syntax(&object.meta().transfer_syntax)
            {
                let actual_method = methods
                    .last()
                    .map(|method| method.trim())
                    .unwrap_or_default();
                if actual_method != expected_method {
                    return Err(format!(
                        "lossy transfer syntax requires final compression method {expected_method}, found {actual_method:?}"
                    ));
                }
            }
        }
        _ => {
            return Err(format!(
                "Lossy Image Compression is {flag:?}, expected 00 or 01"
            ))
        }
    }
    Ok(())
}

fn validate_dimension_rule(object: &DefaultDicomObject) -> Result<(), String> {
    let dimension_items = match object.element(tags::DIMENSION_INDEX_SEQUENCE) {
        Ok(element) => element
            .items()
            .ok_or_else(|| "Dimension Index Sequence is not a sequence".to_string())?,
        Err(_) => return Ok(()),
    };
    let pointers = dimension_items
        .iter()
        .map(|item| {
            item.element(tags::DIMENSION_INDEX_POINTER)
                .map_err(|_| "Dimension Index item is missing Dimension Index Pointer".to_string())?
                .value()
                .to_tag()
                .map_err(|err| format!("Dimension Index Pointer is invalid: {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let row_index = pointers
        .iter()
        .position(|pointer| *pointer == tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX);
    let column_index = pointers
        .iter()
        .position(|pointer| *pointer == tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX);
    let (Some(row_index), Some(column_index)) = (row_index, column_index) else {
        if row_index.is_none() && column_index.is_none() {
            return Ok(());
        }
        return Err(
            "Dimension Index Sequence must contain both row and column or omit both redundant TILED_FULL indices"
                .into(),
        );
    };
    if row_index >= column_index {
        return Err(
            "row must precede column in Dimension Index Sequence because row is slower-varying"
                .into(),
        );
    }

    let frame_rows = required_positive_u64(object, tags::ROWS, "Rows")?;
    let frame_columns = required_positive_u64(object, tags::COLUMNS, "Columns")?;
    let per_frame = sequence_items(
        object,
        tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
        "Per-frame Functional Groups Sequence",
    )?;
    for (frame, item) in per_frame.iter().enumerate() {
        let frame_content = item
            .element(tags::FRAME_CONTENT_SEQUENCE)
            .ok()
            .and_then(|element| element.items())
            .and_then(|items| items.first())
            .ok_or_else(|| format!("frame {frame} is missing Frame Content Sequence"))?;
        let values = frame_content
            .element(tags::DIMENSION_INDEX_VALUES)
            .map_err(|_| format!("frame {frame} is missing Dimension Index Values"))?
            .to_multi_int::<u32>()
            .map_err(|err| format!("frame {frame} has invalid Dimension Index Values: {err}"))?;
        if values.len() != pointers.len() {
            return Err(format!(
                "frame {frame} Dimension Index Values VM {} does not match {} index items",
                values.len(),
                pointers.len()
            ));
        }
        let position = item
            .element(tags::PLANE_POSITION_SLIDE_SEQUENCE)
            .ok()
            .and_then(|element| element.items())
            .and_then(|items| items.first())
            .ok_or_else(|| format!("frame {frame} is missing Plane Position Slide Sequence"))?;
        let row_position = required_positive_u64(
            position,
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            "Row Position in Total Image Pixel Matrix",
        )?;
        let column_position = required_positive_u64(
            position,
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            "Column Position in Total Image Pixel Matrix",
        )?;
        let expected_row = u32::try_from((row_position - 1) / frame_rows + 1)
            .map_err(|_| format!("frame {frame} row dimension index exceeds UL"))?;
        let expected_column = u32::try_from((column_position - 1) / frame_columns + 1)
            .map_err(|_| format!("frame {frame} column dimension index exceeds UL"))?;
        if values[row_index] != expected_row || values[column_index] != expected_column {
            return Err(format!(
                "frame {frame} Dimension Index Values do not match its row/column plane position"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpecimenIdentity {
    uid: String,
    identifier: String,
    issuer: Option<IssuerIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IssuerIdentity {
    local: Option<String>,
    universal: Option<String>,
    universal_type: Option<String>,
}

fn specimen_identities(object: &DefaultDicomObject) -> Result<Vec<SpecimenIdentity>, String> {
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
    let items = sequence_items(
        specimen,
        tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE,
        "Issuer of the Specimen Identifier Sequence",
    )?;
    if items.is_empty() {
        return Ok(None);
    }
    if items.len() != 1 {
        return Err(format!(
            "Specimen Description item {specimen_index} has {} issuer items, expected one",
            items.len()
        ));
    }
    let item = &items[0];
    let local = optional_string(item, tags::LOCAL_NAMESPACE_ENTITY_ID)?;
    let universal = optional_string(item, tags::UNIVERSAL_ENTITY_ID)?;
    let universal_type = optional_string(item, tags::UNIVERSAL_ENTITY_ID_TYPE)?;
    if local.is_none() && universal.is_none() {
        return Err(format!(
            "Specimen Description item {specimen_index} has an empty issuer item"
        ));
    }
    if universal.is_some() != universal_type.is_some() {
        return Err(format!(
            "Specimen Description item {specimen_index} must pair Universal Entity ID with its type"
        ));
    }
    if universal_type.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "DNS" | "EUI64" | "ISO" | "URI" | "UUID" | "X400" | "X500"
        )
    }) {
        return Err(format!(
            "Specimen Description item {specimen_index} has an invalid Universal Entity ID Type"
        ));
    }
    Ok(Some(IssuerIdentity {
        local,
        universal,
        universal_type,
    }))
}

fn sequence_items<'a>(
    object: &'a InMemDicomObject,
    tag: Tag,
    name: &str,
) -> Result<&'a [InMemDicomObject], String> {
    object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .items()
        .ok_or_else(|| format!("{name} is not a sequence"))
}

fn required_string(object: &InMemDicomObject, tag: Tag, name: &str) -> Result<String, String> {
    let value = object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .to_str()
        .map_err(|err| format!("{name} is invalid: {err}"))?
        .trim()
        .to_string();
    if value.is_empty() {
        Err(format!("{name} is empty"))
    } else {
        Ok(value)
    }
}

fn optional_string(object: &InMemDicomObject, tag: Tag) -> Result<Option<String>, String> {
    match object.element(tag) {
        Ok(element) => {
            let value = element
                .to_str()
                .map_err(|err| format!("element {tag:?} is invalid: {err}"))?
                .trim()
                .to_string();
            if value.is_empty() {
                Err(format!("element {tag:?} is empty"))
            } else {
                Ok(Some(value))
            }
        }
        Err(_) => Ok(None),
    }
}

fn required_positive_u64(object: &InMemDicomObject, tag: Tag, name: &str) -> Result<u64, String> {
    let value = object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .to_int::<u64>()
        .map_err(|err| format!("{name} is invalid: {err}"))?;
    if value == 0 {
        Err(format!("{name} must be positive"))
    } else {
        Ok(value)
    }
}

fn require_numeric_value(
    object: &InMemDicomObject,
    tag: Tag,
    name: &str,
    expected: f64,
) -> Result<(), String> {
    let value = object
        .element(tag)
        .map_err(|_| format!("{name} is missing"))?
        .to_float64()
        .map_err(|err| format!("{name} is invalid: {err}"))?;
    if value == expected {
        Ok(())
    } else {
        Err(format!("{name} is {value}, expected {expected}"))
    }
}

fn rule_check(
    path: &Path,
    name: &str,
    citation: &str,
    result: Result<(), String>,
) -> ValidationCheck {
    let (status, message) = match result {
        Ok(()) => (ValidationStatus::Passed, format!("{citation}: conformant")),
        Err(reason) => (ValidationStatus::Failed, format!("{citation}: {reason}")),
    };
    ValidationCheck {
        name: name.to_string(),
        path: (!path.as_os_str().is_empty()).then(|| path.to_path_buf()),
        status,
        command: Vec::new(),
        message,
        stdout: String::new(),
        stderr: String::new(),
    }
}
