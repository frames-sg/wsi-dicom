use super::*;
use crate::validation::{ValidationCheck, ValidationReport};

const IOD_CHECKS: [&str; 8] = [
    "intrinsic-wsi-dicom-iod-modules",
    "intrinsic-wsi-dicom-iod-attribute-presence",
    "intrinsic-wsi-dicom-iod-enumerated-values",
    "intrinsic-wsi-dicom-iod-vr-vm",
    "intrinsic-wsi-dicom-iod-value-format",
    "intrinsic-wsi-dicom-iod-sequence-items",
    "intrinsic-wsi-dicom-iod-value-sets",
    "intrinsic-wsi-dicom-file-meta",
];

fn iod_report(path: &Path, profile: ValidationProfile) -> ValidationReport {
    validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            profile,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap()
}

fn named(report: &ValidationReport, name: &str) -> ValidationCheck {
    report
        .checks
        .iter()
        .find(|check| check.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("missing check {name}"))
}

fn iod_check(path: &Path, profile: ValidationProfile, name: &str) -> ValidationCheck {
    named(&iod_report(path, profile), name)
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

#[test]
fn exported_wsi_passes_every_iod_check_in_both_profiles() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "valid-grid", 4, 4, 2);
    for profile in [ValidationProfile::General, ValidationProfile::Core] {
        let report = iod_report(&path, profile);
        for name in IOD_CHECKS {
            let check = named(&report, name);
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
        "intrinsic-wsi-dicom-iod-attribute-presence",
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
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "Patient's Name (0010,0010): Type 2 missing (PS3.3 C.7.1.1 Table C.7-1)",
    );
    let empty = edited_wsi(tmp.path(), "patient-name-empty", |object| {
        object.put(DataElement::new(
            tags::PATIENT_NAME,
            VR::PN,
            PrimitiveValue::Empty,
        ));
    });
    assert_iod_passes(&empty, "intrinsic-wsi-dicom-iod-attribute-presence");
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
        "intrinsic-wsi-dicom-iod-attribute-presence",
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
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "Lossy Image Compression Ratio (0028,2112): Type 1C present although its condition is not met",
    );
}

#[test]
fn a_color_wsi_needs_planar_configuration() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "planar-configuration-missing", |object| {
        object.take(tags::PLANAR_CONFIGURATION).unwrap();
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "Planar Configuration (0028,0006): Type 1C missing",
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
        "intrinsic-wsi-dicom-iod-enumerated-values",
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
        "intrinsic-wsi-dicom-iod-enumerated-values",
        "Image Type (0008,0008) Value 2 \"SECONDARY\" is not an Enumerated Value [PRIMARY]",
    );
    let defined_term = edited_wsi(tmp.path(), "type-of-patient-id", |object| {
        object.put(DataElement::new(
            tags::TYPE_OF_PATIENT_ID,
            VR::CS,
            "LOCAL_CODE",
        ));
    });
    assert_iod_passes(&defined_term, "intrinsic-wsi-dicom-iod-enumerated-values");
}

#[test]
fn vr_and_vm_follow_the_data_dictionary() {
    let tmp = tempfile::tempdir().unwrap();
    let multiplicity = edited_wsi(tmp.path(), "image-type-vm", |object| {
        object.put(DataElement::new(tags::IMAGE_TYPE, VR::CS, "ORIGINAL"));
    });
    assert_iod_failure(
        &multiplicity,
        "intrinsic-wsi-dicom-iod-vr-vm",
        "ImageType (0008,0008): 1 Value where PS3.6 VM is 2-n",
    );
    let representation = edited_wsi(tmp.path(), "modality-vr", |object| {
        object.put(DataElement::new(tags::MODALITY, VR::LO, "SM"));
    });
    assert_iod_failure(
        &representation,
        "intrinsic-wsi-dicom-iod-vr-vm",
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
        "intrinsic-wsi-dicom-iod-value-format",
        "StudyDate (0008,0020) \"2024-01-01\": not of the form YYYYMMDD for VR DA (PS3.5 6.2)",
    );
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-value-format",
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
        "intrinsic-wsi-dicom-iod-sequence-items",
        "Shared Functional Groups Sequence (5200,9229): 2 Items where exactly 1 is required",
    );
}

