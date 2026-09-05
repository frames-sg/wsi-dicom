use super::*;

#[test]
fn intrinsic_wsi_icc_rule_fails_even_when_external_validators_pass() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "invalid-icc");
    let mut object = dicom_object::open_file(&path).unwrap();
    let mut optical_path_sequence = object.take(tags::OPTICAL_PATH_SEQUENCE).unwrap();
    let optical_paths = optical_path_sequence.items_mut().unwrap();
    let mut profile = optical_paths[0]
        .element(tags::ICC_PROFILE)
        .unwrap()
        .to_bytes()
        .unwrap()
        .into_owned();
    profile[12..16].copy_from_slice(b"mntr");
    optical_paths[0].put(DataElement::new(
        tags::ICC_PROFILE,
        VR::OB,
        PrimitiveValue::from(profile),
    ));
    object.put(optical_path_sequence);
    object.write_to_file(&path).unwrap();

    let runner = FakeRunner::default()
        .with_command("dciodvfy")
        .with_command("dcentvfy")
        .with_command("validate_iods");
    let report = validate_dicom_path_with_runner(
        &path,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &runner,
    )
    .unwrap();

    assert!(report
        .checks
        .iter()
        .any(|check| { check.name == "dciodvfy" && check.status == ValidationStatus::Passed }));
    assert!(report.checks.iter().any(|check| {
        check.name == "intrinsic-wsi-dicom-2026c-icc-profile"
            && check.status == ValidationStatus::Failed
    }));
    assert!(report.has_failures());
}

#[test]
fn corrected_producer_output_passes_all_versioned_intrinsic_wsi_rules() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "valid");

    let report = validate_dicom_path_with_runner(
        &path,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();

    for name in [
        "intrinsic-wsi-dicom-2026c-icc-profile",
        "intrinsic-wsi-dicom-2026c-monochrome-presentation",
        "intrinsic-wsi-dicom-2026c-lossy-history",
        "intrinsic-wsi-dicom-2026c-specimen-identity",
        "intrinsic-wsi-dicom-2026c-specimen-container",
        "intrinsic-wsi-dicom-2026c-dimension-order",
        "intrinsic-wsi-dicom-2026c-slide-coordinate-system",
        "intrinsic-wsi-dicom-2026c-optical-path-structure",
        "intrinsic-wsi-dicom-2026c-core-image-profile",
        "intrinsic-wsi-dicom-2026c-specimen-uid-set",
        "intrinsic-wsi-dicom-2026c-tile-geometry",
        "intrinsic-wsi-dicom-2026c-identity-set",
        "intrinsic-wsi-dicom-2026c-clinical-identity-set",
        "intrinsic-wsi-dicom-2026c-image-metadata",
    ] {
        assert!(
            report
                .checks
                .iter()
                .any(|check| { check.name == name && check.status == ValidationStatus::Passed }),
            "missing passing intrinsic check {name}"
        );
    }
    for name in [
        "intrinsic-wsi-dicom-2026c-pyramid-geometry",
        "intrinsic-wsi-dicom-2026c-source-relationship",
    ] {
        assert!(
            report.checks.iter().all(|check| check.name != name),
            "non-applicable corpus rule {name} must not be reported as passed"
        );
    }
    assert!(!report.has_failures());
}

#[test]
fn core_image_profile_rule_rejects_missing_acquisition_datetime() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "missing-acquisition-datetime");
    let mut object = dicom_object::open_file(&path).unwrap();
    object.take(tags::ACQUISITION_DATE_TIME).unwrap();
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-core-image-profile");
}

#[test]
fn core_image_profile_rule_rejects_missing_type_two_acquisition_context() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "missing-acquisition-context");
    let mut object = dicom_object::open_file(&path).unwrap();
    object.take(tags::ACQUISITION_CONTEXT_SEQUENCE).unwrap();
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-core-image-profile");
}

#[test]
fn core_image_profile_rule_rejects_missing_enhanced_equipment_identity() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "missing-manufacturer");
    let mut object = dicom_object::open_file(&path).unwrap();
    object.take(tags::MANUFACTURER).unwrap();
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-core-image-profile");
}

#[test]
fn slide_coordinate_system_rule_rejects_missing_origin() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "missing-origin");
    let mut object = dicom_object::open_file(&path).unwrap();
    object
        .take(tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE)
        .unwrap();
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-slide-coordinate-system");
}

