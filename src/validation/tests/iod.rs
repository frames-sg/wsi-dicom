use super::*;
use crate::validation::ValidationCheck;
use dicom_object::DefaultDicomObject;

const IOD_CHECKS: [&str; 7] = [
    "intrinsic-wsi-dicom-2026c-iod-modules",
    "intrinsic-wsi-dicom-2026c-iod-attribute-presence",
    "intrinsic-wsi-dicom-2026c-iod-enumerated-values",
    "intrinsic-wsi-dicom-2026c-iod-vr-vm",
    "intrinsic-wsi-dicom-2026c-iod-value-format",
    "intrinsic-wsi-dicom-2026c-iod-sequence-items",
    "intrinsic-wsi-dicom-2026c-file-meta",
];

fn iod_check(path: &Path, profile: ValidationProfile, name: &str) -> ValidationCheck {
    let report = validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            profile,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();
    report
        .checks
        .into_iter()
        .find(|check| check.name == name)
        .unwrap_or_else(|| panic!("missing check {name}"))
}

fn assert_iod_failure(path: &Path, name: &str, expected: &str) {
    let check = iod_check(path, ValidationProfile::General, name);
    assert_eq!(
        check.status,
        ValidationStatus::Failed,
        "{name}: {}",
        check.message
    );
    assert!(
        check.message.contains(expected),
        "{name} message lacks {expected:?}: {}",
        check.message
    );
}

fn assert_iod_passes(path: &Path, name: &str) {
    let check = iod_check(path, ValidationProfile::General, name);
    assert_eq!(
        check.status,
        ValidationStatus::Passed,
        "{name}: {}",
        check.message
    );
}

/// Exports a valid WSI, applies `edit`, and writes it back.
fn edited_wsi(root: &Path, name: &str, edit: impl FnOnce(&mut DefaultDicomObject)) -> PathBuf {
    let path = export_valid_color_wsi_for_validation(root, name);
    let mut object = dicom_object::open_file(&path).unwrap();
    edit(&mut object);
    object.write_to_file(&path).unwrap();
    path
}

#[test]
fn exported_wsi_passes_every_iod_check_in_both_profiles() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "valid-grid", 4, 4, 2);
    for profile in [ValidationProfile::General, ValidationProfile::Core2026c] {
        for name in IOD_CHECKS {
            let check = iod_check(&path, profile, name);
            assert_eq!(
                check.status,
                ValidationStatus::Passed,
                "{profile:?} {name}: {}",
                check.message
            );
        }
    }
}

#[test]
fn nested_type_one_deletion_names_the_path_and_table() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "optical-path-identifier", |object| {
        let mut paths = object.take(tags::OPTICAL_PATH_SEQUENCE).unwrap();
        paths.items_mut().unwrap()[0]
            .take(tags::OPTICAL_PATH_IDENTIFIER)
            .unwrap();
        object.put(paths);
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-iod-attribute-presence",
        "Optical Path Sequence[1] > Optical Path Identifier (0048,0106): Type 1 missing (PS3.3 C.8.12.5 Table C.8.12.5-1)",
    );
}

#[test]
fn type_two_attribute_must_be_present_but_may_be_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = edited_wsi(tmp.path(), "patient-name-missing", |object| {
        object.take(tags::PATIENT_NAME);
    });
    assert_iod_failure(
        &missing,
        "intrinsic-wsi-dicom-2026c-iod-attribute-presence",
        "Patient's Name (0010,0010): Type 2 missing (PS3.3 C.7.1.1 Table C.7-1)",
    );
    let empty = edited_wsi(tmp.path(), "patient-name-empty", |object| {
        object.put(DataElement::new(
            tags::PATIENT_NAME,
            VR::PN,
            PrimitiveValue::Empty,
        ));
    });
    assert_iod_passes(&empty, "intrinsic-wsi-dicom-2026c-iod-attribute-presence");
}

#[test]
fn type_one_attribute_needs_a_value() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "software-versions-empty", |object| {
        object.put(DataElement::new(
            tags::SOFTWARE_VERSIONS,
            VR::LO,
            PrimitiveValue::from("\\"),
        ));
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-iod-attribute-presence",
        "Software Versions (0018,1020): Type 1 present without a Value",
    );
}