#[test]
fn urn_code_values_without_a_scheme_still_undergo_context_group_validation() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "urn-measurement-unit", |object| {
        let mut unit = InMemDicomObject::new_empty();
        unit.put(DataElement::new(
            tags::URN_CODE_VALUE,
            VR::UR,
            "urn:example:invalid-unit",
        ));
        unit.put(DataElement::new(tags::CODE_MEANING, VR::LO, "Invalid unit"));

        let mut mapping = InMemDicomObject::new_empty();
        mapping.put(DataElement::new(tags::LUT_EXPLANATION, VR::LO, "Test"));
        mapping.put(DataElement::new(tags::LUT_LABEL, VR::SH, "TEST"));
        mapping.put(DataElement::new(
            tags::REAL_WORLD_VALUE_FIRST_VALUE_MAPPED,
            VR::US,
            PrimitiveValue::from(0_u16),
        ));
        mapping.put(DataElement::new(
            tags::REAL_WORLD_VALUE_LAST_VALUE_MAPPED,
            VR::US,
            PrimitiveValue::from(255_u16),
        ));
        mapping.put(DataElement::new(
            tags::REAL_WORLD_VALUE_INTERCEPT,
            VR::FD,
            PrimitiveValue::from(0_f64),
        ));
        mapping.put(DataElement::new(
            tags::REAL_WORLD_VALUE_SLOPE,
            VR::FD,
            PrimitiveValue::from(1_f64),
        ));
        mapping.put(DataElement::new(
            tags::MEASUREMENT_UNITS_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![unit]),
        ));

        let mut shared = object
            .take(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
            .unwrap();
        shared.items_mut().unwrap()[0].put(DataElement::new(
            tags::REAL_WORLD_VALUE_MAPPING_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![mapping]),
        ));
        object.put(shared);
    });

    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-value-sets",
        "(urn:example:invalid-unit, no Coding Scheme Designator, \"Invalid unit\") is not in Defined CID 82 \"Measurement Unit\"",
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
        "intrinsic-wsi-dicom-iod-modules",
        "Pixel Measures Functional Group (PS3.3 C.7.6.16.2.1, Usage M): shall be used as a Shared Functional Group",
    );
    let counted = edited_wsi(tmp.path(), "frame-count", |object| {
        object.put(DataElement::new(tags::NUMBER_OF_FRAMES, VR::IS, "2"));
    });
    assert_iod_failure(
        &counted,
        "intrinsic-wsi-dicom-iod-sequence-items",
        "Per-Frame Functional Groups Sequence (5200,9230): 1 Item where 2 (the Value of (0028,0008)) are required",
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
        "intrinsic-wsi-dicom-file-meta",
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
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "SOP Class UID (0008,0016): Type 1 missing (PS3.3 C.12.1 Table C.12-1)",
    );
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-file-meta",
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
        "intrinsic-wsi-dicom-iod-value-format",
    );
    assert_eq!(check.status, ValidationStatus::Failed);
    assert!(check.message.starts_with(&format!(
        "DICOM PS3.5 2026d 6.2 and 7.1: {frame_count} violations: "
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

        assert_iod_passes(&path, "intrinsic-wsi-dicom-file-meta");
    }
}

#[test]
fn file_meta_rejects_a_version_without_version_one_compatibility() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "meta-version");
    patch_file_meta_version(&path, [0x00, 0x02]);

    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-file-meta",
        "File Meta Information Version (0002,0001) is [00, 02]; bit 0 of the second byte is not set",
    );
}

/// Rewrites the group 0002 elements of an exported file as encoded, each a
/// (element number, encoded bytes) pair, and recomputes the File Meta
/// Information Group Length.
fn rewrite_file_meta(path: &Path, edit: impl FnOnce(&mut Vec<(u16, Vec<u8>)>)) {
    let mut bytes = std::fs::read(path).unwrap();
    let group_length_at = 128 + 4 + 8;
    let read_u32 =
        |bytes: &[u8], at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let group_end = group_length_at + 4 + read_u32(&bytes, group_length_at) as usize;
    let mut offset = group_length_at + 4;
    let mut elements = Vec::new();
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
        elements.push((element, bytes[offset..offset + length].to_vec()));
        offset += length;
    }
    edit(&mut elements);
    let encoded: Vec<u8> = elements.into_iter().flat_map(|(_, bytes)| bytes).collect();
    let length = u32::try_from(encoded.len()).unwrap();
    bytes.splice(group_length_at + 4..group_end, encoded);
    bytes[group_length_at..group_length_at + 4].copy_from_slice(&length.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}

/// Removes group 0002 `elements` from an exported file as encoded.
fn remove_file_meta_elements(path: &Path, elements: &[u16]) {
    rewrite_file_meta(path, |encoded| {
        let before = encoded.len();
        encoded.retain(|(element, _)| !elements.contains(element));
        assert_eq!(
            encoded.len() < before,
            !elements.is_empty(),
            "File Meta elements to remove"
        );
    });
}

/// Adds (0002,0200), which Table 7.1-1 does not define, as `vr` with Value
/// "FUTURE", in ascending tag order.
fn add_undefined_file_meta_tag(path: &Path, vr: [u8; 2]) {
    rewrite_file_meta(path, |encoded| {
        let mut element = vec![0x02, 0x00, 0x00, 0x02, vr[0], vr[1]];
        if vr == *b"UN" {
            element.extend_from_slice(&[0, 0, 6, 0, 0, 0]);
        } else {
            element.extend_from_slice(&[6, 0]);
        }
        element.extend_from_slice(b"FUTURE");
        let at = encoded
            .iter()
            .position(|(number, _)| *number > 0x0200)
            .unwrap_or(encoded.len());
        encoded.insert(at, (0x0200, element));
    });
}

