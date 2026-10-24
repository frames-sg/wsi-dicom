use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

use super::dimensions::validate_dimension_indices;
use super::fields::{required_finite_float64, sequence_items, PixelSpacingMm};
use super::wsi_image::{functional_group_items, integer, number, sequence, string, strings};
use crate::validation::iod::EDITION;

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

pub(super) fn validate_dimension_rule(object: &DefaultDicomObject) -> Result<(), String> {
    let mut violations = validate_dimension_indices(object);
    frame_index_values(object, &mut violations);
    super::violation_result(violations)
}

/// C.7.6.17.1: each Attribute the Dimension Index Sequence references has an
/// index for each Frame. A TILED_FULL image may omit its Frame Content Items
/// with its Per-Frame Functional Groups (C.7.6.17.3); other images place
/// their Frames only through them.
fn frame_index_values(object: &DefaultDicomObject, violations: &mut Vec<String>) {
    if sequence(object, tags::DIMENSION_INDEX_SEQUENCE).is_empty()
        || string(object, tags::DIMENSION_ORGANIZATION_TYPE).as_deref() == Some("TILED_FULL")
    {
        return;
    }
    let frames = integer(object, tags::NUMBER_OF_FRAMES).unwrap_or(0);
    let indexed = sequence(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .iter()
        .filter(|frame| {
            sequence(frame, tags::FRAME_CONTENT_SEQUENCE)
                .first()
                .is_some_and(|content| content.element(tags::DIMENSION_INDEX_VALUES).is_ok())
        })
        .count();
    if (indexed as i64) < frames {
        violations.push(format!(
            "{} of {frames} Frames have Dimension Index Values; outside TILED_FULL each Frame has an index for each Attribute of the Dimension Index Sequence (PS3.3 {EDITION} C.7.6.17.1 and C.7.6.17.3)",
            indexed
        ));
    }
}

pub(super) fn validate_tile_geometry_rule(object: &DefaultDicomObject) -> Result<(), String> {
    let mut violations = Vec::new();
    super::tiled_full::validate_tiled_full(object, &mut violations);
    imaged_volume(object, &mut violations);
    equally_spaced_planes(object, &mut violations);
    super::violation_result(violations)
}

/// C.8.12.4.1.2 and C.8.12.14.1.1: the imaged volume is the Total Pixel
/// Matrix at the Pixel Spacing, within a pixel. Imaged Volume Width and Height
/// are required for VOLUME images and may be present otherwise; LABEL and
/// OVERVIEW images need no Pixel Spacing (C.7.6.16.2.1).
fn imaged_volume(object: &DefaultDicomObject, violations: &mut Vec<String>) {
    let volume = strings(object, tags::IMAGE_TYPE)
        .and_then(|values| values.get(2).cloned())
        .is_none_or(|flavor| flavor == "VOLUME");
    let declared = (
        number(object, tags::IMAGED_VOLUME_WIDTH),
        number(object, tags::IMAGED_VOLUME_HEIGHT),
    );
    let (Some(width), Some(height)) = declared else {
        if volume {
            for (value, name) in [
                (declared.0, "Imaged Volume Width (0048,0001)"),
                (declared.1, "Imaged Volume Height (0048,0002)"),
            ] {
                if value.is_none() {
                    violations.push(format!(
                        "{name} is absent or not a number; it is required for Image Type Value 3 VOLUME (PS3.3 {EDITION} C.8.12.4)"
                    ));
                }
            }
        }
        return;
    };
    let (Some(rows), Some(columns)) = (
        integer(object, tags::TOTAL_PIXEL_MATRIX_ROWS),
        integer(object, tags::TOTAL_PIXEL_MATRIX_COLUMNS),
    ) else {
        return;
    };
    let Ok(spacing) = PixelSpacingMm::from_shared_functional_groups(object) else {
        return; // reported by the IOD and pixel measure checks when required
    };
    let expected = (
        spacing.matrix_width(columns.max(0) as u64),
        spacing.matrix_height(rows.max(0) as u64),
    );
    let within = |declared: f64, expected: f64, pixel: f64| {
        (declared - expected).abs() <= pixel.max(expected.abs() * 1e-6)
    };
    if !within(width, expected.0, spacing.column()) || !within(height, expected.1, spacing.row()) {
        violations.push(format!(
            "Imaged Volume Width and Height are {width}x{height} mm, but the {columns}x{rows} pixel Total Pixel Matrix at Pixel Spacing {}\\{} mm is {}x{} mm (PS3.3 {EDITION} C.8.12.4.1.2 and C.8.12.14.1.1)",
            spacing.row(),
            spacing.column(),
            expected.0,
            expected.1
        ));
    }
}

/// C.7.6.17-1: a 3D image is a set of equally spaced parallel planes, so its
/// Frames' Z offsets step evenly, by Spacing Between Slices when present.
fn equally_spaced_planes(object: &DefaultDicomObject, violations: &mut Vec<String>) {
    if string(object, tags::DIMENSION_ORGANIZATION_TYPE).as_deref() != Some("3D") {
        return;
    }
    let mut depths: Vec<f64> = functional_group_items(object)
        .into_iter()
        .flat_map(|(_, item)| sequence(item, tags::PLANE_POSITION_SLIDE_SEQUENCE))
        .filter_map(|position| number(position, tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM))
        .collect();
    depths.sort_by(f64::total_cmp);
    depths.dedup_by(|later, earlier| (*later - *earlier).abs() <= 1e-6);
    let steps: Vec<f64> = depths.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let Some(first) = steps.first().copied() else {
        return;
    };
    let tolerance = first.abs() * 1e-3 + 1e-6;
    if let Some(step) = steps.iter().find(|step| (**step - first).abs() > tolerance) {
        violations.push(format!(
            "Dimension Organization Type is 3D, a set of equally spaced parallel planes, but its focal planes are {first} µm and {step} µm apart (PS3.3 {EDITION} C.7.6.17 Table C.7.6.17-1)"
        ));
        return;
    }
    let spacing = functional_group_items(object)
        .into_iter()
        .flat_map(|(_, item)| sequence(item, tags::PIXEL_MEASURES_SEQUENCE))
        .find_map(|measures| number(measures, tags::SPACING_BETWEEN_SLICES));
    if let Some(spacing) = spacing {
        if (spacing * 1000.0 - first).abs() > tolerance {
            violations.push(format!(
                "Spacing Between Slices (0018,0088) is {spacing} mm, but the 3D focal planes are {first} µm apart (PS3.3 {EDITION} C.7.6.16.2.1)"
            ));
        }
    }
}