#[test]
fn optical_path_structure_rule_rejects_count_mismatch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "optical-path-count");
    let mut object = dicom_object::open_file(&path).unwrap();
    object.put(DataElement::new(
        tags::NUMBER_OF_OPTICAL_PATHS,
        VR::UL,
        PrimitiveValue::from(2_u32),
    ));
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-optical-path-structure");
}

#[test]
fn specimen_container_rule_rejects_missing_type_two_preparation_sequence() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "missing-preparation");
    let mut object = dicom_object::open_file(&path).unwrap();
    let mut descriptions = object.take(tags::SPECIMEN_DESCRIPTION_SEQUENCE).unwrap();
    descriptions.items_mut().unwrap()[0]
        .take(tags::SPECIMEN_PREPARATION_SEQUENCE)
        .unwrap();
    object.put(descriptions);
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-specimen-container");
}

#[test]
fn clinical_identity_set_rule_rejects_patient_conflict_within_study() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let first_source = export_valid_color_wsi_for_validation(tmp.path(), "patient-first");
    let second_source = export_valid_color_wsi_for_validation(tmp.path(), "patient-second");
    let set_dir = tmp.path().join("patient-set");
    std::fs::create_dir(&set_dir).unwrap();
    let first = set_dir.join("first.dcm");
    let second = set_dir.join("second.dcm");
    std::fs::copy(first_source, &first).unwrap();
    std::fs::copy(second_source, &second).unwrap();

    let first_object = dicom_object::open_file(&first).unwrap();
    let study_uid = first_object
        .element(tags::STUDY_INSTANCE_UID)
        .unwrap()
        .to_str()
        .unwrap()
        .into_owned();
    let mut object = dicom_object::open_file(&second).unwrap();
    object.put(DataElement::new(
        tags::STUDY_INSTANCE_UID,
        VR::UI,
        study_uid,
    ));
    object.put(DataElement::new(
        tags::PATIENT_ID,
        VR::LO,
        "CONFLICTING-PATIENT",
    ));
    object.write_to_file(&second).unwrap();

    let report = validate_dicom_path_with_runner(
        &set_dir,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();

    assert!(report.checks.iter().any(|check| {
        check.name == "intrinsic-wsi-dicom-2026c-clinical-identity-set"
            && check.status == ValidationStatus::Failed
    }));
}

#[test]
fn clinical_identity_set_rule_rejects_missing_patient_type_two_attribute() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "missing-birth-date");
    let mut object = dicom_object::open_file(&path).unwrap();
    object.take(tags::PATIENT_BIRTH_DATE).unwrap();
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-clinical-identity-set");
}

#[test]
fn tile_geometry_rule_rejects_duplicate_tiled_full_position() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "duplicate-tile", 4, 4, 2);
    let mut object = dicom_object::open_file(&path).unwrap();
    let mut per_frame = object
        .take(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap();
    let items = per_frame.items_mut().unwrap();
    let first_position = items[0]
        .element(tags::PLANE_POSITION_SLIDE_SEQUENCE)
        .unwrap()
        .clone();
    let first_content = items[0]
        .element(tags::FRAME_CONTENT_SEQUENCE)
        .unwrap()
        .clone();
    items[1].put(first_position);
    items[1].put(first_content);
    object.put(per_frame);
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-tile-geometry");
}

#[test]
fn identity_set_rule_rejects_file_meta_and_dataset_sop_mismatch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "sop-mismatch");
    let mut object = dicom_object::open_file(&path).unwrap();
    object.put(DataElement::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        "1.2.826.0.1.3680043.10.999.399",
    ));
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-identity-set");
}

#[test]
fn image_metadata_rule_rejects_samples_and_photometric_contradiction() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "samples-mismatch");
    let mut object = dicom_object::open_file(&path).unwrap();
    object.put(DataElement::new(
        tags::SAMPLES_PER_PIXEL,
        VR::US,
        PrimitiveValue::from(1_u16),
    ));
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-image-metadata");
}

