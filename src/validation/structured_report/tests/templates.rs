//! TID 2010 at a KOS root, and the Template an SR document declares (TID
//! 1500): rows, conditions, counts, extensibility, order, Value Sets, and
//! the declaration itself.

use dicom_core::VR;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::fixtures::*;
use super::{assert_fails, assert_passes, check};

#[test]
fn a_kos_root_identifies_tid_2010() {
    let mut object = key_object_selection();
    object.remove_element(tags::CONTENT_TEMPLATE_SEQUENCE);
    assert_fails(
        object,
        "the root CONTAINER has no Content Template Sequence (0040,A504) identifying TID 2010, which the document shall be constructed from at the root node; created using such a Template, the Root shall identify the Template in the CONTAINER Content Item (PS3.3 2026d A.35.4.3.1.3 and C.18.8.1.2)",
    );
    let mut object = key_object_selection();
    object.put(template_sequence("1500"));
    assert_fails(
        object,
        "the root CONTAINER's Content Template Sequence (0040,A504) declares Template 1500 of Mapping Resource DCMR; the document shall be constructed from TID 2010 invoked at the root node",
    );
}

#[test]
fn a_template_identifier_is_digits_without_leading_zeroes() {
    for identifier in ["TID 2010", "02010"] {
        let mut object = key_object_selection();
        object.put(template_sequence(identifier));
        assert_fails(
            object,
            &format!("the root Content Item > Content Template Sequence > Template Identifier (0040,DB00) is \"{identifier}\"; the DICOM Template Identifier is a string of digits, without leading zeroes, and does not include the string \"TID\" (PS3.3 2026d C.18.8.1.2)"),
        );
    }
}

#[test]
fn a_kos_title_is_from_cid_7010() {
    // CID 7010 is extensible: a title outside it is an undeclared
    // extension, advised (PS3.16 7.2.2); a declared one is not.
    let mut object = key_object_selection();
    object.put(sq(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        vec![code("DCM", "126000", "Imaging Measurement Report")],
    ));
    let check = assert_passes(object.clone());
    assert!(
        check.message.contains("advisory: the root Content Item > Concept Name Code Sequence (0040,A043): (126000, DCM, \"Imaging Measurement Report\") is not in Defined CID 7010 \"Key Object Selection Document Title\""),
        "{}",
        check.message
    );
    let mut title = code("DCM", "126000", "Imaging Measurement Report");
    title.put(text(tags::CONTEXT_GROUP_EXTENSION_FLAG, VR::CS, "Y"));
    object.put(sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![title]));
    let check = assert_passes(object);
    assert!(!check.message.contains("CID 7010"), "{}", check.message);
}

#[test]
fn kos_references_carry_no_purpose_of_reference() {
    let named = |purpose: InMemDicomObject| {
        let mut object = key_object_selection();
        put_at(
            &mut object,
            &[1],
            sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![purpose]),
        );
        object
    };
    assert_fails(
        named(code("DCM", "121200", "Illustration of ROI")),
        "Content Sequence[2]: TID 2010 \"Key Object Selection\" Row 8 IMAGE has Concept Name Code Sequence (0040,A043) (121200, DCM, \"Illustration of ROI\"); for Rows 8, 9 and 10, purpose of reference shall not be present",
    );
    // The generic name highdicom writes, which Table C.17-5 allows.
    let check = assert_passes(named(code("SCT", "260753009", "Source")));
    assert!(
        check.message.contains("advisory: Content Sequence[2]: TID 2010 \"Key Object Selection\" Row 8 IMAGE has Concept Name Code Sequence (0040,A043) (260753009, SCT, \"Source\"); for Rows 8, 9 and 10, purpose of reference shall not be present (PS3.16 2026d); but PS3.3 2026d Table C.17-5 allows this generic Concept Name where a Template does not require the Purpose of Reference: the two texts are in tension, so it is advised, not failed"),
        "{}",
        check.message
    );
}

#[test]
fn a_kos_references_at_least_one_instance() {
    let mut object = key_object_selection();
    set_children(&mut object, &[], vec![]);
    assert_fails(
        object,
        "the root Content Item: TID 2010 \"Key Object Selection\" at least one of Rows 8, 9, 10 shall be present; none is",
    );
}

