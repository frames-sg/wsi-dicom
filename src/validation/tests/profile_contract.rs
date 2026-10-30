use super::*;

fn intrinsic_status(path: &Path, name: &str) -> ValidationStatus {
    let report = validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            profile: ValidationProfile::Core,
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
        intrinsic_status(&path, "intrinsic-wsi-dicom-tile-geometry"),
        ValidationStatus::Passed
    );
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-optical-path-structure"),
        ValidationStatus::Passed
    );
}

#[test]
fn tiled_full_accepts_dimension_index_without_per_frame_groups() {
    // PixelMed TIFFToDicom output (most of NCI IDC) keeps the row/column
    // Dimension Index Sequence and omits the Per-frame Functional Groups.
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "implicit-index", 4, 4, 2);
    let mut object = dicom_object::open_file(&path).unwrap();
    object
        .take(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap();
    object.write_to_file(&path).unwrap();
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-dimension-order"),
        ValidationStatus::Passed
    );
}

#[test]
fn dimension_order_requires_per_frame_groups_outside_tiled_full() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "sparse-index", 4, 4, 2);
    let mut object = dicom_object::open_file(&path).unwrap();
    object
        .take(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap();
    object.put(DataElement::new(
        tags::DIMENSION_ORGANIZATION_TYPE,
        VR::CS,
        "TILED_SPARSE",
    ));
    object.write_to_file(&path).unwrap();
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-dimension-order"),
        ValidationStatus::Failed
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
        intrinsic_status(&path, "intrinsic-wsi-dicom-tile-geometry"),
        ValidationStatus::Failed
    );
}

#[test]
fn tile_geometry_defers_missing_required_attributes_to_iod_presence() {
    let tmp = tempfile::tempdir().unwrap();
    let exported = export_valid_color_wsi_grid_for_validation(tmp.path(), "missing", 4, 4, 2);
    let original = dicom_object::open_file(&exported).unwrap();
    for tag in [
        tags::NUMBER_OF_FRAMES,
        tags::ROWS,
        tags::COLUMNS,
        tags::TOTAL_PIXEL_MATRIX_ROWS,
        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
    ] {
        let path = tmp
            .path()
            .join(format!("missing-{}-{}.dcm", tag.group(), tag.element()));
        let mut object = original.clone();
        object.take(tag).unwrap();
        object.write_to_file(&path).unwrap();

        assert_eq!(
            intrinsic_status(&path, "intrinsic-wsi-dicom-tile-geometry"),
            ValidationStatus::Passed,
            "tag {tag}"
        );
        assert_eq!(
            intrinsic_status(&path, "intrinsic-wsi-dicom-iod-attribute-presence"),
            ValidationStatus::Failed,
            "tag {tag}"
        );
    }

    let path = tmp.path().join("unspecified.dcm");
    let mut object = original;
    object.take(tags::DIMENSION_ORGANIZATION_TYPE).unwrap();
    object.write_to_file(&path).unwrap();
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-tile-geometry"),
        ValidationStatus::Passed
    );
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-iod-attribute-presence"),
        ValidationStatus::Passed
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
            intrinsic_status(&path, "intrinsic-wsi-dicom-image-metadata"),
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
        intrinsic_status(&path, "intrinsic-wsi-dicom-image-metadata"),
        ValidationStatus::Failed
    );
}

fn intrinsic_check(path: &Path, name: &str) -> crate::validation::ValidationCheck {
    validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            profile: ValidationProfile::Core,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap()
    .checks
    .into_iter()
    .find(|check| check.name == name)
    .unwrap()
}

