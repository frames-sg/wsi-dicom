//! The IOD's Value Types and Relationship Content Constraints, by-reference
//! relationships, and the SELECTED FROM Targets of SCOORD and TCOORD.

use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::tags;

use super::fixtures::*;
use super::{assert_fails, assert_passes};

#[test]
fn the_conformant_documents_pass() {
    assert_passes(key_object_selection());
    assert_passes(text_report());
    for class in [ENHANCED_SR, COMPREHENSIVE_SR, COMPREHENSIVE_3D_SR] {
        assert_passes(measurement_report(class));
    }
}

#[test]
fn a_value_type_the_iod_does_not_permit_fails() {
    // A.35.1.3.1.1: the Basic Text SR has no NUM.
    let mut object = text_report();
    push_child(&mut object, &[], area());
    assert_fails(
        object,
        "Content Sequence[3] has Value Type (0040,A040) NUM, which the IOD does not permit in Content Sequence (0040,A730)",
    );
}

#[test]
fn a_relationship_the_table_does_not_permit_fails() {
    // Table A.35.4-2: a KOS CONTAINER CONTAINS no CODE.
    let mut object = key_object_selection();
    push_child(
        &mut object,
        &[],
        content(
            "CONTAINS",
            "CODE",
            Some(code("DCM", "121071", "Finding")),
            vec![sq(
                tags::CONCEPT_CODE_SEQUENCE,
                vec![code("SCT", "85756007", "Tissue")],
            )],
        ),
    );
    assert_fails(
        object,
        "Content Sequence[3]: a CONTAINER Source Content Item CONTAINS a CODE Target Content Item, which Table A.35.4-2 does not permit (PS3.3 2026d A.35.4.3.1.2)",
    );
}

#[test]
fn the_root_content_item_is_a_container() {
    let mut object = text_report();
    object.put(text(tags::VALUE_TYPE, VR::CS, "TEXT"));
    assert_fails(
        object,
        "the root Content Item has Value Type (0040,A040) TEXT",
    );
}

#[test]
fn by_value_iods_forbid_by_reference_relationships() {
    for (mut object, section) in [
        (key_object_selection(), "A.35.4"),
        (text_report(), "A.35.1"),
        (measurement_report(ENHANCED_SR), "A.35.2"),
    ] {
        push_child(&mut object, &[], by_reference("CONTAINS", &[1, 1]));
        assert_fails(
            object,
            &format!("relationships between Content Items in the content of this IOD shall be conveyed in the by-value mode (PS3.3 2026d {section}.3.1.2)"),
        );
    }
}

/// The NUM's INFERRED FROM the SCOORD of its Measurement Group, by-reference.
fn inferred_from_region(class: &str) -> dicom_object::InMemDicomObject {
    let mut object = measurement_report(class);
    push_child(
        &mut object,
        &AREA,
        by_reference("INFERRED FROM", &[1, 4, 1, 4]),
    );
    object
}

#[test]
fn comprehensive_documents_may_convey_other_relationships_by_reference() {
    assert_passes(inferred_from_region(COMPREHENSIVE_SR));
    assert_passes(inferred_from_region(COMPREHENSIVE_3D_SR));
}

#[test]
fn has_concept_mod_and_contains_are_never_by_reference() {
    for class in [COMPREHENSIVE_SR, COMPREHENSIVE_3D_SR] {
        let mut object = measurement_report(class);
        push_child(&mut object, &GROUP, by_reference("CONTAINS", &[1, 4, 1, 4]));
        assert_fails(
            object,
            "conveys a CONTAINS relationship by-reference (Referenced Content Item Identifier (0040,DB73) 1\\4\\1\\4); the HAS CONCEPT MOD and CONTAINS relationships shall not be conveyed by-reference",
        );
    }
}

#[test]
fn a_reference_names_a_content_item_of_the_tree() {
    for identifier in [&[1, 9][..], &[2, 1], &[1, 4, 0]] {
        let mut object = measurement_report(COMPREHENSIVE_SR);
        push_child(
            &mut object,
            &AREA,
            by_reference("INFERRED FROM", identifier),
        );
        assert_fails(
            object,
            "which identifies no Content Item of the Content Tree",
        );
    }
}

#[test]
fn a_reference_to_an_ancestor_fails() {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    // The NUM INFERRED FROM its own Measurement Group.
    push_child(
        &mut object,
        &AREA,
        by_reference("INFERRED FROM", &[1, 4, 1]),
    );
    assert_fails(
        object,
        "references Content Item 1.4.1 by-reference, which is an ancestor of its Source Content Item; relationships by-reference to ancestor Content Items are forbidden in this IOD to prevent loops (PS3.3 2026d A.35.3.3.1.2)",
    );
    let mut object = measurement_report(COMPREHENSIVE_3D_SR);
    push_child(
        &mut object,
        &AREA,
        by_reference("INFERRED FROM", &[1, 4, 1, 5]),
    );
    assert_fails(object, "which is its Source Content Item");
}

#[test]
fn a_loop_of_by_reference_relationships_fails() {
    // Two TEXT findings, each INFERRED FROM the other.
    let finding = |name: &str, other: u32| {
        content(
            "CONTAINS",
            "TEXT",
            Some(code("DCM", "121071", "Finding")),
            vec![
                text(tags::TEXT_VALUE, VR::UT, name),
                sq(
                    tags::CONTENT_SEQUENCE,
                    vec![by_reference("INFERRED FROM", &[1, other])],
                ),
            ],
        )
    };
    let mut object = measurement_report(COMPREHENSIVE_SR);
    push_child(&mut object, &[], finding("first", 6));
    push_child(&mut object, &[], finding("second", 5));
    assert_fails(
        object,
        "closes a loop of relationships through by-reference Content Items",
    );
}