#[test]
fn conditional_attribute_is_forbidden_when_its_condition_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "lossless-with-ratio", |object| {
        object.put(DataElement::new(
            tags::LOSSY_IMAGE_COMPRESSION,
            VR::CS,
            "00",
        ));
        object.put(DataElement::new(
            tags::LOSSY_IMAGE_COMPRESSION_RATIO,
            VR::DS,
            "10",
        ));
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-iod-attribute-presence",
        "Lossy Image Compression Ratio (0028,2112): Type 1C present although its condition is not met",
    );
}

#[test]
fn enumerated_values_are_enforced_but_defined_terms_are_not() {
    let tmp = tempfile::tempdir().unwrap();
    let enumerated = edited_wsi(tmp.path(), "focus-method", |object| {
        object.put(DataElement::new(tags::FOCUS_METHOD, VR::CS, "SEMIAUTO"));
    });
    assert_iod_failure(
        &enumerated,
        "intrinsic-wsi-dicom-2026c-iod-enumerated-values",
        "Focus Method (0048,0011) \"SEMIAUTO\" is not an Enumerated Value [AUTO, MANUAL]",
    );
    let image_type = edited_wsi(tmp.path(), "image-type-secondary", |object| {
        object.put(DataElement::new(
            tags::IMAGE_TYPE,
            VR::CS,
            PrimitiveValue::Strs(
                ["ORIGINAL", "SECONDARY", "VOLUME", "NONE"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            ),
        ));
    });
    assert_iod_failure(
        &image_type,
        "intrinsic-wsi-dicom-2026c-iod-enumerated-values",
        "Image Type (0008,0008) Value 2 \"SECONDARY\" is not an Enumerated Value [PRIMARY]",
    );
    let defined_term = edited_wsi(tmp.path(), "type-of-patient-id", |object| {
        object.put(DataElement::new(
            tags::TYPE_OF_PATIENT_ID,
            VR::CS,
            "LOCAL_CODE",
        ));
    });
    assert_iod_passes(
        &defined_term,
        "intrinsic-wsi-dicom-2026c-iod-enumerated-values",
    );
}

#[test]
fn vr_and_vm_follow_the_data_dictionary() {
    let tmp = tempfile::tempdir().unwrap();
    let multiplicity = edited_wsi(tmp.path(), "image-type-vm", |object| {
        object.put(DataElement::new(tags::IMAGE_TYPE, VR::CS, "ORIGINAL"));
    });
    assert_iod_failure(
        &multiplicity,
        "intrinsic-wsi-dicom-2026c-iod-vr-vm",
        "ImageType (0008,0008): 1 Value where PS3.6 VM is 2-n",
    );
    let representation = edited_wsi(tmp.path(), "modality-vr", |object| {
        object.put(DataElement::new(tags::MODALITY, VR::LO, "SM"));
    });
    assert_iod_failure(
        &representation,
        "intrinsic-wsi-dicom-2026c-iod-vr-vm",
        "Modality (0008,0060): VR LO where PS3.6 specifies CS",
    );
}

#[test]
fn value_format_rejects_malformed_strings() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "study-date", |object| {
        object.put(DataElement::new(tags::STUDY_DATE, VR::DA, "2024-01-01"));
        object.put(DataElement::new(
            tags::FRAME_OF_REFERENCE_UID,
            VR::UI,
            "1.2.04.5",
        ));
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-iod-value-format",
        "StudyDate (0008,0020) \"2024-01-01\": not of the form YYYYMMDD for VR DA (PS3.5 6.2)",
    );
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-iod-value-format",
        "FrameOfReferenceUID (0020,0052) \"1.2.04.5\"",
    );
}

#[test]
fn sequence_item_counts_follow_the_attribute_description() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "two-shared-items", |object| {
        let mut shared = object
            .take(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
            .unwrap();
        let items = shared.items_mut().unwrap();
        items.push(items[0].clone());
        object.put(shared);
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-iod-sequence-items",
        "Shared Functional Groups Sequence (5200,9229): 2 Items where exactly 1 is required",
    );
}

