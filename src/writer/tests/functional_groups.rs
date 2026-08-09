use super::*;

#[test]
fn per_frame_functional_groups_stream_with_an_exact_budget() {
    let plan = super::PerFrameFunctionalGroupsPlan::new(
        100_000,
        super::FrameGrid {
            frame_columns: 1,
            frame_rows: 1,
            matrix_columns: 100_000,
            matrix_rows: 1,
        },
        0.0005,
        0.0005,
    )
    .unwrap();
    let estimate = plan.encoded_len().unwrap();

    let mut rejected = Vec::new();
    let error = plan.write_to(&mut rejected, estimate - 1).unwrap_err();
    assert!(error.to_string().contains("metadata"));
    assert!(rejected.is_empty(), "preflight must precede output");

    let written = plan.write_to(&mut std::io::sink(), estimate).unwrap();
    assert_eq!(written, estimate);
}

#[test]
fn per_frame_metadata_rejects_an_impossible_plan_from_its_structural_lower_bound() {
    let plan = super::PerFrameFunctionalGroupsPlan::new(
        u32::MAX,
        super::FrameGrid {
            frame_columns: 1,
            frame_rows: 1,
            matrix_columns: u64::from(u32::MAX),
            matrix_rows: 1,
        },
        0.0005,
        0.0005,
    )
    .unwrap();
    let budget = 256 * 1024 * 1024;

    assert!(plan.minimum_encoded_len().unwrap() > budget);
    let error = plan.encoded_len_with_limit(budget).unwrap_err();

    assert!(matches!(error, Error::InvalidOptions { .. }));
    assert!(error.to_string().contains("metadata"));
}

#[test]
fn decimal_string_formatting_stays_within_dicom_limit() {
    assert_eq!(format_ds(0.0002528), "0.0002528");
    assert!(format_ds(123_456.789_123_456).len() <= 16);
}

