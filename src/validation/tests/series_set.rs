//! Relations between the objects of a validated directory, each failed by a
//! two-object set made from one export.

use super::*;
use dicom_object::DefaultDicomObject;

const SERIES_SET: &str = "intrinsic-wsi-dicom-series-set";

/// A directory with an exported WSI and a copy that is another SOP Instance
/// of the same Series, with `edit` applied to the copy.
fn pair(root: &Path, name: &str, edit: impl FnOnce(&mut DefaultDicomObject)) -> PathBuf {
    let directory = root.join(name);
    std::fs::create_dir_all(&directory).unwrap();
    let original = export_valid_color_wsi_for_validation(root, name);
    std::fs::copy(&original, directory.join("a.dcm")).unwrap();
    let copy = directory.join("b.dcm");
    std::fs::copy(&original, &copy).unwrap();
    edited(copy, |object| {
        let mut uid = object
            .element(tags::SOP_INSTANCE_UID)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let last = uid.pop().unwrap();
        uid.push(if last == '0' { '1' } else { '0' });
        object.put(text(tags::SOP_INSTANCE_UID, VR::UI, &uid));
        object.meta_mut().media_storage_sop_instance_uid = uid;
        object.put(text(tags::INSTANCE_NUMBER, VR::IS, "77"));
        edit(object);
    });
    directory
}

fn series_set(path: &Path) -> (ValidationStatus, String) {
    rule_result(path, SERIES_SET)
}

fn set_concatenation_member(
    object: &mut DefaultDicomObject,
    number: u16,
    offset: u32,
    frames: i64,
) {
    object.put(text(
        tags::CONCATENATION_UID,
        VR::UI,
        "1.2.826.0.1.3680043.10.999.906",
    ));
    object.put(DataElement::new(
        tags::IN_CONCATENATION_NUMBER,
        VR::US,
        PrimitiveValue::from(number),
    ));
    object.put(DataElement::new(
        tags::IN_CONCATENATION_TOTAL_NUMBER,
        VR::US,
        PrimitiveValue::from(4u16),
    ));
    object.put(DataElement::new(
        tags::CONCATENATION_FRAME_OFFSET_NUMBER,
        VR::UL,
        PrimitiveValue::from(offset),
    ));
    object.put(text(tags::NUMBER_OF_FRAMES, VR::IS, &frames.to_string()));
    object.put(text(tags::INSTANCE_NUMBER, VR::IS, "1"));
}

fn concatenation_pair(root: &Path, name: &str, members: [(u16, u32, i64); 2]) -> PathBuf {
    let [(first_number, first_offset, first_frames), (second_number, second_offset, second_frames)] =
        members;
    let directory = pair(root, name, move |object| {
        set_concatenation_member(object, second_number, second_offset, second_frames);
    });
    edited(directory.join("a.dcm"), move |object| {
        set_concatenation_member(object, first_number, first_offset, first_frames);
    });
    directory
}

#[test]
fn a_consistent_pair_passes_the_series_set_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let (status, message) = series_set(&pair(tmp.path(), "consistent", |_| {}));
    assert_eq!(status, ValidationStatus::Passed, "{message}");
}

#[test]
fn an_individual_later_concatenation_member_passes_the_series_set_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let object = export_valid_color_wsi_for_validation(tmp.path(), "later-concatenation-member");
    edited(object.clone(), |object| {
        object.put(text(
            tags::CONCATENATION_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.905",
        ));
        object.put(DataElement::new(
            tags::IN_CONCATENATION_NUMBER,
            VR::US,
            PrimitiveValue::from(2u16),
        ));
        object.put(DataElement::new(
            tags::IN_CONCATENATION_TOTAL_NUMBER,
            VR::US,
            PrimitiveValue::from(2u16),
        ));
        object.put(DataElement::new(
            tags::CONCATENATION_FRAME_OFFSET_NUMBER,
            VR::UL,
            PrimitiveValue::from(1u32),
        ));
    });

    let (status, message) = series_set(&object);
    assert_eq!(status, ValidationStatus::Passed, "{message}");
}