#[test]
fn versioned_intrinsic_wsi_rules_reject_targeted_metadata_mutations() {
    let tmp = tempfile::tempdir().expect("tempdir");

    let monochrome = export_valid_color_wsi_for_validation(tmp.path(), "monochrome");
    let mut object = dicom_object::open_file(&monochrome).unwrap();
    object.put(DataElement::new(
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        "MONOCHROME2",
    ));
    object.write_to_file(&monochrome).unwrap();
    assert_intrinsic_rule_fails(
        &monochrome,
        "intrinsic-wsi-dicom-2026c-monochrome-presentation",
    );

    let lossy = export_valid_color_wsi_for_validation(tmp.path(), "lossy");
    let mut object = dicom_object::open_file(&lossy).unwrap();
    object.put(DataElement::new(
        tags::LOSSY_IMAGE_COMPRESSION,
        VR::CS,
        "01",
    ));
    object.write_to_file(&lossy).unwrap();
    assert_intrinsic_rule_fails(&lossy, "intrinsic-wsi-dicom-2026c-lossy-history");

    let specimen = export_valid_color_wsi_for_validation(tmp.path(), "specimen");
    let mut object = dicom_object::open_file(&specimen).unwrap();
    let mut sequence = object.take(tags::SPECIMEN_DESCRIPTION_SEQUENCE).unwrap();
    sequence.items_mut().unwrap()[0].put(DataElement::new(
        tags::SPECIMEN_UID,
        VR::UI,
        "invalid-uid",
    ));
    object.put(sequence);
    object.write_to_file(&specimen).unwrap();
    assert_intrinsic_rule_fails(&specimen, "intrinsic-wsi-dicom-2026c-specimen-identity");

    let dimension = export_valid_color_wsi_for_validation(tmp.path(), "dimension");
    let mut object = dicom_object::open_file(&dimension).unwrap();
    let mut sequence = object.take(tags::DIMENSION_INDEX_SEQUENCE).unwrap();
    sequence.items_mut().unwrap().swap(0, 1);
    object.put(sequence);
    object.write_to_file(&dimension).unwrap();
    assert_intrinsic_rule_fails(&dimension, "intrinsic-wsi-dicom-2026c-dimension-order");
}

#[test]
fn specimen_uid_set_rule_rejects_conflicting_identifier_scope() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let first = export_valid_color_wsi_for_validation(tmp.path(), "set-source");
    let set_dir = tmp.path().join("set");
    std::fs::create_dir(&set_dir).unwrap();
    let first_copy = set_dir.join("first.dcm");
    let second_copy = set_dir.join("second.dcm");
    std::fs::copy(&first, &first_copy).unwrap();
    std::fs::copy(&first, &second_copy).unwrap();

    let mut second = dicom_object::open_file(&second_copy).unwrap();
    let mut sequence = second.take(tags::SPECIMEN_DESCRIPTION_SEQUENCE).unwrap();
    sequence.items_mut().unwrap()[0].put(DataElement::new(
        tags::SPECIMEN_IDENTIFIER,
        VR::LO,
        "CONFLICTING-SPECIMEN",
    ));
    second.put(sequence);
    second.write_to_file(&second_copy).unwrap();

    let report = validate_dicom_path_with_runner(
        &set_dir,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();

    assert!(report.checks.iter().any(|check| {
        check.name == "intrinsic-wsi-dicom-2026c-specimen-uid-set"
            && check.status == ValidationStatus::Failed
    }));
}

