use super::*;

#[test]
fn vl_wsi_research_placeholder_contains_required_conformance_metadata() {
    let object = sample_object_with_metadata(DicomMetadata::research_placeholder());

    assert_eq!(tag_str(&object, tags::PATIENT_BIRTH_DATE), "");
    assert_eq!(tag_str(&object, tags::PATIENT_SEX), "");
    assert_eq!(tag_str(&object, tags::STUDY_DATE), "19700101");
    assert_eq!(tag_str(&object, tags::STUDY_TIME), "000000");
    assert_eq!(tag_str(&object, tags::REFERRING_PHYSICIAN_NAME), "");
    assert!(object.element(tags::LATERALITY).is_err());
    assert_eq!(
        tag_str(&object, tags::POSITION_REFERENCE_INDICATOR),
        "SLIDE_CORNER"
    );
    assert_eq!(tag_str(&object, tags::MANUFACTURER), "wsi-dicom");
    assert_eq!(tag_str(&object, tags::MANUFACTURER_MODEL_NAME), "wsi-dicom");
    assert_eq!(tag_str(&object, tags::DEVICE_SERIAL_NUMBER), "RESEARCH");
    assert_eq!(
        tag_str(&object, tags::SOFTWARE_VERSIONS),
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(tag_str(&object, tags::CONTENT_DATE), "19700101");
    assert_eq!(tag_str(&object, tags::CONTENT_TIME), "000000");
    assert_eq!(
        tag_str(&object, tags::ACQUISITION_DATE_TIME),
        "19700101000000"
    );
    assert_eq!(
        tag_str(&object, tags::CONTAINER_IDENTIFIER),
        "RESEARCH-CONTAINER"
    );
    assert_eq!(tag_str(&object, tags::VOLUMETRIC_PROPERTIES), "VOLUME");
    assert_eq!(tag_str(&object, tags::BURNED_IN_ANNOTATION), "NO");
    assert_eq!(tag_str(&object, tags::FOCUS_METHOD), "AUTO");
    assert_eq!(tag_str(&object, tags::EXTENDED_DEPTH_OF_FIELD), "NO");
    assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_WIDTH), "0.512");
    assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_HEIGHT), "0.768");
    assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_DEPTH), "1");

    assert_eq!(
        sequence_items(&object, tags::ACQUISITION_CONTEXT_SEQUENCE).len(),
        0
    );
    assert_eq!(
        sequence_items(&object, tags::ISSUER_OF_THE_CONTAINER_IDENTIFIER_SEQUENCE).len(),
        0
    );
    let container_type = sequence_items(&object, tags::CONTAINER_TYPE_CODE_SEQUENCE);
    assert_eq!(container_type.len(), 1);
    assert_code_item(&container_type[0], "433466003", "SCT", "Microscope slide");

    let specimen = sequence_items(&object, tags::SPECIMEN_DESCRIPTION_SEQUENCE);
    assert_eq!(specimen.len(), 1);
    assert_eq!(
        tag_str(&specimen[0], tags::SPECIMEN_IDENTIFIER),
        "RESEARCH-SPECIMEN"
    );
    assert!(!tag_str(&specimen[0], tags::SPECIMEN_UID).is_empty());
    assert_eq!(
        tag_str(&specimen[0], tags::SPECIMEN_SHORT_DESCRIPTION),
        "Research placeholder specimen"
    );
    assert_eq!(
        tag_str(&specimen[0], tags::SPECIMEN_DETAILED_DESCRIPTION),
        "Research placeholder specimen"
    );

    let optical_path = sequence_items(&object, tags::OPTICAL_PATH_SEQUENCE);
    assert_eq!(optical_path.len(), 1);
    let illumination_type = sequence_items(&optical_path[0], tags::ILLUMINATION_TYPE_CODE_SEQUENCE);
    assert_eq!(illumination_type.len(), 1);
    assert_code_item(
        &illumination_type[0],
        "111744",
        "DCM",
        "Brightfield illumination",
    );
    let illumination_color =
        sequence_items(&optical_path[0], tags::ILLUMINATION_COLOR_CODE_SEQUENCE);
    assert_eq!(illumination_color.len(), 1);
    assert_code_item(&illumination_color[0], "371251000", "SCT", "White");
    assert!(optical_path[0].element(tags::ICC_PROFILE).is_ok());
}