#[test]
fn nothing_may_be_added_to_tid_2010() {
    // Table A.35.4-2 permits HAS ACQ CONTEXT on the root and on an IMAGE;
    // TID 2010 has no such row, and is not extensible, recursively.
    let acquisition = || {
        content(
            "HAS ACQ CONTEXT",
            "TEXT",
            Some(code("DCM", "121106", "Comment")),
            vec![text(tags::TEXT_VALUE, VR::UT, "scanner")],
        )
    };
    let mut object = key_object_selection();
    push_child(&mut object, &[], acquisition());
    assert_fails(
        object,
        "Content Sequence[3]: TID 2010 \"Key Object Selection\" Content Item TEXT (121106, DCM, \"Comment\") matches no row of TID 2010 \"Key Object Selection\", to which no Content Items may be added",
    );
    let mut object = key_object_selection();
    push_child(&mut object, &[1], acquisition());
    assert_fails(object, "Content Sequence[2] > Content Sequence[1]: TID 2010 \"Key Object Selection\" Content Item TEXT");
}

#[test]
fn a_best_in_set_kos_has_its_title_modifier() {
    let mut object = key_object_selection();
    object.put(sq(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        vec![code("DCM", "113013", "Best In Set")],
    ));
    assert_fails(
        object,
        "the root Content Item: TID 2010 \"Key Object Selection\" Row 4 CODE (113011, DCM, \"Document Title Modifier\") missing",
    );
}

#[test]
fn a_rejection_kos_may_give_its_reason() {
    let mut object = key_object_selection();
    object.put(sq(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        vec![code("DCM", "113001", "Rejected for Quality Reasons")],
    ));
    push_child(
        &mut object,
        &[],
        content(
            "HAS CONCEPT MOD",
            "CODE",
            Some(code("DCM", "113011", "Document Title Modifier")),
            vec![sq(
                tags::CONCEPT_CODE_SEQUENCE,
                vec![code("DCM", "111219", "Inappropriate image processing")],
            )],
        ),
    );
    assert_passes(object);
}

#[test]
fn a_manifest_with_description_includes_an_image_library() {
    let mut object = key_object_selection();
    object.put(sq(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        vec![code("DCM", "131560", "Manifest with Description")],
    ));
    assert_fails(
        object,
        "the root Content Item: TID 2010 \"Key Object Selection\" TID 1600 Row 1 CONTAINER (111028, DCM, \"Image Library\") missing",
    );
    // An Image Library in another KOS is forbidden (IFF).
    let mut object = key_object_selection();
    push_child(&mut object, &[], super::document::image_library(&[IMAGE]));
    assert_fails(
        object,
        "TID 1600 Row 1 CONTAINER (111028, DCM, \"Image Library\") present although its condition is not met",
    );
}

#[test]
fn a_report_without_a_declared_template_is_reported_as_such() {
    let check = assert_passes(text_report());
    assert!(
        check.message.contains("not examined: Template conformance: the root CONTAINER declares no Template in Content Template Sequence (0040,A504)"),
        "{}",
        check.message
    );
    let mut object = measurement_report(COMPREHENSIVE_SR);
    object.put(template_sequence("4300"));
    let check = assert_passes(object);
    assert!(
        check.message.contains(
            "it declares TID 4300, which is not among the Templates of PS3.16 the IODs reach"
        ),
        "{}",
        check.message
    );
}

#[test]
fn a_measurement_report_has_a_measurements_container() {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    let mut children: Vec<InMemDicomObject> = object
        .get(tags::CONTENT_SEQUENCE)
        .and_then(|element| element.items())
        .unwrap()
        .to_vec();
    children.pop();
    object.put(sq(tags::CONTENT_SEQUENCE, children));
    let check = assert_fails(
        object,
        "the root Content Item: TID 1500 \"Measurement Report\" Row 6 CONTAINER (126010, DCM, \"Imaging Measurements\") missing",
    );
    assert!(
        check.message.contains("Row 10 CONTAINER"),
        "{}",
        check.message
    );
}

#[test]
fn a_measurement_report_title_is_from_cid_7021() {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    object.put(sq(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        vec![code("DCM", "113000", "Of Interest")],
    ));
    let check = assert_passes(object);
    assert!(
        check.message.contains("(113000, DCM, \"Of Interest\") is not in Defined CID 7021 \"Measurement Report Document Title\""),
        "{}",
        check.message
    );
}

#[test]
fn a_planar_roi_is_selected_from_an_image() {
    // TID 1410 Row 6: the SCOORD's SELECTED FROM IMAGE is Mandatory.
    let mut object = measurement_report(COMPREHENSIVE_SR);
    set_children(&mut object, &REGION, vec![]);
    assert_fails(
        object,
        "Content Sequence[4] > Content Sequence[1] > Content Sequence[4]: TID 1500 \"Measurement Report\" TID 1410 Row 6 IMAGE of any Concept Name missing",
    );
}