#[test]
fn file_meta_ignores_tags_table_7_1_1_does_not_define_unless_encoded_as_un() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "future-meta-tag");
    add_undefined_file_meta_tag(&path, *b"LO");
    assert_iod_passes(&path, "intrinsic-wsi-dicom-file-meta");

    let path = export_valid_color_wsi_for_validation(tmp.path(), "future-meta-tag-un");
    add_undefined_file_meta_tag(&path, *b"UN");
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-file-meta",
        "(0002,0200): VR UN, which shall not be used for File Meta Information Data Elements",
    );
}

#[test]
fn presentation_addresses_follow_their_transport() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, address, expected) in [
        ("upper-layer", "dicom:127.0.0.1:104", None),
        ("web", "http://myhost.mydomain.com:80/wado-rs/", None),
        (
            "upper-layer-port",
            "dicom:myhost.mydomain.com",
            Some("is not \"dicom:\" followed by a host name or IP address, a colon and a port number"),
        ),
        ("https", "https://myhost/wado-rs/", Some("uses the scheme \"https\"")),
        ("other", "ftp://myhost/", Some("is neither a DICOM Upper Layer address")),
    ] {
        let path = export_valid_color_wsi_for_validation(tmp.path(), name);
        rewrite_file_meta(&path, |elements| {
            let mut value = address.as_bytes().to_vec();
            if value.len() % 2 == 1 {
                value.push(b' ');
            }
            // (0002,0026) Source Presentation Address, UR.
            let mut element = vec![0x02, 0x00, 0x26, 0x00, b'U', b'R', 0, 0];
            element.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
            element.extend_from_slice(&value);
            elements.push((0x0026, element));
            elements.sort_by_key(|(element, _)| *element);
        });
        match expected {
            None => assert_iod_passes(&path, "intrinsic-wsi-dicom-file-meta"),
            Some(expected) => {
                assert_iod_failure(&path, "intrinsic-wsi-dicom-file-meta", expected)
            }
        }
    }
}

#[test]
fn file_meta_is_checked_when_the_data_set_cannot_be_parsed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "no-transfer-syntax");
    // Without Transfer Syntax UID the parser cannot read the Data Set.
    remove_file_meta_elements(&path, &[0x0010]);

    assert_iod_failure(&path, "intrinsic-wsi-dicom-file-meta", "(0002,0010)");
    let pixel_structure = iod_check(
        &path,
        ValidationProfile::General,
        "intrinsic-pixel-structure",
    );
    assert_eq!(pixel_structure.status, ValidationStatus::Failed);
}

#[test]
fn jpip_referenced_files_are_not_ps3_10_media() {
    let tmp = tempfile::tempdir().unwrap();
    for syntax in ["1.2.840.10008.1.2.4.94", "1.2.840.10008.1.2.4.204"] {
        let path = edited_wsi(tmp.path(), &format!("jpip-{syntax}"), |object| {
            object.take(tags::PIXEL_DATA).unwrap();
            object.put(DataElement::new(
                tags::PIXEL_DATA_PROVIDER_URL,
                VR::UR,
                PrimitiveValue::from("https://example.org/jpip?target=slide.jp2"),
            ));
            object.put(DataElement::new(
                tags::PHOTOMETRIC_INTERPRETATION,
                VR::CS,
                PrimitiveValue::from("YBR_RCT"),
            ));
            let meta = object.meta_mut();
            meta.transfer_syntax = syntax.to_string();
            meta.update_information_group_length();
        });
        let report = iod_report(&path, ValidationProfile::General);
        let file_meta = named(&report, "intrinsic-wsi-dicom-file-meta");
        assert!(
            file_meta.status == ValidationStatus::Failed
                && file_meta.message.contains(&format!(
                    "Transfer Syntax UID (0002,0010) \"{syntax}\" is a JPIP Referenced Transfer Syntax, which shall not be used for PS3.10 media storage (PS3.5 2026d 10.8"
                )),
            "{}",
            file_meta.message
        );
        // Otherwise the object is a well-formed JPIP Referenced encoding.
        for name in [
            "intrinsic-wsi-dicom-iod-attribute-presence",
            "intrinsic-pixel-structure",
            "intrinsic-wsi-dicom-image-metadata",
        ] {
            let check = named(&report, name);
            assert_eq!(
                check.status,
                ValidationStatus::Passed,
                "{syntax} {name}: {}",
                check.message
            );
        }
    }

    // The deflated JPIP syntax cannot be parsed here; its File Meta is still checked.
    let path = export_valid_color_wsi_for_validation(tmp.path(), "jpip-deflate");
    let mut bytes = std::fs::read(&path).unwrap();
    let declared = b"1.2.840.10008.1.2.4.90";
    let at = bytes
        .windows(declared.len())
        .position(|window| window == declared)
        .expect("Transfer Syntax UID");
    bytes[at..at + declared.len()].copy_from_slice(b"1.2.840.10008.1.2.4.95");
    std::fs::write(&path, bytes).unwrap();
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-file-meta",
        "\"1.2.840.10008.1.2.4.95\" is a JPIP Referenced Transfer Syntax",
    );
}

