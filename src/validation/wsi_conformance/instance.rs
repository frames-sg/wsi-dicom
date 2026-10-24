use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

use super::fields::{
    optional_string, require_numeric_value, required_finite_float64, required_positive_u64,
    required_string, required_type_two_string, sequence_items,
};
use super::lossy_encoding::{lossiness, Lossiness};
use super::nested::Nested;
use super::pixel_syntax::{
    is_motion_syntax, is_native, motion_departures, planar_configuration_section,
    reject_motion_syntax, standard_photometric, validate_photometric_for_syntax, NOT_NATIVE,
};
use super::VL_WSI;
use crate::icc::validate_dicom_icc_profile;
use crate::validation::iod::EDITION;

pub(super) fn validate_core_image_profile_rule(object: &DefaultDicomObject) -> Result<(), String> {
    let image_type = object
        .element(tags::IMAGE_TYPE)
        .map_err(|_| "Image Type is missing".to_string())?
        .to_multi_str()
        .map_err(|err| format!("Image Type is invalid: {err}"))?;
    if image_type.len() != 4 {
        return Err(format!(
            "Image Type has {} values, expected four",
            image_type.len()
        ));
    }
    if image_type[2].trim() != "VOLUME" {
        return Err(format!(
            "Image Type Value 3 is {}; the core profile accepts VOLUME images only, not LABEL, OVERVIEW or THUMBNAIL images",
            image_type[2].trim()
        ));
    }
    if !matches!(image_type[0].trim(), "ORIGINAL" | "DERIVED")
        || image_type[1].trim() != "PRIMARY"
        || !matches!(image_type[3].trim(), "NONE" | "RESAMPLED")
    {
        return Err(format!(
            "Image Type is {}; the core profile accepts ORIGINAL or DERIVED\\PRIMARY\\VOLUME\\NONE or RESAMPLED only",
            image_type.join("\\")
        ));
    }
    super::functional_groups::validate_functional_groups(object)?;
    for (tag, name) in [
        (tags::INSTANCE_NUMBER, "Instance Number"),
        (tags::CONTENT_DATE, "Content Date"),
        (tags::CONTENT_TIME, "Content Time"),
    ] {
        required_string(object, tag, name)?;
    }
    required_type_two_string(
        object,
        tags::POSITION_REFERENCE_INDICATOR,
        "Position Reference Indicator",
    )?;
    if required_finite_float64(object, tags::IMAGED_VOLUME_DEPTH, "Imaged Volume Depth")? <= 0.0 {
        return Err("Imaged Volume Depth must be positive".into());
    }
    if object.element(tags::CONCATENATION_UID).is_ok() {
        return Err(
            "Concatenation UID (0020,9161) is present; the core profile excludes Concatenations, so each image is one SOP Instance"
                .into(),
        );
    }
    required_string(object, tags::ACQUISITION_DATE_TIME, "Acquisition DateTime")?;
    sequence_items(
        object,
        tags::ACQUISITION_CONTEXT_SEQUENCE,
        "Acquisition Context Sequence",
    )?;
    for (tag, name) in [
        (tags::MANUFACTURER, "Manufacturer"),
        (tags::MANUFACTURER_MODEL_NAME, "Manufacturer's Model Name"),
        (tags::DEVICE_SERIAL_NUMBER, "Device Serial Number"),
        (tags::SOFTWARE_VERSIONS, "Software Versions"),
    ] {
        required_string(object, tag, name)?;
    }
    let modality = required_string(object, tags::MODALITY, "Modality")?;
    if modality != "SM" {
        return Err(format!("Modality is {modality}, expected SM"));
    }
    let dimension_type = required_string(
        object,
        tags::DIMENSION_ORGANIZATION_TYPE,
        "Dimension Organization Type",
    )?;
    if dimension_type != "TILED_FULL" {
        return Err(format!(
            "Dimension Organization Type is {dimension_type}; the core profile accepts TILED_FULL only, not TILED_SPARSE or 3D"
        ));
    }
    let planes = required_positive_u64(
        object,
        tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES,
        "Total Pixel Matrix Focal Planes",
    )?;
    if planes != 1 {
        return Err(format!(
            "Total Pixel Matrix Focal Planes is {planes}; the core profile accepts a single focal plane only"
        ));
    }
    for (tag, name, expected, configuration) in [
        (
            tags::VOLUMETRIC_PROPERTIES,
            "Volumetric Properties",
            "VOLUME",
            "images whose pixels represent the imaged volume",
        ),
        (
            tags::SPECIMEN_LABEL_IN_IMAGE,
            "Specimen Label in Image",
            "NO",
            "images without the slide label",
        ),
        (
            tags::BURNED_IN_ANNOTATION,
            "Burned In Annotation",
            "NO",
            "images without burned in annotation",
        ),
        (
            tags::EXTENDED_DEPTH_OF_FIELD,
            "Extended Depth of Field",
            "NO",
            "images without extended depth of field (focus stacking)",
        ),
    ] {
        let value = required_string(object, tag, name)?;
        if value != expected {
            return Err(format!(
                "{name} is {value}; the core profile accepts {configuration} only ({name} {expected})"
            ));
        }
    }
    let focus_method = required_string(object, tags::FOCUS_METHOD, "Focus Method")?;
    if !matches!(focus_method.as_str(), "AUTO" | "MANUAL") {
        return Err(format!(
            "Focus Method is {focus_method}, expected AUTO or MANUAL"
        ));
    }
    Ok(())
}