#[test]
fn a_planar_roi_has_one_region() {
    // TID 1410 Rows 5 and 7b are exclusive (XOR); TID 1501 would take both,
    // so the group declares TID 1410.
    let mut object = measurement_report(COMPREHENSIVE_3D_SR);
    put_at(&mut object, &GROUP, template_sequence("1410"));
    push_child(
        &mut object,
        &GROUP,
        content(
            "CONTAINS",
            "SCOORD3D",
            Some(code("DCM", "111030", "Image Region")),
            vec![
                text(
                    tags::REFERENCED_FRAME_OF_REFERENCE_UID,
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.60.40",
                ),
                text(tags::GRAPHIC_TYPE, VR::CS, "POINT"),
                floats(tags::GRAPHIC_DATA, &[1.0, 2.0, 3.0]),
            ],
        ),
    );
    assert_fails(
        object,
        "TID 1410 Rows 5, 7, 7b, 8b are exclusive (XOR), but 2 are present",
    );
}

#[test]
fn a_measurement_group_matches_the_template_it_departs_from_least() {
    // A Measurement Group of TID 1501, whose SCOORD names its purpose with a
    // parameter rather than "Image Region", departs from TID 1410 but not
    // from TID 1501.
    let mut object = measurement_report(COMPREHENSIVE_SR);
    put_at(
        &mut object,
        &REGION,
        sq(
            tags::CONCEPT_NAME_CODE_SEQUENCE,
            vec![code("SCT", "260753009", "Source")],
        ),
    );
    let check = assert_passes(object);
    assert!(
        check
            .message
            .contains("rows a parameter constrains admit any Concept Name"),
        "{}",
        check.message
    );
}

#[test]
fn observer_context_items_follow_their_template_order() {
    // TID 1004 is ordered: Device Observer UID (Row 1) before its Name (Row 2).
    let name = content(
        "HAS OBS CONTEXT",
        "TEXT",
        Some(code("DCM", "121013", "Device Observer Name")),
        vec![text(tags::TEXT_VALUE, VR::UT, "Scanner")],
    );
    let mut object = measurement_report(COMPREHENSIVE_SR);
    let mut children: Vec<InMemDicomObject> = object
        .get(tags::CONTENT_SEQUENCE)
        .and_then(|element| element.items())
        .unwrap()
        .to_vec();
    children.insert(2, name.clone());
    object.put(sq(tags::CONTENT_SEQUENCE, children.clone()));
    assert_passes(object.clone());
    children.remove(2);
    children.insert(1, name);
    object.put(sq(tags::CONTENT_SEQUENCE, children));
    assert_fails(
        object,
        "TID 1004 \"Device Observer Identifying Attributes\" Content Items are not in Template row order",
    );
}

#[test]
fn a_device_observer_has_its_uid() {
    // TID 1002 Row 3: IFF Row 1 value is Device, TID 1004, whose Row 1
    // Device Observer UID is Mandatory.
    let mut object = measurement_report(COMPREHENSIVE_SR);
    let mut children: Vec<InMemDicomObject> = object
        .get(tags::CONTENT_SEQUENCE)
        .and_then(|element| element.items())
        .unwrap()
        .to_vec();
    children.remove(1);
    object.put(sq(tags::CONTENT_SEQUENCE, children));
    let check = check(object);
    assert!(
        check
            .message
            .contains("TID 1004 Row 1 UIDREF (121012, DCM, \"Device Observer UID\") missing"),
        "{}",
        check.message
    );
}

#[test]
fn a_declared_subtree_template_is_held_to_its_own() {
    // The Basic Text SR declares no root Template; a CONTAINER below it
    // declares TID 1501, whose Measurement Group it lacks the Concept Name of.
    let mut object = measurement_report(ENHANCED_SR);
    object.remove_element(tags::CONTENT_TEMPLATE_SEQUENCE);
    let mut group = measurement_group();
    group.put(template_sequence("1501"));
    group.put(sq(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        vec![code("DCM", "126010", "Imaging Measurements")],
    ));
    set_children(&mut object, &[3], vec![group]);
    assert_fails(
        object,
        "Content Sequence[4] > Content Sequence[1]: TID 1501 \"Measurement and Qualitative Evaluation Group\" Row 1 is CONTAINER (125007, DCM, \"Measurement Group\")",
    );
}

#[test]
fn code_rows_hold_the_value_set_they_bind() {
    // TID 1002 Row 1, Observer Type, binds Defined CID 270, which is not
    // extensible.
    let mut object = measurement_report(COMPREHENSIVE_SR);
    put_at(
        &mut object,
        &[0],
        sq(
            tags::CONCEPT_CODE_SEQUENCE,
            vec![code("DCM", "121192", "Device Subject")],
        ),
    );
    assert_fails(
        object,
        "Content Sequence[1] > Concept Code Sequence (0040,A168): (121192, DCM, \"Device Subject\") is not in Defined CID 270 \"Observer Type\"",
    );
}