#[test]
fn file_meta_is_checked_when_an_unparsed_file_declares_no_sop_class() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "no-sop-class");
    remove_file_meta_elements(&path, &[0x0002, 0x0010]);

    assert_iod_failure(&path, "intrinsic-wsi-dicom-file-meta", "(0002,0002)");
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
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "Referenced Series Sequence (0008,1115): Type 1C missing",
    );
}

/// Statement and scope pairs decidable from the encoded objects that no check
/// enforces yet. It only falls, and it must return to 0 before the bench is
/// rebuilt; every family is at 0.
const UNCHECKED_LIMIT: usize = 0;

#[test]
fn requirement_catalog_names_only_emitted_rules_and_unchecked_statements_only_fall() {
    use crate::validation::wsi_conformance::DICOMWEB_RULES;
    use crate::validation::{requirement_catalog, RequirementDisposition};

    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("associated.svs");
    crate::test_support::write_aperio_svs_with_associated_images(&source, false);
    let output = tmp.path().join("out");
    crate::export_dicom(crate::ExportRequest {
        source_path: source,
        output_dir: output.clone(),
        options: crate::ExportOptions {
            tile_size: 32,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            ..crate::ExportOptions::default()
        },
        color_management: crate::ColorManagement::SourceOrSrgb,
        metadata: crate::MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();
    // An object of each other IOD, for the checks that hold only theirs.
    pathology::write_fixtures(&output);
    structured_report::write_documents(&output);
    // A File-set, for the File-set check.
    crate::validation::file_set::tests::write_empty_file_set(&output.join("FILESET"));
    let emitted: BTreeSet<String> = [ValidationProfile::General, ValidationProfile::Core]
        .into_iter()
        .flat_map(|profile| iod_report(&output, profile).checks)
        .map(|check| check.name)
        .collect();

    let catalog = requirement_catalog();
    let mut seen = BTreeSet::new();
    let mut unchecked = 0;
    for entry in &catalog {
        assert!(
            seen.insert((entry.part, entry.section, entry.keyword, entry.text)),
            "duplicate statement {entry:?}"
        );
        assert!(!entry.iods.is_empty(), "{entry:?} applies to no IOD");
        assert!(
            entry
                .iod_dispositions
                .keys()
                .all(|iod| entry.iods.contains(iod) && *iod != "A.32.8"),
            "{entry:?} has a disposition for an IOD it does not apply to, or for VL WSI"
        );
        for iod in &entry.iods {
            let disposition = entry
                .iod_dispositions
                .get(iod)
                .copied()
                .unwrap_or(entry.disposition);
            match disposition {
                // The DICOMweb checks run against an origin server, and a
                // DICOMweb test proves that such a run emits each of them.
                RequirementDisposition::Checked(rule) if *iod == "dicomweb" => {
                    assert!(
                        DICOMWEB_RULES.contains(&rule),
                        "{rule} is not a DICOMweb check: {entry:?}"
                    );
                }
                RequirementDisposition::Checked(rule) => {
                    assert!(emitted.contains(rule), "{rule} is not emitted: {entry:?}");
                }
                RequirementDisposition::Unchecked(_) => unchecked += 1,
                RequirementDisposition::OutsideScope(reason)
                | RequirementDisposition::Behavior(reason)
                | RequirementDisposition::Undecidable(reason) => {
                    assert!(!reason.is_empty(), "{entry:?}");
                }
                RequirementDisposition::Tables => {}
            }
        }
    }
    match unchecked.cmp(&UNCHECKED_LIMIT) {
        std::cmp::Ordering::Greater => {
            panic!("{unchecked} unchecked statement and IOD pairs exceed the limit of {UNCHECKED_LIMIT}")
        }
        std::cmp::Ordering::Less => panic!("lower UNCHECKED_LIMIT to {unchecked}"),
        std::cmp::Ordering::Equal => {}
    }
}

#[test]
fn empty_values_and_empty_type_three_sequences_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "empty-image-type-value", |object| {
        object.put(DataElement::new(
            tags::IMAGE_TYPE,
            VR::CS,
            PrimitiveValue::Strs(
                ["", "PRIMARY", "VOLUME", "NONE"]
                    .map(String::from)
                    .into_iter()
                    .collect(),
            ),
        ));
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-enumerated-values",
        "Image Type (0008,0008) Value 1 \"\" is not an Enumerated Value [ORIGINAL, DERIVED]",
    );
    let path = edited_wsi(tmp.path(), "empty-type-three-sequence", |object| {
        let mut specimens = object.take(tags::SPECIMEN_DESCRIPTION_SEQUENCE).unwrap();
        specimens.items_mut().unwrap()[0].put(DataElement::new(
            tags::SPECIMEN_TYPE_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(Vec::<InMemDicomObject>::new()),
        ));
        object.put(specimens);
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-sequence-items",
        "Specimen Description Sequence[1] > Specimen Type Code Sequence (0040,059A): Type 3 Sequence present with zero Items (PS3.5 7.4.5",
    );
}

const VALUE_SETS: &str = "intrinsic-wsi-dicom-iod-value-sets";

fn advisories(path: &Path) -> Vec<String> {
    iod_report(path, ValidationProfile::General)
        .optional
        .into_iter()
        .flat_map(|optional| optional.advisories)
        .collect()
}

fn assert_advises(path: &Path, expected: &str) {
    let advisories = advisories(path);
    assert!(
        advisories
            .iter()
            .any(|advisory| advisory.contains(expected)),
        "no advisory contains {expected:?}: {advisories:?}"
    );
}

fn assert_does_not_advise(path: &Path, unexpected: &str) {
    let advisories = advisories(path);
    assert!(
        !advisories
            .iter()
            .any(|advisory| advisory.contains(unexpected)),
        "an advisory contains {unexpected:?}: {advisories:?}"
    );
}

/// A Real World Value Mapping Sequence mapping stored values to `units`.
fn value_mapping(units: InMemDicomObject) -> InMemElement {
    sq(
        tags::REAL_WORLD_VALUE_MAPPING_SEQUENCE,
        vec![item(vec![
            text(tags::LUT_EXPLANATION, VR::LO, "Optical density"),
            text(tags::LUT_LABEL, VR::SH, "OD"),
            DataElement::new(
                tags::REAL_WORLD_VALUE_FIRST_VALUE_MAPPED,
                VR::US,
                PrimitiveValue::from(0_u16),
            ),
            DataElement::new(
                tags::REAL_WORLD_VALUE_LAST_VALUE_MAPPED,
                VR::US,
                PrimitiveValue::from(255_u16),
            ),
            DataElement::new(
                tags::REAL_WORLD_VALUE_INTERCEPT,
                VR::FD,
                PrimitiveValue::from(0_f64),
            ),
            DataElement::new(
                tags::REAL_WORLD_VALUE_SLOPE,
                VR::FD,
                PrimitiveValue::from(0.01_f64),
            ),
            sq(tags::MEASUREMENT_UNITS_CODE_SEQUENCE, vec![units]),
        ])],
    )
}

/// `code` declared as an extension of its Context Group (PS3.3 8.7).
fn extension(mut code: InMemDicomObject) -> InMemDicomObject {
    code.put(text(tags::CONTEXT_GROUP_EXTENSION_FLAG, VR::CS, "Y"));
    code.put(text(tags::CONTEXT_GROUP_LOCAL_VERSION, VR::DT, "20260101"));
    code.put(text(
        tags::CONTEXT_GROUP_EXTENSION_CREATOR_UID,
        VR::UI,
        "1.2.826.0.1.3680043.10.999.930",
    ));
    code
}

/// An export whose Shared Functional Groups Item maps values to `units`
/// (Real World Value Mapping Functional Group: CID 82, Defined, not
/// extensible).
fn mapped_frames(root: &Path, name: &str, units: InMemDicomObject) -> PathBuf {
    edited_wsi(root, name, |object| {
        first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
            shared.put(value_mapping(units));
        });
    })
}