#[test]
fn partial_concatenation_members_validate_observed_relationships() {
    let tmp = tempfile::tempdir().unwrap();
    let valid = concatenation_pair(tmp.path(), "adjacent-later-members", [(2, 1, 1), (3, 2, 1)]);
    let (status, message) = series_set(&valid);
    assert_eq!(status, ValidationStatus::Passed, "{message}");

    for (name, members, expected) in [
        (
            "first-member-offset",
            [(1, 1, 1), (3, 3, 1)],
            "Number 1 but has Concatenation Frame Offset Number Some(1) instead of 0",
        ),
        (
            "later-member-zero-offset",
            [(2, 0, 1), (4, 3, 1)],
            "each earlier member has at least one Frame",
        ),
        (
            "overlapping-nonadjacent-member",
            [(1, 0, 10), (3, 1, 1)],
            "overlaps the Frames",
        ),
        (
            "duplicate-number",
            [(2, 1, 1), (2, 2, 1)],
            "Numbers and Concatenation Frame Offset Numbers are not strictly increasing",
        ),
        (
            "reversed-number",
            [(2, 2, 1), (3, 1, 1)],
            "Numbers and Concatenation Frame Offset Numbers are not strictly increasing",
        ),
        (
            "noncontiguous-offset",
            [(2, 1, 1), (3, 3, 1)],
            "inconsistent with adjacent member",
        ),
        (
            "overflowing-frame-range",
            [(2, 1, i64::MAX), (3, 2, 1)],
            "cannot be added to Concatenation Frame Offset Number",
        ),
    ] {
        let (status, message) = series_set(&concatenation_pair(tmp.path(), name, members));
        assert_eq!(status, ValidationStatus::Failed, "{name}: {message}");
        assert!(message.contains(expected), "{name}: {message}");
    }
}

#[test]
fn series_set_relations_fail_their_violations() {
    let cases: [(&str, Edit, &str); 7] = [
        (
            "frame-of-reference",
            |object| {
                object.put(text(
                    tags::FRAME_OF_REFERENCE_UID,
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.901",
                ));
            },
            "each Series has a single one",
        ),
        (
            "slide",
            |object| {
                object.put(text(tags::CONTAINER_IDENTIFIER, VR::LO, "OTHER-SLIDE"));
            },
            "are spatially related to one slide",
        ),
        (
            "uid-role",
            |object| {
                let study = object
                    .element(tags::STUDY_INSTANCE_UID)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string();
                object.put(text(tags::FRAME_OF_REFERENCE_UID, VR::UI, &study));
            },
            "is both a Study Instance UID",
        ),
        (
            "instance-number",
            |object| {
                object.put(text(tags::INSTANCE_NUMBER, VR::IS, "1"));
            },
            "it is shared only within a Concatenation",
        ),
        (
            "dimension-organization",
            |object| {
                let mut items = object.take(tags::DIMENSION_INDEX_SEQUENCE).unwrap();
                items.items_mut().unwrap().truncate(1);
                object.put(items);
            },
            "indexes different Attributes",
        ),
        (
            "specimen",
            |object| {
                let mut specimens = object.take(tags::SPECIMEN_DESCRIPTION_SEQUENCE).unwrap();
                specimens.items_mut().unwrap()[0].put(text(
                    tags::SPECIMEN_UID,
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.902",
                ));
                object.put(specimens);
            },
            "names Specimen UIDs",
        ),
        (
            "concatenation-total",
            |object| {
                object.put(text(
                    tags::CONCATENATION_UID,
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.903",
                ));
                object.put(DataElement::new(
                    tags::IN_CONCATENATION_TOTAL_NUMBER,
                    VR::US,
                    PrimitiveValue::from(1u16),
                ));
            },
            "In-concatenation Total Number shall be greater than one",
        ),
    ];
    let tmp = tempfile::tempdir().unwrap();
    for (name, edit, expected) in cases {
        let (status, message) = series_set(&pair(tmp.path(), name, edit));
        assert_eq!(status, ValidationStatus::Failed, "{name}: {message}");
        assert!(message.contains(expected), "{name}: {message}");
    }
}

#[test]
fn concatenation_members_must_agree_and_number_in_frame_order() {
    let tmp = tempfile::tempdir().unwrap();
    let directory = pair(tmp.path(), "concatenation", |object| {
        object.put(text(
            tags::CONCATENATION_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.904",
        ));
        object.put(DataElement::new(
            tags::IN_CONCATENATION_NUMBER,
            VR::US,
            PrimitiveValue::from(1u16),
        ));
        object.put(DataElement::new(
            tags::CONCATENATION_FRAME_OFFSET_NUMBER,
            VR::UL,
            PrimitiveValue::from(0u32),
        ));
        object.put(text(tags::INSTANCE_NUMBER, VR::IS, "1"));
    });
    edited(directory.join("a.dcm"), |object| {
        object.put(text(
            tags::CONCATENATION_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.904",
        ));
        object.put(DataElement::new(
            tags::IN_CONCATENATION_NUMBER,
            VR::US,
            PrimitiveValue::from(1u16),
        ));
        object.put(DataElement::new(
            tags::CONCATENATION_FRAME_OFFSET_NUMBER,
            VR::UL,
            PrimitiveValue::from(1u32),
        ));
        object.put(text(tags::SERIES_DESCRIPTION, VR::LO, "changed"));
    });
    let (status, message) = series_set(&directory);
    assert_eq!(status, ValidationStatus::Failed, "{message}");
    for expected in [
        "Attributes outside the per-instance ones differs",
        "Numbers and Concatenation Frame Offset Numbers are not strictly increasing",
    ] {
        assert!(message.contains(expected), "{expected}: {message}");
    }
}