#[test]
fn a_reference_targets_a_content_item_with_a_value_type() {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    push_child(
        &mut object,
        &AREA,
        by_reference("INFERRED FROM", &[1, 4, 1, 4]),
    );
    // A second by-reference Item names the first.
    push_child(
        &mut object,
        &AREA,
        by_reference("INFERRED FROM", &[1, 4, 1, 5, 1]),
    );
    assert_fails(object, "which is itself a by-reference Content Item");
}

#[test]
fn an_scoord_is_selected_from_an_image() {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    set_children(&mut object, &REGION, vec![]);
    assert_fails(
        object,
        "is an SCOORD Content Item without a SELECTED FROM relationship to an IMAGE Target Content Item; an SCOORD Content Item shall always be the Source Content Item of one or more SELECTED FROM Relationships with IMAGE Target Content Items (PS3.3 2026d C.18.6)",
    );
}

#[test]
fn an_scoord_may_be_selected_from_an_image_by_reference() {
    // An Illustration of ROI image in the group, which the SCOORD selects
    // from by-reference. TID 1410 Row 6 selects by-value, so the document
    // declares no Template here.
    let mut object = measurement_report(COMPREHENSIVE_SR);
    object.remove_element(tags::CONTENT_TEMPLATE_SEQUENCE);
    push_child(
        &mut object,
        &GROUP,
        content(
            "CONTAINS",
            "IMAGE",
            Some(code("DCM", "121200", "Illustration of ROI")),
            vec![sq(
                tags::REFERENCED_SOP_SEQUENCE,
                vec![reference(WSI, IMAGE)],
            )],
        ),
    );
    set_children(
        &mut object,
        &REGION,
        vec![by_reference("SELECTED FROM", &[1, 4, 1, 6])],
    );
    assert_passes(object);
}

#[test]
fn the_image_an_scoord_is_selected_from_is_an_image() {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    set_children(
        &mut object,
        &REGION,
        vec![content(
            "SELECTED FROM",
            "IMAGE",
            None,
            vec![sq(
                tags::REFERENCED_SOP_SEQUENCE,
                vec![reference(BASIC_TEXT_SR, IMAGE)],
            )],
        )],
    );
    assert_fails(
        object,
        "which is not an Image Storage SOP Class; each IMAGE Target Content Item shall contain a reference to one single-frame or Multi-frame Image (PS3.3 2026d C.18.6)",
    );
}

fn temporal(elements: Vec<dicom_object::mem::InMemElement>) -> dicom_object::InMemDicomObject {
    let mut elements = elements;
    elements.push(text(tags::TEMPORAL_RANGE_TYPE, VR::CS, "POINT"));
    content(
        "CONTAINS",
        "TCOORD",
        Some(code("DCM", "111030", "Image Region")),
        elements,
    )
}

fn waveform(channels: &[u16]) -> dicom_object::InMemDicomObject {
    let mut sop = reference(
        "1.2.840.10008.5.1.4.1.1.9.1.1",
        "1.2.826.0.1.3680043.10.999.60.30",
    );
    sop.put(DataElement::new(
        tags::REFERENCED_WAVEFORM_CHANNELS,
        VR::US,
        PrimitiveValue::U16(channels.iter().copied().collect()),
    ));
    content(
        "SELECTED FROM",
        "WAVEFORM",
        None,
        vec![sq(tags::REFERENCED_SOP_SEQUENCE, vec![sop])],
    )
}

#[test]
fn a_tcoord_is_selected_from_coordinates_images_or_waveforms() {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    push_child(
        &mut object,
        &GROUP,
        temporal(vec![text(tags::REFERENCED_TIME_OFFSETS, VR::DS, "1.5")]),
    );
    assert_fails(
        object,
        "is a TCOORD Content Item without a SELECTED FROM relationship to an SCOORD, IMAGE or WAVEFORM Target Content Item",
    );
}

#[test]
fn referenced_sample_positions_need_a_single_multiplex_group() {
    let positions = || text(tags::REFERENCED_SAMPLE_POSITIONS, VR::UL, "5");
    let single = temporal(vec![
        positions(),
        sq(tags::CONTENT_SEQUENCE, vec![waveform(&[1, 1, 1, 2])]),
    ]);
    let mut object = measurement_report(COMPREHENSIVE_SR);
    push_child(&mut object, &GROUP, single);
    let check = super::check(object);
    assert!(
        !check.message.contains("Referenced Sample Positions"),
        "{}",
        check.message
    );

    let mixed = temporal(vec![
        positions(),
        sq(tags::CONTENT_SEQUENCE, vec![waveform(&[1, 1, 2, 1])]),
    ]);
    let mut object = measurement_report(COMPREHENSIVE_SR);
    push_child(&mut object, &GROUP, mixed);
    assert_fails(
        object,
        "has Referenced Waveform Channels (0040,A0B0) in multiplex groups [1, 2]; it shall only be used when they refer to channels within a single multiplex group (PS3.3 2026d Table C.18.7-1)",
    );

    let imaged = temporal(vec![
        positions(),
        sq(tags::CONTENT_SEQUENCE, vec![image("SELECTED FROM")]),
    ]);
    let mut object = measurement_report(COMPREHENSIVE_SR);
    push_child(&mut object, &GROUP, imaged);
    assert_fails(object, "but the TCOORD selects from no WAVEFORM");
}
