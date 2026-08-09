use super::*;

#[test]
fn vl_wsi_rejects_dimensions_that_exceed_dicom_attribute_ranges() {
    let rows_err =
        sample_object_with_dimensions(512, u32::from(u16::MAX) + 1, 512, 512).unwrap_err();
    assert!(
        rows_err.to_string().contains("Rows exceeds US range"),
        "unexpected error: {rows_err}"
    );

    let columns_err =
        sample_object_with_dimensions(u32::from(u16::MAX) + 1, 512, 512, 512).unwrap_err();
    assert!(
        columns_err.to_string().contains("Columns exceeds US range"),
        "unexpected error: {columns_err}"
    );

    let matrix_columns_err =
        sample_object_with_dimensions(512, 512, u64::from(u32::MAX) + 1, 512).unwrap_err();
    assert!(
        matrix_columns_err
            .to_string()
            .contains("Total Pixel Matrix Columns exceeds UL range"),
        "unexpected error: {matrix_columns_err}"
    );

    let matrix_rows_err =
        sample_object_with_dimensions(512, 512, 512, u64::from(u32::MAX) + 1).unwrap_err();
    assert!(
        matrix_rows_err
            .to_string()
            .contains("Total Pixel Matrix Rows exceeds UL range"),
        "unexpected error: {matrix_rows_err}"
    );
}

#[test]
fn vl_wsi_rectangular_frames_write_rows_columns_and_positions() {
    let icc_profile = synthetic_srgb_icc_profile().unwrap();
    let metadata = DicomMetadata::research_placeholder();
    let mut params = sample_dicom_object_params(&metadata, Some(&icc_profile));
    params.frame_grid = super::FrameGrid {
        frame_columns: 64,
        frame_rows: 8,
        matrix_columns: 130,
        matrix_rows: 31,
    };
    params.frame_count = 12;
    params.profile = PixelProfile {
        components: 3,
        bits_allocated: 8,
        photometric_interpretation: "YBR_FULL_422",
    };
    params.pixel_spacing_mm = Some((0.0005, 0.00025));
    let object = super::build_dicom_object(params).unwrap();

    assert_eq!(
        object.element(tags::ROWS).unwrap().to_int::<u16>().unwrap(),
        8
    );
    assert_eq!(
        object
            .element(tags::COLUMNS)
            .unwrap()
            .to_int::<u16>()
            .unwrap(),
        64
    );
    let per_frame = sequence_items(&object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
    assert_eq!(per_frame.len(), 12);
    let frame_4_position = sequence_items(&per_frame[4], tags::PLANE_POSITION_SLIDE_SEQUENCE);
    assert_eq!(
        frame_4_position[0]
            .element(tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
            .unwrap()
            .to_int::<i32>()
            .unwrap(),
        65
    );
    assert_eq!(
        frame_4_position[0]
            .element(tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
            .unwrap()
            .to_int::<i32>()
            .unwrap(),
        9
    );
}

#[test]
fn vl_wsi_rejects_per_frame_positions_outside_sl_range() {
    let metadata = DicomMetadata::research_placeholder();
    let mut params = sample_dicom_object_params(&metadata, None);
    params.frame_grid = super::FrameGrid {
        frame_columns: u16::MAX as u32,
        frame_rows: 1,
        matrix_columns: 2_147_516_416,
        matrix_rows: 1,
    };
    params.frame_count = 32_770;
    params.profile = PixelProfile {
        components: 3,
        bits_allocated: 8,
        photometric_interpretation: "YBR_FULL_422",
    };
    params.pixel_spacing_mm = Some((0.0005, 0.00025));
    let err = super::build_dicom_object(params).unwrap_err();

    assert!(
        err.to_string().contains("position exceeds SL range"),
        "unexpected error: {err}"
    );
}

#[test]
fn per_frame_items_reject_invalid_frame_grid_values() {
    let err = super::per_frame_items(
        1,
        super::FrameGrid {
            frame_columns: 0,
            frame_rows: 512,
            matrix_columns: 512,
            matrix_rows: 512,
        },
        0.00025,
        0.00025,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("non-zero frame dimensions"),
        "unexpected error: {err}"
    );

    let err = super::checked_dimension_index_value(u64::from(u32::MAX), "column")
        .expect_err("one-based dimension index should exceed UL range");
    assert!(
        err.to_string()
            .contains("column dimension index exceeds UL range"),
        "unexpected error: {err}"
    );
}