#[test]
fn pyramid_geometry_rule_rejects_inconsistent_physical_extent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source = export_valid_color_wsi_grid_for_validation(tmp.path(), "pyramid", 4, 4, 2);
    let set_dir = tmp.path().join("pyramid-set");
    std::fs::create_dir(&set_dir).unwrap();
    let first = set_dir.join("level-0.dcm");
    let second = set_dir.join("level-1.dcm");
    std::fs::copy(&source, &first).unwrap();
    std::fs::copy(&source, &second).unwrap();

    let mut object = dicom_object::open_file(&second).unwrap();
    let mut shared = object
        .take(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap();
    let shared_item = &mut shared.items_mut().unwrap()[0];
    let mut pixel_measures = shared_item.take(tags::PIXEL_MEASURES_SEQUENCE).unwrap();
    pixel_measures.items_mut().unwrap()[0].put(DataElement::new(
        tags::PIXEL_SPACING,
        VR::DS,
        "0.01\\0.01",
    ));
    shared_item.put(pixel_measures);
    object.put(shared);
    object.write_to_file(&second).unwrap();

    let report = validate_dicom_path_with_runner(
        &set_dir,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();

    assert!(report.checks.iter().any(|check| {
        check.name == "intrinsic-wsi-dicom-2026c-pyramid-geometry"
            && check.status == ValidationStatus::Failed
    }));
}

#[test]
fn source_relationship_rule_rejects_self_reference() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_valid_color_wsi_for_validation(tmp.path(), "self-source");
    let mut object = dicom_object::open_file(&path).unwrap();
    let sop_class = object
        .element(tags::SOP_CLASS_UID)
        .unwrap()
        .to_str()
        .unwrap()
        .into_owned();
    let sop_instance = object
        .element(tags::SOP_INSTANCE_UID)
        .unwrap()
        .to_str()
        .unwrap()
        .into_owned();
    let mut reference = InMemDicomObject::new_empty();
    reference.put(DataElement::new(
        tags::REFERENCED_SOP_CLASS_UID,
        VR::UI,
        sop_class,
    ));
    reference.put(DataElement::new(
        tags::REFERENCED_SOP_INSTANCE_UID,
        VR::UI,
        sop_instance,
    ));
    object.put(DataElement::new(
        tags::SOURCE_IMAGE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![reference]),
    ));
    object.write_to_file(&path).unwrap();

    assert_intrinsic_rule_fails(&path, "intrinsic-wsi-dicom-2026c-source-relationship");
}

#[test]
fn source_relationship_rule_rejects_present_target_with_wrong_class() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let owner_source = export_valid_color_wsi_for_validation(tmp.path(), "owner");
    let target_source = export_valid_color_wsi_for_validation(tmp.path(), "target");
    let set_dir = tmp.path().join("relationship-set");
    std::fs::create_dir(&set_dir).unwrap();
    let owner_path = set_dir.join("owner.dcm");
    let target_path = set_dir.join("target.dcm");
    std::fs::copy(owner_source, &owner_path).unwrap();
    std::fs::copy(target_source, &target_path).unwrap();

    let target = dicom_object::open_file(&target_path).unwrap();
    let target_uid = target
        .element(tags::SOP_INSTANCE_UID)
        .unwrap()
        .to_str()
        .unwrap()
        .into_owned();
    let mut owner = dicom_object::open_file(&owner_path).unwrap();
    let mut reference = InMemDicomObject::new_empty();
    reference.put(DataElement::new(
        tags::REFERENCED_SOP_CLASS_UID,
        VR::UI,
        "1.2.840.10008.5.1.4.1.1.2",
    ));
    reference.put(DataElement::new(
        tags::REFERENCED_SOP_INSTANCE_UID,
        VR::UI,
        target_uid,
    ));
    owner.put(DataElement::new(
        tags::SOURCE_IMAGE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![reference]),
    ));
    owner.write_to_file(&owner_path).unwrap();

    let report = validate_dicom_path_with_runner(
        &set_dir,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();
    assert!(report.checks.iter().any(|check| {
        check.name == "intrinsic-wsi-dicom-2026c-source-relationship"
            && check.status == ValidationStatus::Failed
    }));
}

#[test]
fn applicable_source_relationship_is_reported_after_other_corpus_checks() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let owner_source = export_valid_color_wsi_for_validation(tmp.path(), "valid-owner");
    let target_source = export_valid_color_wsi_for_validation(tmp.path(), "valid-target");
    let set_dir = tmp.path().join("valid-relationship-set");
    std::fs::create_dir(&set_dir).unwrap();
    let owner_path = set_dir.join("owner.dcm");
    let target_path = set_dir.join("target.dcm");
    std::fs::copy(owner_source, &owner_path).unwrap();
    std::fs::copy(target_source, &target_path).unwrap();

    let target = dicom_object::open_file(&target_path).unwrap();
    let target_class = target
        .element(tags::SOP_CLASS_UID)
        .unwrap()
        .to_str()
        .unwrap()
        .into_owned();
    let target_uid = target
        .element(tags::SOP_INSTANCE_UID)
        .unwrap()
        .to_str()
        .unwrap()
        .into_owned();
    let mut owner = dicom_object::open_file(&owner_path).unwrap();
    let mut reference = InMemDicomObject::new_empty();
    reference.put(DataElement::new(
        tags::REFERENCED_SOP_CLASS_UID,
        VR::UI,
        target_class,
    ));
    reference.put(DataElement::new(
        tags::REFERENCED_SOP_INSTANCE_UID,
        VR::UI,
        target_uid,
    ));
    owner.put(DataElement::new(
        tags::SOURCE_IMAGE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![reference]),
    ));
    owner.write_to_file(&owner_path).unwrap();

    let report = validate_dicom_path_with_runner(
        &set_dir,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();
    let identity_index = report
        .checks
        .iter()
        .position(|check| check.name == "intrinsic-wsi-dicom-2026c-identity-set")
        .unwrap();
    let source_index = report
        .checks
        .iter()
        .position(|check| check.name == "intrinsic-wsi-dicom-2026c-source-relationship")
        .unwrap();

    assert_eq!(report.checks[source_index].status, ValidationStatus::Passed);
    assert!(identity_index < source_index);
}