/// An export whose General Image Module maps values to `units` (CID 7181,
/// Defined, extensible).
fn mapped_image(root: &Path, name: &str, units: InMemDicomObject) -> PathBuf {
    edited_wsi(root, name, |object| {
        object.put(value_mapping(units));
    })
}

fn specimen_type(root: &Path, name: &str, specimen: InMemDicomObject) -> PathBuf {
    edited_wsi(root, name, |object| {
        first_item(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |description| {
            description.put(sq(tags::SPECIMEN_TYPE_CODE_SEQUENCE, vec![specimen]));
        });
    })
}

#[test]
fn context_groups_fail_or_advise_by_their_strength() {
    let tmp = tempfile::tempdir().unwrap();
    let local_unit = || code("99LOCAL", "mm", "millimeter");
    let not_in_ucum = "Shared Functional Groups Sequence[1] > Real World Value Mapping Sequence[1] > Measurement Units Code Sequence (0040,08EA)[1]: (mm, 99LOCAL, \"millimeter\") is not in Defined CID 82 \"Measurement Unit\"";
    let path = mapped_frames(tmp.path(), "unit-outside-ucum", local_unit());
    assert_iod_failure(&path, VALUE_SETS, not_in_ucum);
    // A declared extension does not extend a non-extensible group.
    let path = mapped_frames(tmp.path(), "unit-extending-ucum", extension(local_unit()));
    assert_iod_failure(&path, VALUE_SETS, not_in_ucum);
    // CID 82 comprises UCUM whole.
    let path = mapped_frames(tmp.path(), "ucum-unit", code("UCUM", "mm", "millimeter"));
    assert_iod_passes(&path, VALUE_SETS);

    let undeclared = "Measurement Units Code Sequence (0040,08EA)[1]: (mm, 99LOCAL, \"millimeter\") is not in Defined CID 7181";
    let path = mapped_image(tmp.path(), "undeclared-extension", local_unit());
    assert_iod_passes(&path, VALUE_SETS);
    assert_advises(&path, undeclared);
    assert_advises(
        &path,
        "declared with Context Group Extension Flag (0008,010B) \"Y\"",
    );
    let path = mapped_image(tmp.path(), "declared-extension", extension(local_unit()));
    assert_iod_passes(&path, VALUE_SETS);
    assert_does_not_advise(&path, "CID 7181");

    let path = specimen_type(
        tmp.path(),
        "local-specimen-type",
        code("99LOCAL", "T1", "Local specimen type"),
    );
    assert_iod_passes(&path, VALUE_SETS);
    assert_advises(
        &path,
        "Specimen Type Code Sequence (0040,059A)[1]: (T1, 99LOCAL, \"Local specimen type\") is not in Baseline CID 8103 \"Anatomic Pathology Specimen Type\"",
    );
    // The retired SRT designator matches by the SNOMED-RT identifier.
    let path = specimen_type(
        tmp.path(),
        "legacy-specimen-type",
        code("SRT", "G-8300", "Tissue specimen"),
    );
    assert_does_not_advise(&path, "CID 8103");
}

