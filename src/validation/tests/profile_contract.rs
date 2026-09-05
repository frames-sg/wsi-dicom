use super::*;

fn intrinsic_status(path: &Path, name: &str) -> ValidationStatus {
    let report = validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            profile: ValidationProfile::Core2026c,
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();
    report
        .checks
        .iter()
        .find(|check| check.name == name)
        .unwrap()
        .status
}

#[test]
fn tiled_full_accepts_implicit_frame_positions() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "implicit", 4, 4, 2);
    let mut object = dicom_object::open_file(&path).unwrap();
    object
        .take(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap();
    object.take(tags::DIMENSION_INDEX_SEQUENCE).unwrap();
    let mut shared = object
        .take(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap();
    shared.items_mut().unwrap()[0]
        .take(tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE)
        .unwrap();
    object.put(shared);
    object.write_to_file(&path).unwrap();
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-2026c-tile-geometry"),
        ValidationStatus::Passed
    );
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-2026c-optical-path-structure"),
        ValidationStatus::Passed
    );
}

#[test]
fn tiled_full_rejects_a_complete_but_permuted_frame_order() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "permuted", 4, 4, 2);
    let mut object = dicom_object::open_file(&path).unwrap();
    let mut frames = object
        .take(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap();
    frames.items_mut().unwrap().swap(0, 1);
    object.put(frames);
    object.write_to_file(&path).unwrap();
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-2026c-tile-geometry"),
        ValidationStatus::Failed
    );
}

#[test]
fn wsi_pixel_metadata_enforces_specialized_values() {
    for (tag, value) in [
        (tags::PIXEL_REPRESENTATION, 1u16),
        (tags::BITS_STORED, 7),
        (tags::PLANAR_CONFIGURATION, 1),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let path = export_valid_color_wsi_for_validation(tmp.path(), "metadata");
        let mut object = dicom_object::open_file(&path).unwrap();
        object.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
        if tag == tags::BITS_STORED {
            object.put(DataElement::new(
                tags::HIGH_BIT,
                VR::US,
                PrimitiveValue::from(6u16),
            ));
        }
        object.write_to_file(&path).unwrap();
        assert_eq!(
            intrinsic_status(&path, "intrinsic-wsi-dicom-2026c-image-metadata"),
            ValidationStatus::Failed,
            "tag {tag}"
        );
    }
}

#[test]
fn wsi_pixel_metadata_rejects_color_space_outside_its_transfer_syntax() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "photometric");
    let mut object = dicom_object::open_file(&path).unwrap();
    object.put(DataElement::new(
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        "YBR_PARTIAL_420",
    ));
    object.write_to_file(&path).unwrap();
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-2026c-image-metadata"),
        ValidationStatus::Failed
    );
}

#[test]
fn core_requires_frame_type_and_derivation_when_source_is_referenced() {
    for derived in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let path = export_valid_color_wsi_for_validation(tmp.path(), "functional-groups");
        let mut object = dicom_object::open_file(&path).unwrap();
        if derived {
            object.put(DataElement::new(
                tags::IMAGE_TYPE,
                VR::CS,
                "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED",
            ));
            let mut shared = object
                .take(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
                .unwrap();
            let mut frame_type = shared.items_mut().unwrap()[0]
                .take(tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE)
                .unwrap();
            frame_type.items_mut().unwrap()[0].put(DataElement::new(
                tags::FRAME_TYPE,
                VR::CS,
                "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED",
            ));
            shared.items_mut().unwrap()[0].put(frame_type);
            object.put(shared);
            object.put(DataElement::new(
                tags::SOURCE_IMAGE_SEQUENCE,
                VR::SQ,
                dicom_core::value::DataSetSequence::from(vec![
                    dicom_object::InMemDicomObject::new_empty(),
                ]),
            ));
        } else {
            let mut shared = object
                .take(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
                .unwrap();
            shared.items_mut().unwrap()[0]
                .take(tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE)
                .unwrap();
            object.put(shared);
        }
        object.write_to_file(&path).unwrap();
        assert_eq!(
            intrinsic_status(&path, "intrinsic-wsi-dicom-2026c-core-image-profile"),
            ValidationStatus::Failed,
            "derived={derived}"
        );
    }
}

#[test]
fn general_profile_omits_core_only_checks_and_core_rejects_other_sop_classes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "scope");
    let general = validate_dicom_path_with_runner(
        &path,
        &ValidationOptions {
            max_pixel_frames: 0,
            ..Default::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();
    assert!(!general
        .checks
        .iter()
        .any(|c| c.name == "intrinsic-wsi-dicom-2026c-core-image-profile"));
    let mut object = dicom_object::open_file(&path).unwrap();
    object.put(DataElement::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        "1.2.840.10008.5.1.4.1.1.2",
    ));
    object.write_to_file(&path).unwrap();
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-2026c-core-image-profile"),
        ValidationStatus::Failed
    );
}

#[test]
fn validation_enforces_input_size_before_parsing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "input-budget");
    let size = std::fs::metadata(&path).unwrap().len();
    let options = ValidationOptions {
        max_input_bytes: size - 1,
        max_pixel_frames: 0,
        ..Default::default()
    };
    let error =
        validate_dicom_path_with_runner(&path, &options, &FakeRunner::default()).unwrap_err();
    assert!(error.to_string().contains("max_input_bytes"), "{error}");
    let options = ValidationOptions {
        max_input_bytes: size,
        ..options
    };
    assert!(validate_dicom_path_with_runner(&path, &options, &FakeRunner::default()).is_ok());
}

#[test]
fn bounded_reader_preserves_files_without_the_optional_preamble() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "no-preamble");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[128..]).unwrap();
    assert!(dicom_object::open_file(&path).is_ok());
    assert_eq!(
        intrinsic_status(&path, "intrinsic-pixel-structure"),
        ValidationStatus::Passed
    );
}
