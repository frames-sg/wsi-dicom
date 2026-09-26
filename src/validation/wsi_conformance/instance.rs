use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

use super::fields::{
    require_numeric_value, required_finite_float64, required_positive_u64, required_string,
    required_type_two_string, sequence_items,
};
use crate::icc::validate_dicom_icc_profile;
use crate::lossy::method_for_lossy_transfer_syntax;
use crate::VL_WSI_SOP_CLASS_UID;

pub(super) fn is_vl_wsi(object: &DefaultDicomObject) -> bool {
    object
        .element(tags::SOP_CLASS_UID)
        .ok()
        .and_then(|element| element.to_str().ok())
        .is_some_and(|uid| uid.trim_end_matches('\0') == VL_WSI_SOP_CLASS_UID)
}

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
    if !matches!(image_type[0].trim(), "ORIGINAL" | "DERIVED")
        || image_type[1].trim() != "PRIMARY"
        || image_type[2].trim() != "VOLUME"
        || !matches!(image_type[3].trim(), "NONE" | "RESAMPLED")
    {
        return Err(format!(
            "Image Type {:?} is outside the core VOLUME profile",
            image_type
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
        return Err("concatenations are outside the core profile".into());
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
            "Dimension Organization Type is {dimension_type}, expected TILED_FULL"
        ));
    }
    if required_positive_u64(
        object,
        tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES,
        "Total Pixel Matrix Focal Planes",
    )? != 1
    {
        return Err("Total Pixel Matrix Focal Planes must be one in the core profile".into());
    }
    for (tag, name, expected) in [
        (
            tags::VOLUMETRIC_PROPERTIES,
            "Volumetric Properties",
            "VOLUME",
        ),
        (
            tags::SPECIMEN_LABEL_IN_IMAGE,
            "Specimen Label in Image",
            "NO",
        ),
        (tags::BURNED_IN_ANNOTATION, "Burned In Annotation", "NO"),
        (
            tags::EXTENDED_DEPTH_OF_FIELD,
            "Extended Depth of Field",
            "NO",
        ),
    ] {
        let value = required_string(object, tag, name)?;
        if value != expected {
            return Err(format!("{name} is {value}, expected {expected}"));
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
            "core profile requires one optical path; declared {declared}, found {}",
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

pub(super) fn validate_icc_rule(object: &DefaultDicomObject) -> Result<(), String> {
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

pub(super) fn validate_lossy_rule(object: &DefaultDicomObject) -> Result<(), String> {
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
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_image_metadata_rule(object: &DefaultDicomObject) -> Result<(), String> {
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
    let syntax = object.meta().transfer_syntax.trim_end_matches('\0');
    let compatible = if photometric == "MONOCHROME2" || photometric == "RGB" {
        true
    } else {
        match syntax {
            "1.2.840.10008.1.2.4.50" => photometric == "YBR_FULL_422",
            "1.2.840.10008.1.2.4.90" | "1.2.840.10008.1.2.4.201" | "1.2.840.10008.1.2.4.202" => {
                photometric == "YBR_RCT"
            }
            // These syntaxes permit reversible and irreversible codestreams.
            "1.2.840.10008.1.2.4.91" | "1.2.840.10008.1.2.4.203" => {
                matches!(photometric.as_str(), "YBR_RCT" | "YBR_ICT")
            }
            _ => false,
        }
    };
    if !compatible || (syntax == "1.2.840.10008.1.2.4.50" && allocated != 8) {
        return Err(format!("Photometric Interpretation {photometric} and {allocated}-bit samples are incompatible with transfer syntax {syntax}"));
    }
    Ok(())
}