/// A Content Item of `value_type` named by `name`, with its value Attributes.
fn content_item(
    value_type: &str,
    name: InMemDicomObject,
    mut value: Vec<InMemElement>,
) -> InMemDicomObject {
    value.push(text(tags::VALUE_TYPE, VR::CS, value_type));
    value.push(sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![name]));
    item(value)
}

fn text_item(scheme: &str, value: &str, meaning: &str, text_value: &str) -> InMemDicomObject {
    content_item(
        "TEXT",
        code(scheme, value, meaning),
        vec![text(tags::TEXT_VALUE, VR::UT, text_value)],
    )
}

fn location() -> InMemDicomObject {
    text_item("DCM", "111718", "Location of Specimen", "upper left")
}

fn x_offset() -> InMemDicomObject {
    content_item(
        "NUMERIC",
        code("DCM", "111719", "Location of Specimen X offset"),
        vec![sq(
            tags::MEASURED_VALUE_SEQUENCE,
            vec![item(vec![
                text(tags::NUMERIC_VALUE, VR::DS, "1.5"),
                sq(
                    tags::MEASUREMENT_UNITS_CODE_SEQUENCE,
                    vec![code("UCUM", "mm", "millimeter")],
                ),
            ])],
        )],
    )
}

fn localized(root: &Path, name: &str, content: Vec<InMemDicomObject>) -> PathBuf {
    edited_wsi(root, name, |object| {
        first_item(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |specimen| {
            specimen.put(sq(
                tags::SPECIMEN_LOCALIZATION_CONTENT_ITEM_SEQUENCE,
                content,
            ));
        });
    })
}

#[test]
fn defined_templates_fail_row_count_and_order_departures() {
    let tmp = tempfile::tempdir().unwrap();
    let template =
        "Specimen Localization Content Item Sequence: TID 8004 \"Specimen Localization\"";
    let path = localized(tmp.path(), "two-locations", vec![location(), location()]);
    assert_iod_failure(
        &path,
        VALUE_SETS,
        &format!("{template} Row 2 TEXT (111718, DCM, \"Location of Specimen\") appears 2 times"),
    );
    let path = localized(tmp.path(), "offset-first", vec![x_offset(), location()]);
    assert_iod_failure(
        &path,
        VALUE_SETS,
        &format!("{template} Content Items are not in Template row order"),
    );
    // TID 8004 is extensible: an Item no row names neither fails nor breaks
    // the row order.
    let comment = text_item("DCM", "121106", "Comment", "near the label");
    let path = localized(
        tmp.path(),
        "extended",
        vec![location(), comment, x_offset()],
    );
    assert_iod_passes(&path, VALUE_SETS);
}