#[test]
fn functional_group_usage_and_frame_count_are_enforced() {
    let tmp = tempfile::tempdir().unwrap();
    let moved = edited_wsi(tmp.path(), "pixel-measures-per-frame", |object| {
        let mut shared = object
            .take(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
            .unwrap();
        let measures = shared.items_mut().unwrap()[0]
            .take(tags::PIXEL_MEASURES_SEQUENCE)
            .unwrap();
        object.put(shared);
        let mut frames = object
            .take(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
            .unwrap();
        for frame in frames.items_mut().unwrap() {
            frame.put(measures.clone());
        }
        object.put(frames);
    });
    assert_iod_failure(
        &moved,
        "intrinsic-wsi-dicom-2026c-iod-modules",
        "Pixel Measures Functional Group (PS3.3 C.7.6.16.2.1, Usage M): shall be used as a Shared Functional Group",
    );
    let counted = edited_wsi(tmp.path(), "frame-count", |object| {
        object.put(DataElement::new(tags::NUMBER_OF_FRAMES, VR::IS, "2"));
    });
    assert_iod_failure(
        &counted,
        "intrinsic-wsi-dicom-2026c-iod-modules",
        "Per-Frame Functional Groups Sequence (5200,9230): 1 Items for Number of Frames 2",
    );
}

#[test]
fn file_meta_must_identify_the_data_set() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "instance-mismatch", |object| {
        object.put(DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.9",
        ));
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-file-meta",
        "does not match SOP Instance UID (0008,0018)",
    );
}

#[test]
fn missing_sop_class_uid_still_runs_the_iod_checks_for_a_declared_wsi_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "sop-class", |object| {
        object.take(tags::SOP_CLASS_UID);
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-iod-attribute-presence",
        "SOP Class UID (0008,0016): Type 1 missing (PS3.3 C.12.1 Table C.12-1)",
    );
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-file-meta",
        "Media Storage SOP Class UID (0002,0002)",
    );
}

#[test]
fn violation_messages_are_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "bounded", 16, 16, 2);
    let mut object = dicom_object::open_file(&path).unwrap();
    let mut frames = object
        .take(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap();
    let frame_count = frames.items().unwrap().len();
    for frame in frames.items_mut().unwrap() {
        let mut content = frame.take(tags::FRAME_CONTENT_SEQUENCE).unwrap();
        content.items_mut().unwrap()[0].put(DataElement::new(
            tags::FRAME_ACQUISITION_DATE_TIME,
            VR::DT,
            "yesterday",
        ));
        frame.put(content);
    }
    object.put(frames);
    object.write_to_file(&path).unwrap();
    assert!(
        frame_count > 20,
        "fixture needs more Frames than shown violations"
    );

    let check = iod_check(
        &path,
        ValidationProfile::General,
        "intrinsic-wsi-dicom-2026c-iod-value-format",
    );
    assert_eq!(check.status, ValidationStatus::Failed);
    assert!(check.message.starts_with(&format!(
        "DICOM PS3.5 2026c 6.2 and 7.1: {frame_count} violations: "
    )));
    assert!(check
        .message
        .contains(&format!("; and {} more", frame_count - 20)));
    assert!(check.message.len() < 20 * 400, "message is not bounded");
}

fn patch_file_meta_version(path: &Path, replacement: [u8; 2]) {
    let mut bytes = std::fs::read(path).unwrap();
    // (0002,0001) OB, reserved 0000H, length 2, then the two version bytes.
    let version: &[u8] = &[
        0x02, 0x00, 0x01, 0x00, b'O', b'B', 0, 0, 2, 0, 0, 0, 0x00, 0x01,
    ];
    let at = bytes
        .windows(version.len())
        .position(|window| window == version)
        .expect("File Meta Information Version element");
    bytes[at + version.len() - 2..at + version.len()].copy_from_slice(&replacement);
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn exported_file_meta_uses_the_version_one_value() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "meta-version-writer");

    assert_eq!(
        dicom_object::open_file(path)
            .unwrap()
            .meta()
            .information_version,
        [0x00, 0x01]
    );
}

