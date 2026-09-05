use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::fields::{
    optional_string, require_numeric_value, required_finite_float64, required_float64,
    required_positive_u64, required_string, required_type_two_string, sequence_items,
    PixelSpacingMm, PlanePosition,
};
use crate::icc::validate_dicom_icc_profile;
use crate::lossy::method_for_lossy_transfer_syntax;
use crate::uid::is_valid_dicom_uid;
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

pub(super) fn validate_slide_coordinate_system_rule(
    object: &DefaultDicomObject,
) -> Result<(), String> {
    let origins = sequence_items(
        object,
        tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE,
        "Total Pixel Matrix Origin Sequence",
    )?;
    if origins.len() != 1 {
        return Err(format!(
            "Total Pixel Matrix Origin Sequence has {} items, expected one",
            origins.len()
        ));
    }
    let origin = &origins[0];
    required_finite_float64(
        origin,
        tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
        "X Offset in Slide Coordinate System",
    )?;
    required_finite_float64(
        origin,
        tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
        "Y Offset in Slide Coordinate System",
    )?;
    if let Ok(element) = origin.element(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM) {
        let value = element
            .to_float64()
            .map_err(|err| format!("Z Offset in Slide Coordinate System is invalid: {err}"))?;
        if !value.is_finite() {
            return Err("Z Offset in Slide Coordinate System must be finite".into());
        }
    }

    let orientation = object
        .element(tags::IMAGE_ORIENTATION_SLIDE)
        .map_err(|_| "Image Orientation (Slide) is missing".to_string())?
        .to_multi_float64()
        .map_err(|err| format!("Image Orientation (Slide) is invalid: {err}"))?;
    if orientation.len() != 6 || orientation.iter().any(|value| !value.is_finite()) {
        return Err("Image Orientation (Slide) must contain six finite direction cosines".into());
    }
    let row_norm = orientation[..3]
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    let column_norm = orientation[3..]
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    let dot = orientation[..3]
        .iter()
        .zip(&orientation[3..])
        .map(|(left, right)| left * right)
        .sum::<f64>();
    if (row_norm - 1.0).abs() > 1e-6 || (column_norm - 1.0).abs() > 1e-6 || dot.abs() > 1e-6 {
        return Err(
            "Image Orientation (Slide) row and column vectors must be unit length and orthogonal"
                .into(),
        );
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(super) struct SlideCoordinateIdentity {
    pub(super) origin: [f64; 3],
    pub(super) orientation: [f64; 6],
}

pub(super) fn slide_coordinate_identity(
    object: &DefaultDicomObject,
) -> Result<SlideCoordinateIdentity, String> {
    validate_slide_coordinate_system_rule(object)?;
    let origin = &sequence_items(
        object,
        tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE,
        "Total Pixel Matrix Origin Sequence",
    )?[0];
    let x = required_finite_float64(
        origin,
        tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
        "X Offset in Slide Coordinate System",
    )?;
    let y = required_finite_float64(
        origin,
        tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
        "Y Offset in Slide Coordinate System",
    )?;
    let z = origin
        .element(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
        .ok()
        .map(|element| {
            element
                .to_float64()
                .map_err(|err| format!("Z Offset in Slide Coordinate System is invalid: {err}"))
        })
        .transpose()?
        .unwrap_or(0.0);
    let values = object
        .element(tags::IMAGE_ORIENTATION_SLIDE)
        .map_err(|_| "Image Orientation (Slide) is missing".to_string())?
        .to_multi_float64()
        .map_err(|err| format!("Image Orientation (Slide) is invalid: {err}"))?;
    let orientation: [f64; 6] = values
        .as_slice()
        .try_into()
        .map_err(|_| "Image Orientation (Slide) does not have six values".to_string())?;
    Ok(SlideCoordinateIdentity {
        origin: [x, y, z],
        orientation,
    })
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

pub(super) fn validate_dimension_rule(object: &DefaultDicomObject) -> Result<(), String> {
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
        let position = PlanePosition::from_frame_functional_group(item, frame)?;
        let (expected_row, expected_column) =
            position.dimension_indices(frame, frame_rows, frame_columns)?;
        if values[row_index] != expected_row || values[column_index] != expected_column {
            return Err(format!(
                "frame {frame} Dimension Index Values do not match its row/column plane position"
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_tile_geometry_rule(object: &DefaultDicomObject) -> Result<(), String> {
    let dimension_type = required_string(
        object,
        tags::DIMENSION_ORGANIZATION_TYPE,
        "Dimension Organization Type",
    )?;
    if dimension_type != "TILED_FULL" {
        return Ok(());
    }

    let frame_count = required_positive_u64(object, tags::NUMBER_OF_FRAMES, "Number of Frames")?;
    let frame_rows = required_positive_u64(object, tags::ROWS, "Rows")?;
    let frame_columns = required_positive_u64(object, tags::COLUMNS, "Columns")?;
    let matrix_rows = required_positive_u64(
        object,
        tags::TOTAL_PIXEL_MATRIX_ROWS,
        "Total Pixel Matrix Rows",
    )?;
    let matrix_columns = required_positive_u64(
        object,
        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
        "Total Pixel Matrix Columns",
    )?;
    let expected_rows = matrix_rows.div_ceil(frame_rows);
    let expected_columns = matrix_columns.div_ceil(frame_columns);
    let expected_frames = expected_rows
        .checked_mul(expected_columns)
        .ok_or_else(|| "TILED_FULL frame count overflows u64".to_string())?;
    if frame_count != expected_frames {
        return Err(format!(
            "TILED_FULL declares {frame_count} frames but matrix geometry requires {expected_frames}"
        ));
    }

    // TILED_FULL encodes positions implicitly in row-major frame order.
    // Explicit positions are optional, but must agree with that same order.
    if object
        .element(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .is_ok()
    {
        let per_frame = sequence_items(
            object,
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            "Per-frame Functional Groups Sequence",
        )?;
        if u64::try_from(per_frame.len()).ok() != Some(frame_count) {
            return Err(format!(
                "Per-frame Functional Groups Sequence has {} items for {frame_count} frames",
                per_frame.len()
            ));
        }
        for (frame, item) in per_frame.iter().enumerate() {
            if item.element(tags::PLANE_POSITION_SLIDE_SEQUENCE).is_err() {
                continue;
            }
            let position = PlanePosition::from_frame_functional_group(item, frame)?;
            let ordinal = u64::try_from(frame).map_err(|_| "frame index exceeds u64")?;
            let expected = PlanePosition::new(
                (ordinal / expected_columns) * frame_rows + 1,
                (ordinal % expected_columns) * frame_columns + 1,
            );
            if position != expected {
                return Err(format!("TILED_FULL frame {frame} position ({}, {}) differs from implicit position ({}, {})", position.row(), position.column(), expected.row(), expected.column()));
            }
        }
    }

    validate_imaged_volume_spacing(object, matrix_rows, matrix_columns)
}

fn validate_imaged_volume_spacing(
    object: &DefaultDicomObject,
    matrix_rows: u64,
    matrix_columns: u64,
) -> Result<(), String> {
    let spacing = PixelSpacingMm::from_shared_functional_groups(object)?;
    let width = required_float64(object, tags::IMAGED_VOLUME_WIDTH, "Imaged Volume Width")?;
    let height = required_float64(object, tags::IMAGED_VOLUME_HEIGHT, "Imaged Volume Height")?;
    let expected_width = spacing.matrix_width(matrix_columns);
    let expected_height = spacing.matrix_height(matrix_rows);
    if !approximately_equal(width, expected_width) || !approximately_equal(height, expected_height)
    {
        return Err(format!(
            "Pixel Spacing and total matrix imply imaged volume {expected_width}x{expected_height} mm, declared {width}x{height} mm"
        ));
    }
    Ok(())
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

fn approximately_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-6_f64.max(right.abs() * 1e-6)
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