#[test]
fn baseline_templates_only_advise() {
    let tmp = tempfile::tempdir().unwrap();
    let step = text_item("DCM", "121041", "Specimen Identifier", "S1");
    let path = edited_wsi(
        tmp.path(),
        "preparation-without-processing-type",
        |object| {
            first_item(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |specimen| {
                specimen.put(sq(
                    tags::SPECIMEN_PREPARATION_SEQUENCE,
                    vec![item(vec![sq(
                        tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
                        vec![step],
                    )])],
                ));
            });
        },
    );
    assert_iod_passes(&path, VALUE_SETS);
    assert_advises(
        &path,
        "TID 8001 \"Specimen Preparation\" Row 3 CODE (111701, DCM, \"Processing type\") missing",
    );
}

/// A VL WSI whose Specimen Preparation step holds its Processing type alone,
/// without the Specimen Identifier TID 8001 Row 1 requires, and whose
/// De-identification Method Code Sequence lists `methods`.
fn unidentified_preparation(root: &Path, name: &str, methods: Vec<InMemDicomObject>) -> PathBuf {
    let processing = content_item(
        "CODE",
        code("DCM", "111701", "Processing type"),
        vec![sq(
            tags::CONCEPT_CODE_SEQUENCE,
            vec![code("SCT", "9265001", "Specimen processing")],
        )],
    );
    edited_wsi(root, name, |object| {
        object.put(text(tags::PATIENT_IDENTITY_REMOVED, VR::CS, "YES"));
        object.put(sq(tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE, methods));
        first_item(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |specimen| {
            specimen.put(sq(
                tags::SPECIMEN_PREPARATION_SEQUENCE,
                vec![item(vec![sq(
                    tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
                    vec![processing],
                )])],
            ));
        });
    })
}

#[test]
fn a_specimen_identifier_clean_structured_content_removes_is_advised_citing_both_parts() {
    let tmp = tempfile::tempdir().unwrap();
    let edition = crate::validation::iod::EDITION;
    let missing = "TID 8001 \"Specimen Preparation\" Row 1 TEXT (121041, DCM, \"Specimen Identifier\") missing";
    let basic = || code("DCM", "113100", "Basic Application Confidentiality Profile");
    let cleaned = unidentified_preparation(
        tmp.path(),
        "cleaned",
        vec![
            basic(),
            code("DCM", "113104", "Clean Structured Content Option"),
        ],
    );
    assert_iod_passes(&cleaned, VALUE_SETS);
    assert_advises(
        &cleaned,
        &format!(
            "{missing}, a Content Item PS3.15 {edition} Table E.3.4-1 removes under the Clean Structured Content Option the De-identification Method Code Sequence (0012,0064) declares (PS3.15 {edition} E.3.4; PS3.16 {edition} TID 8001)"
        ),
    );
    // Without the Option, TID 8001 is a Baseline Template: the missing row is
    // its suggestion alone.
    let basic_only = unidentified_preparation(tmp.path(), "basic", vec![basic()]);
    assert_iod_passes(&basic_only, VALUE_SETS);
    assert_advises(&basic_only, &format!("{missing} (PS3.16 {edition})"));
    assert_does_not_advise(&basic_only, "Table E.3.4-1");
}

/// The ICC Profile (0028,2000) of the first Optical Path Item.
fn optical_path_profile(object: &mut InMemDicomObject, edit: impl FnOnce(&mut Vec<u8>)) {
    first_item(object, tags::OPTICAL_PATH_SEQUENCE, |path| {
        let mut bytes = path
            .get(tags::ICC_PROFILE)
            .unwrap()
            .to_bytes()
            .unwrap()
            .to_vec();
        edit(&mut bytes);
        path.put(DataElement::new(
            tags::ICC_PROFILE,
            VR::OB,
            PrimitiveValue::from(bytes),
        ));
    });
}

#[test]
fn icc_profile_recommendations_are_advised_and_never_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let edition = crate::validation::iod::EDITION;
    let exported = edited_wsi(tmp.path(), "exported", |_| {});
    assert_does_not_advise(&exported, "C.11.15.1.1");
    // C.11.15.1.1: "The Rendering Intent should be Perceptual."
    let path = edited_wsi(tmp.path(), "colorimetric", |object| {
        optical_path_profile(object, |bytes| bytes[67] = 1);
    });
    assert_iod_passes(&path, "intrinsic-wsi-dicom-iod-value-format");
    assert_advises(
        &path,
        &format!(
            "OpticalPathSequence (0048,0105)[1] > ICCProfile (0028,2000): the ICC Profile's Rendering Intent is Media-Relative Colorimetric (1); PS3.3 {edition} C.11.15.1.1 recommends Perceptual (0)"
        ),
    );
    // "All LUTs should be represented as 16 bit values, using tag type
    // lut16Type": the first tag becomes an AToB0 LUT of type lut8Type.
    let path = edited_wsi(tmp.path(), "lut8", |object| {
        optical_path_profile(object, |bytes| {
            let offset = u32::from_be_bytes(bytes[136..140].try_into().unwrap()) as usize;
            bytes[132..136].copy_from_slice(b"A2B0");
            bytes[offset..offset + 4].copy_from_slice(b"mft1");
        });
    });
    assert_iod_passes(&path, "intrinsic-wsi-dicom-iod-value-format");
    assert_advises(&path, "A2B0 tag is of type \"mft1\"");
}

