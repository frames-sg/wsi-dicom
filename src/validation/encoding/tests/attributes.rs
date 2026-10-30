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
        explicit_vr: true,
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

/// A Waveform Sequence Item with Waveform Data of `vr`, its padding value
/// and one channel's limits of `related`.
fn waveform(bits: u16, vr: VR, related: VR) -> InMemDicomObject {
    let binary = |tag, vr| DataElement::new(tag, vr, PrimitiveValue::U8(vec![0, 0].into()));
    let channel = InMemDicomObject::from_element_iter([
        binary(tags::CHANNEL_MINIMUM_VALUE, related),
        binary(tags::CHANNEL_MAXIMUM_VALUE, related),
    ]);
    let item = InMemDicomObject::from_element_iter([
        DataElement::new(
            tags::WAVEFORM_BITS_ALLOCATED,
            VR::US,
            PrimitiveValue::from(bits),
        ),
        binary(tags::WAVEFORM_PADDING_VALUE, related),
        DataElement::new(
            tags::CHANNEL_DEFINITION_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![channel]),
        ),
        binary(tags::WAVEFORM_DATA, vr),
    ]);
    InMemDicomObject::from_element_iter([DataElement::new(
        tags::WAVEFORM_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![item]),
    )])
}

#[test]
fn waveform_data_is_ow_or_ob_for_eight_bit_samples_with_related_values_alike() {
    for (bits, vr) in [(16, VR::OW), (8, VR::OB), (32, VR::OW)] {
        assert_eq!(
            message(&waveform(bits, vr, vr), Some("A.2")),
            "citation: conformant"
        );
    }
    for (bits, vr) in [(16, VR::OB), (8, VR::OW)] {
        let message = message(&waveform(bits, vr, vr), Some("A.2"));
        assert!(
            message.contains(&format!(
                "WaveformSequence (5400,0100)[1] > WaveformData (5400,1010): VR {vr:?} with Waveform Bits Allocated (5400,1004) {bits}; Waveform Data shall be OW, and OB where Waveform Bits Allocated is 8 in an Explicit VR Transfer Syntax (PS3.5 8.3)"
            )),
            "{message}"
        );
    }
    let related = message(&waveform(16, VR::OW, VR::OB), Some("A.2"));
    for expected in [
        "WaveformSequence (5400,0100)[1] > WaveformPaddingValue (5400,100A): VR OB where Waveform Data (5400,1010) is OW; it shall be encoded with the same VR as Waveform Data (PS3.5 8.3)",
        "WaveformSequence (5400,0100)[1] > ChannelDefinitionSequence (003A,0200)[1] > ChannelMinimumValue (5400,0110): VR OB where Waveform Data (5400,1010) is OW",
        "ChannelDefinitionSequence (003A,0200)[1] > ChannelMaximumValue (5400,0112): VR OB",
    ] {
        assert!(related.contains(expected), "{expected}: {related}");
    }
    // A zero-length Value is absent (PS3.5 7.4.5).
    let mut empty = waveform(16, VR::OW, VR::OW);
    let mut items = empty.take(tags::WAVEFORM_SEQUENCE).unwrap();
    items.items_mut().unwrap()[0].put(DataElement::new(
        tags::WAVEFORM_PADDING_VALUE,
        VR::OB,
        PrimitiveValue::Empty,
    ));
    empty.put(items);
    assert_eq!(message(&empty, Some("A.2")), "citation: conformant");
    // Implicit VR conveys no VR, so the parsed one is not the encoded one.
    let mut findings = Family::default();
    Attributes {
        findings: &mut findings,
        little_endian: Some("A.1"),
        explicit_vr: false,
    }
    .check(&waveform(16, VR::OB, VR::OW));
    assert!(findings.is_empty());
}