#[test]
fn pyramid_resampled_level_metadata_is_grouped_and_labeled() {
    let object = sample_object(1);

    assert_eq!(
        object
            .element(tags::IMAGE_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED"
    );
    assert_eq!(
        object
            .element(tags::PYRAMID_UID)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "1.2.826.0.1.3680043.10.999.5"
    );
    assert_eq!(
        object
            .element(tags::PYRAMID_LABEL)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "WSI pyramid s0 ser0 z0 c0 t0"
    );
    assert_eq!(
        object
            .element(tags::SERIES_NUMBER)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        7
    );
    assert_eq!(
        object
            .element(tags::INSTANCE_NUMBER)
            .unwrap()
            .to_int::<u32>()
            .unwrap(),
        42
    );
    assert_eq!(
        object
            .element(tags::ACQUISITION_DATE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "19700101"
    );
    assert_eq!(
        object
            .element(tags::ACQUISITION_TIME)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "000000"
    );
    assert!(object.element(tags::PIXEL_SPACING).is_err());
}

#[test]
fn vl_wsi_multiframe_metadata_contains_required_shared_and_dimension_sequences() {
    let object = sample_object(0);
    let image_type = object
        .element(tags::IMAGE_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .into_owned();

    let shared = sequence_items(&object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE);
    assert_eq!(shared.len(), 1);
    let pixel_measures = sequence_items(&shared[0], tags::PIXEL_MEASURES_SEQUENCE);
    assert_eq!(pixel_measures.len(), 1);
    assert_eq!(
        pixel_measures[0]
            .element(tags::PIXEL_SPACING)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "0.0005\\0.0005"
    );
    assert_eq!(
        pixel_measures[0]
            .element(tags::SLICE_THICKNESS)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "0.001"
    );
    let frame_type = sequence_items(
        &shared[0],
        tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
    );
    assert_eq!(frame_type.len(), 1);
    assert_eq!(
        frame_type[0]
            .element(tags::FRAME_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        image_type.as_str()
    );

    let dimension_organization = sequence_items(&object, tags::DIMENSION_ORGANIZATION_SEQUENCE);
    assert_eq!(dimension_organization.len(), 1);
    let dimension_uid = dimension_organization[0]
        .element(tags::DIMENSION_ORGANIZATION_UID)
        .unwrap()
        .to_str()
        .unwrap();

    let dimension_index = sequence_items(&object, tags::DIMENSION_INDEX_SEQUENCE);
    assert_eq!(dimension_index.len(), 2);
    assert_dimension_index_item(
        &dimension_index[0],
        tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
        dimension_uid.as_ref(),
    );
    assert_dimension_index_item(
        &dimension_index[1],
        tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
        dimension_uid.as_ref(),
    );

    let per_frame = sequence_items(&object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
    assert_eq!(per_frame.len(), 6);
    let frame_content = sequence_items(&per_frame[0], tags::FRAME_CONTENT_SEQUENCE);
    assert_eq!(frame_content.len(), 1);
    assert_eq!(
        frame_content[0]
            .element(tags::DIMENSION_INDEX_VALUES)
            .unwrap()
            .to_multi_int::<u32>()
            .unwrap(),
        vec![1, 1]
    );
    let second_frame_content = sequence_items(&per_frame[1], tags::FRAME_CONTENT_SEQUENCE);
    assert_eq!(
        second_frame_content[0]
            .element(tags::DIMENSION_INDEX_VALUES)
            .unwrap()
            .to_multi_int::<u32>()
            .unwrap(),
        vec![1, 2]
    );
    let third_frame_content = sequence_items(&per_frame[2], tags::FRAME_CONTENT_SEQUENCE);
    assert_eq!(
        third_frame_content[0]
            .element(tags::DIMENSION_INDEX_VALUES)
            .unwrap()
            .to_multi_int::<u32>()
            .unwrap(),
        vec![2, 1]
    );
    let frame_position = sequence_items(&per_frame[5], tags::PLANE_POSITION_SLIDE_SEQUENCE);
    assert_eq!(frame_position.len(), 1);
    assert_eq!(
        frame_position[0]
            .element(tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
            .unwrap()
            .to_int::<i32>()
            .unwrap(),
        513
    );
    assert_eq!(
        frame_position[0]
            .element(tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
            .unwrap()
            .to_int::<i32>()
            .unwrap(),
        1025
    );
    assert_eq!(
        frame_position[0]
            .element(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "0.256"
    );
    assert_eq!(
        frame_position[0]
            .element(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "0.512"
    );
    assert_eq!(
        frame_position[0]
            .element(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "0"
    );
}

#[test]
fn dimension_index_values_match_three_column_row_major_frame_order() {
    let items = super::functional_groups::per_frame_items(
        4,
        super::FrameGrid {
            frame_columns: 16,
            frame_rows: 16,
            matrix_columns: 48,
            matrix_rows: 32,
        },
        0.0005,
        0.0005,
    )
    .unwrap();
    let values = items
        .iter()
        .map(|item| {
            sequence_items(item, tags::FRAME_CONTENT_SEQUENCE)[0]
                .element(tags::DIMENSION_INDEX_VALUES)
                .unwrap()
                .to_multi_int::<u32>()
                .unwrap()
        })
        .collect::<Vec<_>>();

    assert_eq!(values, [[1, 1], [1, 2], [1, 3], [2, 1]]);
}

#[test]
fn vl_wsi_monochrome2_writes_identity_presentation_transform() {
    let metadata = DicomMetadata::research_placeholder();
    let mut params = sample_dicom_object_params(&metadata, None);
    params.profile = PixelProfile {
        components: 1,
        bits_allocated: 8,
        photometric_interpretation: "MONOCHROME2",
    };

    let object = super::build_dicom_object(params).unwrap();

    assert_eq!(tag_str(&object, tags::PRESENTATION_LUT_SHAPE), "IDENTITY");
    assert_eq!(tag_str(&object, tags::RESCALE_INTERCEPT), "0");
    assert_eq!(tag_str(&object, tags::RESCALE_SLOPE), "1");
    let optical_path = sequence_items(&object, tags::OPTICAL_PATH_SEQUENCE);
    assert!(optical_path[0].element(tags::ICC_PROFILE).is_err());
}

#[test]
fn vl_wsi_color_omits_monochrome_presentation_transform() {
    let object = sample_object(0);

    assert!(object.element(tags::PRESENTATION_LUT_SHAPE).is_err());
    assert!(object.element(tags::RESCALE_INTERCEPT).is_err());
    assert!(object.element(tags::RESCALE_SLOPE).is_err());
}