#[test]
fn a_zero_stride_of_private_value_multiplicity_fails_the_value_format() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "zero-stride", |object| {
        object.put(DataElement::new(
            tags::PRIVATE_DATA_ELEMENT_VALUE_MULTIPLICITY,
            VR::UL,
            PrimitiveValue::U32([2u32, 6, 0].into_iter().collect()),
        ));
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-value-format",
        "PrivateDataElementValueMultiplicity (0008,0309): the stride (third Value) is 0, which is not permitted (PS3.3",
    );
    let path = edited_wsi(tmp.path(), "stride-two", |object| {
        object.put(DataElement::new(
            tags::PRIVATE_DATA_ELEMENT_VALUE_MULTIPLICITY,
            VR::UL,
            PrimitiveValue::U32([2u32, 6, 2].into_iter().collect()),
        ));
    });
    assert_iod_passes(&path, "intrinsic-wsi-dicom-iod-value-format");
}

/// A VL WSI whose Shared Functional Group Item holds the Private Functional
/// Group (0009,1001) of creator "ACME PRIVATE", one Item of `elements`.
fn with_private_group(root: &Path, name: &str, elements: Vec<InMemElement>) -> PathBuf {
    edited_wsi(root, name, |object| {
        first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
            shared.put(text(Tag(0x0009, 0x0010), VR::LO, "ACME PRIVATE"));
            shared.put(sq(Tag(0x0009, 0x1001), vec![item(elements)]));
        });
    })
}

#[test]
fn a_private_functional_group_may_not_replicate_a_standard_one() {
    let tmp = tempfile::tempdir().unwrap();
    let edition = crate::validation::iod::EDITION;
    let path = with_private_group(
        tmp.path(),
        "private-pixel-spacing",
        vec![
            text(Tag(0x0009, 0x0010), VR::LO, "ACME PRIVATE"),
            text(tags::PIXEL_SPACING, VR::DS, "0.00025\\0.00025"),
        ],
    );
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-modules",
        &format!(
            "Shared Functional Groups Sequence[1] > (0009,1001) (Private Creator \"ACME PRIVATE\")[1] > Pixel Spacing (0028,0030): a Private Functional Group replicates an Attribute of the Pixel Measures Functional Group Macro, which it may not (PS3.3 {edition} C.7.6.16.1.1)"
        ),
    );
    // Private Attributes alone replicate nothing.
    let path = with_private_group(
        tmp.path(),
        "private-only",
        vec![
            text(Tag(0x0009, 0x0010), VR::LO, "ACME PRIVATE"),
            text(Tag(0x0009, 0x1002), VR::LO, "focus map 3"),
        ],
    );
    assert_iod_passes(&path, "intrinsic-wsi-dicom-iod-modules");
}

#[test]
fn the_top_level_data_set_holds_one_pixel_data_attribute() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "float-and-integer", |object| {
        object.put(DataElement::new(
            tags::FLOAT_PIXEL_DATA,
            VR::OF,
            PrimitiveValue::F32([0.0f32; 4].into_iter().collect()),
        ));
    });
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "(7FE0,0010), (7FE0,0008) are present; the top level Data Set holds at most one of Pixel Data Provider URL, Pixel Data, Float Pixel Data and Double Float Pixel Data (PS3.5",
    );
}

#[test]
fn the_catalog_reports_each_statement_s_strength() {
    use crate::validation::{requirement_catalog, RequirementStrength};
    let catalog = requirement_catalog();
    let strength = |id: &str| {
        catalog
            .iter()
            .find(|entry| entry.id == Some(id))
            .unwrap_or_else(|| panic!("no statement {id}"))
            .strength
    };
    assert_eq!(
        strength("Ps3_C_11_15_1_1_108351"),
        RequirementStrength::Should
    );
    assert_eq!(strength("Ps5_6_2_b82926"), RequirementStrength::Shall);
    assert_eq!(strength("Ps3_10_14_cb9917"), RequirementStrength::Shall);
    let recommendations: Vec<_> = catalog
        .iter()
        .filter(|entry| entry.strength == RequirementStrength::Should)
        .collect();
    assert_eq!(recommendations.len(), 60);
    // No recommendation is a normative sentence.
    assert!(recommendations
        .iter()
        .all(|entry| !entry.text.contains("shall")));
    let json = serde_json::to_value(recommendations[0]).unwrap();
    assert_eq!(json["strength"], "should");
    let json = serde_json::to_value(&catalog[0]).unwrap();
    assert_eq!(json["strength"], "shall");
}

mod file_set;
mod pathology;
mod structured_report;