#[test]
fn inverse_transfer_syntax_pixel_data_mismatch_is_intrinsically_invalid() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("explicit.dcm");
    write_encapsulated_dicom(&file, "1.2.840.10008.1.2.1", &[1, 2, 3, 4]);

    let report = validate_dicom_path_with_runner(
        &file,
        &ValidationOptions::default(),
        &FakeRunner::default(),
    )
    .expect("validation report");

    assert!(report.checks.iter().any(|check| {
        check.name == "intrinsic-pixel-structure" && check.status == ValidationStatus::Failed
    }));
    assert!(report.has_failures());
}

#[test]
fn native_transfer_syntax_with_defined_length_encapsulated_bytes_is_intrinsically_invalid() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("explicit-defined-length-encapsulation.dcm");
    let encapsulated_bytes = [
        0xFE, 0xFF, 0x00, 0xE0, 0x04, 0x00, 0x00, 0x00, // Basic Offset Table item
        0x00, 0x00, 0x00, 0x00, // first frame offset
        0xFE, 0xFF, 0x00, 0xE0, 0x04, 0x00, 0x00, 0x00, // fragment item
        0x01, 0x02, 0x03, 0x04, // fragment payload
    ];
    write_primitive_pixel_dicom(&file, "1.2.840.10008.1.2.1", &encapsulated_bytes);

    let report = validate_dicom_path_with_runner(
        &file,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .expect("validation report");

    assert!(report.checks.iter().any(|check| {
        check.name == "intrinsic-pixel-structure"
            && check.status == ValidationStatus::Failed
            && check.message.contains("encapsulated item stream")
    }));
    assert!(report.has_failures());
}

#[test]
fn valid_native_pixels_may_match_an_encapsulated_item_stream_byte_pattern() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("valid-item-looking-native-pixels.dcm");
    let item_looking_pixels = [
        0xFE, 0xFF, 0x00, 0xE0, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0x00,
        0xE0, 0x04, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04,
    ];
    write_primitive_pixel_dicom_with_geometry(
        &file,
        "1.2.840.10008.1.2.1",
        &item_looking_pixels,
        1,
        u16::try_from(item_looking_pixels.len()).unwrap(),
        1,
        8,
    );

    let report = validate_dicom_path_with_runner(
        &file,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .expect("validation report");

    assert!(report.checks.iter().any(|check| {
        check.name == "intrinsic-pixel-structure" && check.status == ValidationStatus::Passed
    }));
}

#[test]
fn compressed_primitive_and_empty_encapsulated_pixel_data_fail_without_decoding() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let primitive = tmp.path().join("primitive.dcm");
    write_primitive_pixel_dicom(
        &primitive,
        TransferSyntax::JpegBaseline8Bit.uid(),
        &[0xFF, 0xD8, 0xFF, 0xD9],
    );
    let empty = tmp.path().join("empty.dcm");
    write_encapsulated_dicom(&empty, TransferSyntax::JpegBaseline8Bit.uid(), &[]);

    for file in [primitive, empty] {
        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions {
                profile: ValidationProfile::Core2026c,
                max_pixel_frames: 0,
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .expect("validation report");
        assert!(report.checks.iter().any(|check| {
            check.name == "intrinsic-pixel-structure" && check.status == ValidationStatus::Failed
        }));
        assert!(report.has_failures());
    }
}