#[test]
fn file_meta_accepts_a_compatible_future_version() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, version) in [
        ("second-byte-bits", [0x00, 0x03]),
        ("first-byte-bits", [0x80, 0x01]),
    ] {
        let path = export_valid_color_wsi_for_validation(tmp.path(), name);
        patch_file_meta_version(&path, version);

        assert_iod_passes(&path, "intrinsic-wsi-dicom-2026c-file-meta");
    }
}

#[test]
fn file_meta_rejects_a_version_without_version_one_compatibility() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "meta-version");
    patch_file_meta_version(&path, [0x00, 0x02]);

    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-file-meta",
        "File Meta Information Version (0002,0001) is [00, 02]; bit 0 of the second byte is not set",
    );
}

/// Removes group 0002 `elements` from an exported file as encoded, and
/// shortens its File Meta Information Group Length to match.
fn remove_file_meta_elements(path: &std::path::Path, elements: &[u16]) {
    let mut bytes = std::fs::read(path).unwrap();
    let group_length_at = 128 + 4 + 8;
    let read_u32 =
        |bytes: &[u8], at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let group_end = group_length_at + 4 + read_u32(&bytes, group_length_at) as usize;
    let mut offset = group_length_at + 4;
    let mut kept = Vec::new();
    let mut removed = 0;
    while offset < group_end {
        let element = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]);
        let long = matches!(
            &bytes[offset + 4..offset + 6],
            b"OB"
                | b"OD"
                | b"OF"
                | b"OL"
                | b"OV"
                | b"OW"
                | b"SQ"
                | b"UC"
                | b"UN"
                | b"UR"
                | b"UT"
                | b"UV"
        );
        let length = if long {
            12 + read_u32(&bytes, offset + 8) as usize
        } else {
            8 + usize::from(u16::from_le_bytes([bytes[offset + 6], bytes[offset + 7]]))
        };
        if elements.contains(&element) {
            removed += length;
        } else {
            kept.extend_from_slice(&bytes[offset..offset + length]);
        }
        offset += length;
    }
    assert_eq!(
        removed > 0,
        !elements.is_empty(),
        "File Meta elements to remove"
    );
    bytes.splice(group_length_at + 4..group_end, kept);
    let shortened = (group_end - group_length_at - 4 - removed) as u32;
    bytes[group_length_at..group_length_at + 4].copy_from_slice(&shortened.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn file_meta_is_checked_when_the_data_set_cannot_be_parsed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "no-transfer-syntax");
    // Without Transfer Syntax UID the parser cannot read the Data Set.
    remove_file_meta_elements(&path, &[0x0010]);

    assert_iod_failure(&path, "intrinsic-wsi-dicom-2026c-file-meta", "(0002,0010)");
    let pixel_structure = iod_check(
        &path,
        ValidationProfile::General,
        "intrinsic-pixel-structure",
    );
    assert_eq!(pixel_structure.status, ValidationStatus::Failed);
}

#[test]
fn file_meta_is_checked_when_an_unparsed_file_declares_no_sop_class() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "no-sop-class");
    remove_file_meta_elements(&path, &[0x0002, 0x0010]);

    assert_iod_failure(&path, "intrinsic-wsi-dicom-2026c-file-meta", "(0002,0002)");
}

#[test]
fn references_to_other_instances_must_be_listed_in_the_common_instance_reference_module() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "unlisted-reference", |object| {
        let mut reference = dicom_object::InMemDicomObject::new_empty();
        reference.put(dicom_core::DataElement::new(
            tags::REFERENCED_SOP_CLASS_UID,
            dicom_core::VR::UI,
            crate::VL_WSI_SOP_CLASS_UID,
        ));
        reference.put(dicom_core::DataElement::new(
            tags::REFERENCED_SOP_INSTANCE_UID,
            dicom_core::VR::UI,
            "1.2.826.0.1.3680043.10.999.81",
        ));
        object.put(dicom_core::DataElement::new(
            tags::SOURCE_IMAGE_SEQUENCE,
            dicom_core::VR::SQ,
            dicom_core::value::DataSetSequence::from(vec![reference]),
        ));
    });

    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-2026c-iod-attribute-presence",
        "Referenced Series Sequence (0008,1115): Type 1C missing",
    );
}