pub(super) fn validate_optical_path_structure_rule(
    object: &DefaultDicomObject,
) -> Result<(), String> {
    let declared = required_positive_u64(
        object,
        tags::NUMBER_OF_OPTICAL_PATHS,
        "Number of Optical Paths",
    )?;
    let optical_paths =
        sequence_items(object, tags::OPTICAL_PATH_SEQUENCE, "Optical Path Sequence")?;
    if declared != 1 || optical_paths.len() != 1 {
        return Err(format!(
            "Number of Optical Paths is {declared} and the Optical Path Sequence has {} Items; the core profile accepts a single optical path only",
            optical_paths.len()
        ));
    }
    let identifier = required_string(
        &optical_paths[0],
        tags::OPTICAL_PATH_IDENTIFIER,
        "Optical Path Identifier",
    )?;
    let illumination = sequence_items(
        &optical_paths[0],
        tags::ILLUMINATION_TYPE_CODE_SEQUENCE,
        "Illumination Type Code Sequence",
    )?;
    if illumination.is_empty() {
        return Err("Illumination Type Code Sequence is empty".into());
    }
    let color_present = optical_paths[0]
        .element(tags::ILLUMINATION_COLOR_CODE_SEQUENCE)
        .ok()
        .and_then(|element| element.items())
        .is_some_and(|items| !items.is_empty());
    let wavelength_present = optical_paths[0]
        .element(tags::ILLUMINATION_WAVE_LENGTH)
        .ok()
        .and_then(|element| element.to_float64().ok())
        .is_some_and(|value| value.is_finite() && value > 0.0);
    if !color_present && !wavelength_present {
        return Err(
            "Optical Path requires Illumination Color Code Sequence or Illumination Wave Length"
                .into(),
        );
    }

    let shared = sequence_items(
        object,
        tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
        "Shared Functional Groups Sequence",
    )?;
    if shared.len() != 1 {
        return Err(format!(
            "Shared Functional Groups Sequence has {} items, expected one",
            shared.len()
        ));
    }
    // Identification is optional for TILED_FULL; when supplied, every reference
    // must identify the sole declared optical path.
    let frames = object
        .element(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .ok()
        .and_then(|element| element.items());
    for group in shared.iter().chain(frames.into_iter().flatten()) {
        if group
            .element(tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE)
            .is_err()
        {
            continue;
        }
        let references = sequence_items(
            group,
            tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE,
            "Optical Path Identification Sequence",
        )?;
        if references.len() != 1 {
            return Err("Optical Path Identification Sequence must contain one item".into());
        }
        let referenced = required_string(
            &references[0],
            tags::OPTICAL_PATH_IDENTIFIER,
            "Referenced Optical Path Identifier",
        )?;
        if referenced != identifier {
            return Err(format!(
                "referenced Optical Path Identifier {referenced:?} does not match {identifier:?}"
            ));
        }
    }
    Ok(())
}

/// C.11.15.1.1 constrains every ICC Profile (0028,2000), wherever an IOD's
/// Modules place one: the ICC Profile Module, Optical Path Items, an Icon
/// Image or a Parametric Map. A VL WSI color Optical Path also needs one
/// (C.8.12.5.1.4).
pub(super) fn validate_icc_rule(
    object: &DefaultDicomObject,
    iod: &str,
    nested: &Nested<'_>,
) -> Result<(), String> {
    if iod == VL_WSI {
        optical_path_profiles(object)?;
    }
    let mut violations = Vec::new();
    for found in &nested.profiles {
        let Ok(element) = found.item.element(tags::ICC_PROFILE) else {
            continue;
        };
        match element.to_bytes() {
            Ok(bytes) => {
                if let Err(err) = validate_dicom_icc_profile(bytes.as_ref()) {
                    violations.push(format!(
                        "{} has an invalid ICC Profile: {err}",
                        found.profile_label()
                    ));
                }
            }
            Err(err) => violations.push(format!(
                "{} ICC Profile is not binary: {err}",
                found.profile_label()
            )),
        }
    }
    super::violation_result(violations)
}

fn optical_path_profiles(object: &DefaultDicomObject) -> Result<(), String> {
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
        if optical_path.element(tags::ICC_PROFILE).is_err() && photometric != "MONOCHROME2" {
            return Err(format!(
                "color Optical Path item {index} is missing ICC Profile"
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_monochrome_rule(object: &DefaultDicomObject) -> Result<(), String> {
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

/// C.7.6.1.1.5: Frames that underwent lossy compression make Lossy Image
/// Compression "01", and the Method and Ratio histories correspond Value by
/// Value. VL WSI requires the flag, and with "01" both histories (Table
/// C.8.12.4-1); in another IOD the Module tables decide their presence.
pub(super) fn validate_lossy_rule(object: &DefaultDicomObject, iod: &str) -> Result<(), String> {
    let flag = if iod == VL_WSI {
        Some(required_string(
            object,
            tags::LOSSY_IMAGE_COMPRESSION,
            "Lossy Image Compression",
        )?)
    } else {
        optional_string(object, tags::LOSSY_IMAGE_COMPRESSION)
            .map_err(|_| "Lossy Image Compression is empty".to_string())?
    };
    let encoding = lossiness(object);
    match flag.as_deref() {
        Some("00") => {
            if let Lossiness::Lossy { evidence, .. } = encoding {
                return Err(format!(
                    "Lossy Image Compression is 00, but the Frames are {evidence}; an image that has undergone lossy compression is 01"
                ));
            }
            lossy_history(object, iod, encoding, false)
        }
        Some("01") => lossy_history(object, iod, encoding, iod == VL_WSI),
        None => {
            if let Lossiness::Lossy { evidence, .. } = encoding {
                return Err(format!(
                    "Lossy Image Compression (0028,2110) is absent, but the Frames are {evidence}; the Value of an image that has undergone lossy compression is 01"
                ));
            }
            lossy_history(object, iod, encoding, false)
        }
        Some(flag) => Err(format!(
            "Lossy Image Compression is {flag:?}, expected 00 or 01"
        )),
    }
}

/// C.7.6.1.1.5.1 and C.7.6.1.1.5.2: the Method and Ratio Values of
/// successive steps correspond in order, so they are as many; the last
/// Method names the coding the Frames show. `required` asks for both.
fn lossy_history(
    object: &DefaultDicomObject,
    iod: &str,
    encoding: Lossiness,
    required: bool,
) -> Result<(), String> {
    let methods = match object.element(tags::LOSSY_IMAGE_COMPRESSION_METHOD) {
        Ok(element) => Some(
            element
                .to_multi_str()
                .map_err(|err| format!("Lossy Image Compression Method is invalid: {err}"))?
                .to_vec(),
        ),
        Err(_) if required => return Err("Lossy Image Compression Method is missing".to_string()),
        Err(_) => None,
    };
    let ratios = match object.element(tags::LOSSY_IMAGE_COMPRESSION_RATIO) {
        Ok(element) => Some(
            element
                .to_multi_float64()
                .map_err(|err| format!("Lossy Image Compression Ratio is invalid: {err}"))?,
        ),
        Err(_) if required => return Err("Lossy Image Compression Ratio is missing".to_string()),
        Err(_) => None,
    };
    if let (Some(methods), Some(ratios)) = (&methods, &ratios) {
        if methods.is_empty() || methods.len() != ratios.len() {
            return Err(
                "lossy method and ratio histories have different value multiplicities".into(),
            );
        }
    }
    if let Some(methods) = &methods {
        if methods.iter().any(|method| method.trim().is_empty()) {
            return Err("Lossy Image Compression Method contains an empty value".into());
        }
    }
    if let Some(ratios) = &ratios {
        if ratios
            .iter()
            .any(|ratio| !ratio.is_finite() || *ratio <= 0.0)
        {
            return Err("Lossy Image Compression Ratio must contain positive finite values".into());
        }
    }
    let (
        Some(methods),
        Lossiness::Lossy {
            method: Some(expected_method),
            evidence,
        },
    ) = (methods, encoding)
    else {
        return Ok(());
    };
    let actual_method = methods
        .last()
        .map(|method| method.trim().to_string())
        .unwrap_or_default();
    if actual_method != expected_method {
        let _ = iod;
        return Err(format!(
            "the Frames are {evidence}, so the last Lossy Image Compression Method is {expected_method}, but it is {actual_method:?}"
        ));
    }
    Ok(())
}

/// The image's sample format and its Transfer Syntax: PS3.5 8.1.1 and 8.2
/// with the PS3.3 C.7.6.3.1.2 Photometric Interpretation rules in every image
/// IOD, held to the stricter VL WSI Values (C.8.12.4.1.5 and Table
/// C.8.12.4-1) in VL WSI.
/// The advisory for a Segmentation or Parametric Map whose Pixel Data a video
/// Transfer Syntax encodes: its bit stream is not validated, and every video
/// syntax is lossy, which PS3.3 advises against for both IODs.
pub(super) fn video_syntax_advisory(object: &DefaultDicomObject, iod: &str) -> Option<String> {
    let syntax = object.meta().transfer_syntax.trim_end_matches(['\0', ' ']);
    if !is_motion_syntax(syntax) {
        return None;
    }
    let advice = match iod {
        "A.51" => format!(
            "it is not advisable to lossy compress a Segmentation Instance, and in particular BINARY or LABELMAP Segmentation Instances should not be lossy compressed (PS3.3 {EDITION} C.8.20.2.2)"
        ),
        "A.75" => format!(
            "it is not advisable to lossy compress a Parametric Map SOP Instance, since that will alter the quantitative values that are the intent of the object (PS3.3 {EDITION} C.8.32.2.1)"
        ),
        _ => return None,
    };
    Some(format!(
        "Transfer Syntax {syntax} encodes the Pixel Data as a lossy video bit stream, which is not validated: a pathology Segmentation or Parametric Map is not video; {advice}"
    ))
}

pub(super) fn validate_image_metadata_rule(
    object: &DefaultDicomObject,
    iod: &str,
) -> Result<(), String> {
    let syntax = object.meta().transfer_syntax.trim_end_matches('\0');
    if iod != VL_WSI {
        let mut violations = Vec::new();
        general_image_metadata(object, syntax, &mut violations);
        return super::violation_result(violations);
    }
    reject_motion_syntax(object, syntax)?;
    let samples = required_positive_u64(object, tags::SAMPLES_PER_PIXEL, "Samples per Pixel")?;
    let photometric = required_string(
        object,
        tags::PHOTOMETRIC_INTERPRETATION,
        "Photometric Interpretation",
    )?;
    match samples {
        1 if photometric != "MONOCHROME2" => {
            return Err(format!(
                "Samples per Pixel is 1 but Photometric Interpretation is {photometric}"
            ));
        }
        3 if !matches!(
            photometric.as_str(),
            "RGB" | "YBR_FULL_422" | "YBR_ICT" | "YBR_RCT"
        ) =>
        {
            return Err(format!(
                "Samples per Pixel is 3 but Photometric Interpretation is {photometric}"
            ));
        }
        1 | 3 => {}
        _ => return Err(format!("unsupported Samples per Pixel value {samples}")),
    }
    let allocated = required_positive_u64(object, tags::BITS_ALLOCATED, "Bits Allocated")?;
    let stored = required_positive_u64(object, tags::BITS_STORED, "Bits Stored")?;
    let high_bit = object
        .element(tags::HIGH_BIT)
        .map_err(|_| "High Bit is missing".to_string())?
        .to_int::<u64>()
        .map_err(|err| format!("High Bit is invalid: {err}"))?;
    if !matches!(allocated, 8 | 16) || stored != allocated {
        return Err(format!(
            "VL WSI requires Bits Allocated 8 or 16 and equal Bits Stored, found {allocated}/{stored}"
        ));
    }
    if high_bit != stored - 1 {
        return Err(format!(
            "High Bit {high_bit} does not equal Bits Stored - 1"
        ));
    }
    let representation = object
        .element(tags::PIXEL_REPRESENTATION)
        .map_err(|_| "Pixel Representation is missing".to_string())?
        .to_int::<u64>()
        .map_err(|err| format!("Pixel Representation is invalid: {err}"))?;
    if representation != 0 {
        return Err(format!(
            "Pixel Representation is {representation}, expected unsigned 0"
        ));
    }
    if samples == 3 {
        require_numeric_value(
            object,
            tags::PLANAR_CONFIGURATION,
            "Planar Configuration",
            0.0,
        )?;
    }
    validate_photometric_for_syntax(syntax, &photometric, allocated)
}

/// Photometric Interpretations of three samples (PS3.3 C.7.6.3.1.2).
const THREE_SAMPLES: [&str; 7] = [
    "RGB",
    "YBR_FULL",
    "YBR_FULL_422",
    "YBR_PARTIAL_420",
    "YBR_ICT",
    "YBR_RCT",
    "XYB",
];
/// Photometric Interpretations whose Planar Configuration shall be 0 (PS3.3
/// C.7.6.3.1.2).
const COLOR_BY_PIXEL: [&str; 4] = ["YBR_FULL_422", "YBR_PARTIAL_420", "YBR_ICT", "YBR_RCT"];

/// PS3.5 8.1.1, 8.2 and its subsections, and PS3.3 C.7.6.3.1.2, for an image
/// of any IOD. Absent attributes are presence violations the IOD checks
/// report.
fn general_image_metadata(object: &DefaultDicomObject, syntax: &str, violations: &mut Vec<String>) {
    let integer = |tag| {
        object
            .get(tag)
            .and_then(|element| element.to_int::<u64>().ok())
    };
    if let Some((section, departures)) = motion_departures(object, syntax) {
        if !departures.is_empty() {
            violations.push(format!(
                "Transfer Syntax {syntax} fixes the Pixel Data attributes it encodes, and the image departs from them: {} (PS3.5 {EDITION} {section})",
                departures.join(", ")
            ));
        }
    }
    let samples = integer(tags::SAMPLES_PER_PIXEL);
    let photometric = optional_string(object, tags::PHOTOMETRIC_INTERPRETATION)
        .ok()
        .flatten();
    if let (Some(samples), Some(photometric)) = (samples, photometric.as_deref()) {
        let expected = if THREE_SAMPLES.contains(&photometric) {
            Some(3)
        } else if matches!(photometric, "MONOCHROME1" | "MONOCHROME2" | "PALETTE COLOR") {
            Some(1)
        } else {
            None
        };
        if expected.is_some_and(|expected| expected != samples) {
            violations.push(format!(
                "Photometric Interpretation {photometric} is used with Samples per Pixel {samples}; it may be used only with {} (PS3.3 {EDITION} C.7.6.3.1.2)",
                expected.unwrap_or_default()
            ));
        }
        if COLOR_BY_PIXEL.contains(&photometric) && integer(tags::PLANAR_CONFIGURATION) != Some(0) {
            violations.push(format!(
                "Planar Configuration (0028,0006) is {} with Photometric Interpretation {photometric}; it shall be 0 (PS3.3 {EDITION} C.7.6.3.1.2)",
                integer(tags::PLANAR_CONFIGURATION).map_or_else(|| "absent".to_string(), |value| value.to_string())
            ));
        }
        photometric_for_syntax(syntax, photometric, violations);
    }
    if let (Some(section), Some(3)) = (planar_configuration_section(syntax), samples) {
        if let Some(planar) = integer(tags::PLANAR_CONFIGURATION).filter(|planar| *planar != 0) {
            violations.push(format!(
                "Planar Configuration (0028,0006) is {planar}; Transfer Syntax {syntax} encodes the manner of interleaving components itself, so it shall be 0 (PS3.5 {EDITION} {section})"
            ));
        }
    }
    sample_bits(object, violations);
}

/// PS3.5 8.2 and the table of each syntax: the Standard Photometric
/// Interpretations it admits.
fn photometric_for_syntax(syntax: &str, photometric: &str, violations: &mut Vec<String>) {
    if is_native(syntax) && NOT_NATIVE.contains(&photometric) {
        violations.push(format!(
            "Photometric Interpretation {photometric} is used with Native Format Transfer Syntax {syntax}; it is not encodable uncompressed and shall only be used for Pixel Data in an Encapsulated format (PS3.5 {EDITION} 8.2 and PS3.3 {EDITION} C.7.6.3.1.2)"
        ));
        return;
    }
    let Some((permitted, citation)) = standard_photometric(syntax) else {
        return;
    };
    if !permitted.contains(&photometric) {
        violations.push(format!(
            "Photometric Interpretation {photometric} is not permitted with Transfer Syntax {syntax}, which admits {}; no other Standard Photometric Interpretation shall be used (PS3.5 {EDITION} {citation})",
            permitted.join(", ")
        ));
    }
}

/// PS3.5 8.1.1: Bits Allocated is 1 or a multiple of 8, Bits Stored at most
/// Bits Allocated, High Bit one less than Bits Stored, and samples unsigned
/// or 2's complement as Pixel Representation says; so the sign bit of a 2's
/// complement sample is the High Bit. Floating point Pixel Data has no Bits
/// Stored, High Bit or Pixel Representation (PS3.5 8.2).
fn sample_bits(object: &DefaultDicomObject, violations: &mut Vec<String>) {
    let integer = |tag| {
        object
            .get(tag)
            .and_then(|element| element.to_int::<u64>().ok())
    };
    let allocated = integer(tags::BITS_ALLOCATED);
    if let Some(allocated) = allocated.filter(|bits| *bits != 1 && bits % 8 != 0) {
        violations.push(format!(
            "Bits Allocated (0028,0100) is {allocated}; it shall be either 1 or a multiple of 8 (PS3.5 {EDITION} 8.1.1)"
        ));
    }
    let stored = integer(tags::BITS_STORED);
    if let (Some(allocated), Some(stored)) = (allocated, stored) {
        if stored > allocated {
            violations.push(format!(
                "Bits Stored (0028,0101) {stored} is larger than Bits Allocated (0028,0100) {allocated}; it shall never be (PS3.5 {EDITION} 8.1.1)"
            ));
        }
    }
    if let (Some(high_bit), Some(stored)) = (integer(tags::HIGH_BIT), stored) {
        if high_bit.checked_add(1) != Some(stored) {
            violations.push(format!(
                "High Bit (0028,0102) is {high_bit}; it shall be one less than Bits Stored (0028,0101) {stored}, so the High Bit is the sign bit of a 2's complement sample (PS3.5 {EDITION} 8.1.1)"
            ));
        }
    }
    if let Some(representation) = integer(tags::PIXEL_REPRESENTATION).filter(|value| *value > 1) {
        violations.push(format!(
            "Pixel Representation (0028,0103) is {representation}; a Pixel Sample is an unsigned integer (0) or a binary 2's complement integer (1) (PS3.5 {EDITION} 8.1.1)"
        ));
    }
}