#[test]
fn vl_wsi_strict_metadata_overrides_conformance_defaults() {
    let metadata: DicomMetadata = serde_json::from_value(serde_json::json!({
        "patient_name": "REAL^PATIENT",
        "patient_id": "P-123",
        "patient_birth_date": "19650504",
        "patient_sex": "F",
        "study_date": "20260504",
        "study_time": "142233",
        "referring_physician_name": "REFERRING^DOC",
        "laterality": "L",
        "manufacturer": "ScannerCo",
        "manufacturer_model_name": "Model X",
        "device_serial_number": "SN123",
        "software_versions": "9.8.7",
        "content_date": "20260504",
        "content_time": "142300",
        "acquisition_date_time": "20260504142233",
        "container_identifier": "SLIDE-123",
        "specimen_identifier": "SPEC-123",
        "specimen_uid": "1.2.826.0.1.3680043.10.999.702",
        "specimen_identifier_issuer": {
            "local_namespace_entity_id": "HOSPITAL-A",
            "universal_entity_id": "https://hospital-a.example/specimens",
            "universal_entity_id_type": "URI"
        },
        "specimen_description": "H&E section",
        "imaged_volume_depth_mm": 0.004,
        "focus_method": "MANUAL"
    }))
    .unwrap();
    let object = sample_object_with_metadata(metadata);

    assert_eq!(tag_str(&object, tags::PATIENT_BIRTH_DATE), "19650504");
    assert_eq!(tag_str(&object, tags::PATIENT_SEX), "F");
    assert_eq!(tag_str(&object, tags::STUDY_DATE), "20260504");
    assert_eq!(tag_str(&object, tags::STUDY_TIME), "142233");
    let specimen = sequence_items(&object, tags::SPECIMEN_DESCRIPTION_SEQUENCE);
    assert_eq!(
        tag_str(&specimen[0], tags::SPECIMEN_UID),
        "1.2.826.0.1.3680043.10.999.702"
    );
    let issuer = sequence_items(
        &specimen[0],
        tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE,
    );
    assert_eq!(issuer.len(), 1);
    assert_eq!(
        tag_str(&issuer[0], tags::LOCAL_NAMESPACE_ENTITY_ID),
        "HOSPITAL-A"
    );
    assert_eq!(
        tag_str(&issuer[0], tags::UNIVERSAL_ENTITY_ID),
        "https://hospital-a.example/specimens"
    );
    assert_eq!(tag_str(&issuer[0], tags::UNIVERSAL_ENTITY_ID_TYPE), "URI");
    assert_eq!(
        tag_str(&object, tags::REFERRING_PHYSICIAN_NAME),
        "REFERRING^DOC"
    );
    assert_eq!(tag_str(&object, tags::LATERALITY), "L");
    assert_eq!(tag_str(&object, tags::MANUFACTURER), "ScannerCo");
    assert_eq!(tag_str(&object, tags::MANUFACTURER_MODEL_NAME), "Model X");
    assert_eq!(tag_str(&object, tags::DEVICE_SERIAL_NUMBER), "SN123");
    assert_eq!(tag_str(&object, tags::SOFTWARE_VERSIONS), "9.8.7");
    assert_eq!(tag_str(&object, tags::CONTENT_DATE), "20260504");
    assert_eq!(tag_str(&object, tags::CONTENT_TIME), "142300");
    assert_eq!(
        tag_str(&object, tags::ACQUISITION_DATE_TIME),
        "20260504142233"
    );
    assert_eq!(tag_str(&object, tags::CONTAINER_IDENTIFIER), "SLIDE-123");
    assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_DEPTH), "4");
    assert_eq!(tag_str(&object, tags::FOCUS_METHOD), "MANUAL");

    let specimen = sequence_items(&object, tags::SPECIMEN_DESCRIPTION_SEQUENCE);
    assert_eq!(tag_str(&specimen[0], tags::SPECIMEN_IDENTIFIER), "SPEC-123");
    assert_eq!(
        tag_str(&specimen[0], tags::SPECIMEN_SHORT_DESCRIPTION),
        "H&E section"
    );
}

#[test]
fn vl_wsi_declares_utf8_only_when_caller_metadata_requires_it() {
    let ascii = sample_object_with_metadata(DicomMetadata::research_placeholder());
    assert!(ascii.element(tags::SPECIFIC_CHARACTER_SET).is_err());

    let mut unicode = DicomMetadata::research_placeholder();
    unicode.patient_name = Some("山田^太郎".to_string());
    unicode.study_description = Some("Cafe\u{301} 病理".to_string());
    let unicode = sample_object_with_metadata(unicode);
    assert_eq!(
        tag_str(&unicode, tags::SPECIFIC_CHARACTER_SET),
        "ISO_IR 192"
    );
    assert_eq!(tag_str(&unicode, tags::PATIENT_NAME), "山田^太郎");
    assert_eq!(
        tag_str(&unicode, tags::STUDY_DESCRIPTION),
        "Cafe\u{301} 病理"
    );
}

#[test]
fn vl_wsi_object_contains_tiled_full_origin_orientation_and_representative_frame() {
    let object = sample_object(0);

    let origin = sequence_items(&object, tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE);
    assert_eq!(origin.len(), 1);
    assert_eq!(
        origin[0]
            .element(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "0"
    );
    assert_eq!(
        origin[0]
            .element(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "0"
    );
    assert_eq!(
        object
            .element(tags::IMAGE_ORIENTATION_SLIDE)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "1\\0\\0\\0\\1\\0"
    );
    assert_eq!(
        object
            .element(tags::REPRESENTATIVE_FRAME_NUMBER)
            .unwrap()
            .to_int::<u16>()
            .unwrap(),
        1
    );
    assert_eq!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "00"
    );
}

#[test]
fn vl_wsi_volume_requires_pixel_spacing_for_pixel_measures() {
    let metadata = DicomMetadata::research_placeholder();
    let mut params = sample_dicom_object_params(&metadata, None);
    params.pixel_spacing_mm = None;
    let err = super::build_dicom_object(params).unwrap_err();

    assert!(
        err.to_string().contains("pixel spacing"),
        "unexpected error: {err}"
    );
}

#[test]
fn vl_wsi_writes_ordered_multi_stage_lossy_history() {
    let metadata = DicomMetadata::research_placeholder();
    let icc_profile = synthetic_srgb_icc_profile().unwrap();
    let mut params = sample_dicom_object_params(&metadata, Some(&icc_profile));
    params
        .lossy_compression
        .push_declared("ISO_10918_1", 4.5)
        .unwrap();
    params
        .lossy_compression
        .push_declared("ISO_15444_15", 2.25)
        .unwrap();

    let object = super::build_dicom_object(params).unwrap();

    assert_eq!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION_METHOD)
            .unwrap()
            .to_multi_str()
            .unwrap()
            .as_ref(),
        ["ISO_10918_1", "ISO_15444_15"]
    );
    assert_eq!(
        object
            .element(tags::LOSSY_IMAGE_COMPRESSION_RATIO)
            .unwrap()
            .to_multi_float64()
            .unwrap(),
        vec![4.5, 2.25]
    );
}