#[test]
fn wsi_pixel_metadata_follows_the_transfer_syntax_tables() {
    for (syntax, photometric, expected) in [
        // PS3.5 Table 8.2.1-1 lists JPEG Extended for monochrome images only.
        ("1.2.840.10008.1.2.4.51", "YBR_FULL_422", "is not permitted"),
        ("1.2.840.10008.1.2.4.51", "RGB", "is not permitted"),
        ("1.2.840.10008.1.2.4.110", "YBR_RCT", ""),
        ("1.2.840.10008.1.2.4.111", "YBR_FULL_422", ""),
        (
            "1.2.840.10008.1.2.4.100.1",
            "RGB",
            "is a motion compression syntax",
        ),
        (
            "1.2.840.10008.1.2.4.108",
            "YBR_PARTIAL_420",
            "is a motion compression syntax",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let path = export_valid_color_wsi_for_validation(tmp.path(), "syntax");
        let mut object = dicom_object::open_file(&path).unwrap();
        object.put(DataElement::new(
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            photometric,
        ));
        let meta = object.meta_mut();
        meta.transfer_syntax = syntax.to_string();
        meta.update_information_group_length();
        object.write_to_file(&path).unwrap();
        let check = intrinsic_check(&path, "intrinsic-wsi-dicom-image-metadata");
        if expected.is_empty() {
            assert_eq!(
                check.status,
                ValidationStatus::Passed,
                "{syntax}: {}",
                check.message
            );
        } else {
            assert_eq!(check.status, ValidationStatus::Failed, "{syntax}");
            assert!(check.message.contains(expected), "{}", check.message);
        }
    }
}

#[test]
fn wsi_icon_images_are_checked_for_pixel_attributes_and_icc_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "icon");
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-pixel-attributes"),
        ValidationStatus::Passed
    );
    let mut object = dicom_object::open_file(&path).unwrap();
    let mut profile = object
        .element(tags::OPTICAL_PATH_SEQUENCE)
        .unwrap()
        .items()
        .unwrap()[0]
        .element(tags::ICC_PROFILE)
        .unwrap()
        .to_bytes()
        .unwrap()
        .into_owned();
    profile[16..20].copy_from_slice(b"GRAY");
    let icon = InMemDicomObject::from_element_iter([
        DataElement::new(tags::SAMPLES_PER_PIXEL, VR::US, PrimitiveValue::from(3_u16)),
        DataElement::new(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "RGB"),
        DataElement::new(tags::ROWS, VR::US, PrimitiveValue::from(1_u16)),
        DataElement::new(tags::COLUMNS, VR::US, PrimitiveValue::from(2_u16)),
        DataElement::new(tags::BITS_ALLOCATED, VR::US, PrimitiveValue::from(8_u16)),
        DataElement::new(tags::BITS_STORED, VR::US, PrimitiveValue::from(8_u16)),
        DataElement::new(tags::HIGH_BIT, VR::US, PrimitiveValue::from(7_u16)),
        DataElement::new(
            tags::PIXEL_REPRESENTATION,
            VR::US,
            PrimitiveValue::from(0_u16),
        ),
        DataElement::new(
            tags::PLANAR_CONFIGURATION,
            VR::US,
            PrimitiveValue::from(0_u16),
        ),
        DataElement::new(tags::ICC_PROFILE, VR::OB, PrimitiveValue::from(profile)),
        DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PrimitiveValue::from(vec![0_u8; 6]),
        ),
    ]);
    object.put(DataElement::new(
        tags::ICON_IMAGE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![icon]),
    ));
    object.write_to_file(&path).unwrap();

    let attributes = intrinsic_check(&path, "intrinsic-wsi-dicom-pixel-attributes");
    assert_eq!(attributes.status, ValidationStatus::Failed);
    for expected in [
        "Icon Image Sequence (0088,0200) Item 1: Samples per Pixel is 3",
        "Photometric Interpretation is RGB; Icon Images are MONOCHROME1, MONOCHROME2 or PALETTE COLOR",
        "Planar Configuration (0028,0006) is present",
    ] {
        assert!(attributes.message.contains(expected), "{}", attributes.message);
    }
    let icc = intrinsic_check(&path, "intrinsic-wsi-dicom-icc-profile");
    assert_eq!(icc.status, ValidationStatus::Failed);
    assert!(
        icc.message
            .contains("Icon Image Sequence item 0 has an invalid ICC Profile"),
        "{}",
        icc.message
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
            intrinsic_status(&path, "intrinsic-wsi-dicom-core-image-profile"),
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
            ..Default::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();
    assert!(!general
        .checks
        .iter()
        .any(|c| c.name == "intrinsic-wsi-dicom-core-image-profile"));
    let mut object = dicom_object::open_file(&path).unwrap();
    object.put(DataElement::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        "1.2.840.10008.5.1.4.1.1.2",
    ));
    object.write_to_file(&path).unwrap();
    assert_eq!(
        intrinsic_status(&path, "intrinsic-wsi-dicom-core-image-profile"),
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
