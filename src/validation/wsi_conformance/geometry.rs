use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

use super::fields::{
    required_finite_float64, required_float64, required_positive_u64, required_string,
    sequence_items, PixelSpacingMm, PlanePosition,
};

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

fn approximately_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-6_f64.max(right.abs() * 1e-6)
}
