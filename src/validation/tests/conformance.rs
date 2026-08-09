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
        "intrinsic-wsi-dicom-2026c-dimension-order",
        "intrinsic-wsi-dicom-2026c-specimen-uid-set",
    ] {
        assert!(
            report
                .checks
                .iter()
                .any(|check| { check.name == name && check.status == ValidationStatus::Passed }),
            "missing passing intrinsic check {name}"
        );
    }
    assert!(!report.has_failures());
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
