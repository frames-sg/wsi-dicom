//! Attribute Value rules over parsed Data Sets, per Transfer Syntax.

use std::path::Path;

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::super::attributes::Attributes;
use crate::validation::iod::Family;

fn message(object: &InMemDicomObject, little_endian: Option<&'static str>) -> String {
    let mut findings = Family::default();
    Attributes {
        findings: &mut findings,
        little_endian,
    }
    .check(object);
    findings
        .check(Path::new(""), "encoding", "citation")
        .message
}

#[test]
fn lut_descriptors_are_little_endian_in_every_little_endian_syntax() {
    let voi = InMemDicomObject::from_element_iter([DataElement::new(
        tags::LUT_DESCRIPTOR,
        VR::US,
        PrimitiveValue::U16(vec![256, 0, 0x0800].into()),
    )]);
    let object = InMemDicomObject::from_element_iter([DataElement::new(
        tags::VOILUT_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![voi]),
    )]);
    for annex in ["A.1", "A.2", "A.4"] {
        let message = message(&object, Some(annex));
        assert!(
            message.contains(&format!(
                "VOILUTSequence (0028,3010)[1] > LUTDescriptor (0028,3002): third Value 2048 is 8 byte-swapped, so the Descriptor is encoded Big Endian; it shall be encoded in Little Endian (PS3.5 {annex})"
            )),
            "{message}"
        );
    }
    // Big Endian syntaxes keep their own byte order rules.
    assert_eq!(message(&object, None), "citation: conformant");
    let little = InMemDicomObject::from_element_iter([DataElement::new(
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        VR::US,
        PrimitiveValue::U16(vec![256, 0, 16].into()),
    )]);
    assert_eq!(message(&little, Some("A.1")), "citation: conformant");
}
