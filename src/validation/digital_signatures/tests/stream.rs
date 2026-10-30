//! The MAC byte stream, pinned byte for byte against PS3.3 C.12.1.1.3.1.2.

use dicom_core::value::PixelFragmentSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::{binary, code, sequence, stream_bytes, text, unsigned, EXPLICIT_LE, JPEG_BASELINE};

/// Explicit VR Little Endian Tag and VR.
fn head(tag: Tag, vr: &[u8; 2]) -> Vec<u8> {
    let mut bytes = tag.group().to_le_bytes().to_vec();
    bytes.extend(tag.element().to_le_bytes());
    bytes.extend(vr);
    bytes
}

/// A Data Element with a 16-bit Value Length.
fn short(tag: Tag, vr: &[u8; 2], value: &[u8]) -> Vec<u8> {
    let mut bytes = head(tag, vr);
    bytes.extend(u16::try_from(value.len()).unwrap().to_le_bytes());
    bytes.extend(value);
    bytes
}

const ITEM: [u8; 4] = [0xFE, 0xFF, 0x00, 0xE0];
const DELIMITER: [u8; 4] = [0xFE, 0xFF, 0xDD, 0xE0];

#[test]
fn the_mac_byte_stream_follows_c_12_1_1_3_1_2() {
    let data_set = InMemDicomObject::from_element_iter([
        DataElement::new(Tag(0x0008, 0x0000), VR::UL, PrimitiveValue::from(0_u32)),
        text(tags::SPECIFIC_CHARACTER_SET, VR::CS, "ISO_IR 100"),
        text(tags::SOP_CLASS_UID, VR::UI, "1.2.3"),
        text(tags::PATIENT_NAME, VR::PN, "Müller"),
        text(tags::PATIENT_ID, VR::LO, "unsigned"),
        unsigned(tags::ROWS, 8),
        binary(tags::ICC_PROFILE, vec![1, 2]),
        sequence(tags::ACQUISITION_CONTEXT_SEQUENCE, Vec::new()),
        sequence(
            tags::SPECIMEN_DESCRIPTION_SEQUENCE,
            vec![
                InMemDicomObject::from_element_iter([text(tags::SPECIMEN_IDENTIFIER, VR::LO, "A")]),
                InMemDicomObject::new_empty(),
            ],
        ),
        sequence(
            tags::CONTENT_SEQUENCE,
            vec![InMemDicomObject::from_element_iter([DataElement::new(
                Tag(0x0011, 0x1010),
                VR::UN,
                PrimitiveValue::from(vec![1_u8, 2]),
            )])],
        ),
        DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PixelFragmentSequence::new(Vec::new(), vec![vec![1, 2], vec![3, 4, 5, 6]]),
        ),
    ]);
    let signature = InMemDicomObject::from_element_iter([
        unsigned(tags::MACID_NUMBER, 0),
        text(tags::DIGITAL_SIGNATURE_UID, VR::UI, "1.2"),
        text(
            tags::DIGITAL_SIGNATURE_DATE_TIME,
            VR::DT,
            "20260101000000+0000",
        ),
        text(tags::CERTIFICATE_TYPE, VR::CS, "X509_1993_SIG"),
        binary(tags::CERTIFICATE_OF_SIGNER, vec![0x30, 0x00]),
        binary(tags::SIGNATURE, vec![9; 4]),
        text(tags::CERTIFIED_TIMESTAMP_TYPE, VR::CS, "CMS_TSP"),
        binary(tags::CERTIFIED_TIMESTAMP, vec![0x30, 0x00]),
        sequence(
            tags::DIGITAL_SIGNATURE_PURPOSE_CODE_SEQUENCE,
            vec![InMemDicomObject::from_element_iter([text(
                tags::CODE_VALUE,
                VR::SH,
                "1",
            )])],
        ),
    ]);
    // Listed out of order, with a group length and a Sequence holding a UN.
    let signed = [
        tags::PIXEL_DATA,
        Tag(0x0008, 0x0000),
        tags::SPECIFIC_CHARACTER_SET,
        tags::SOP_CLASS_UID,
        tags::PATIENT_NAME,
        tags::ROWS,
        tags::ICC_PROFILE,
        tags::ACQUISITION_CONTEXT_SEQUENCE,
        tags::SPECIMEN_DESCRIPTION_SEQUENCE,
        tags::CONTENT_SEQUENCE,
    ];

    let mut expected = Vec::new();
    expected.extend(short(tags::SPECIFIC_CHARACTER_SET, b"CS", b"ISO_IR 100"));
    expected.extend(short(tags::SOP_CLASS_UID, b"UI", b"1.2.3\0"));
    // Latin-1, as the Specific Character Set declares.
    expected.extend(short(tags::PATIENT_NAME, b"PN", b"M\xFCller"));
    expected.extend(short(tags::ROWS, b"US", &8_u16.to_le_bytes()));
    // OB of defined length: reserved field and 32-bit Value Length.
    expected.extend(head(tags::ICC_PROFILE, b"OB"));
    expected.extend([0, 0, 2, 0, 0, 0, 1, 2]);
    // Sequences: no Value Length, Item Tags, an appended delimiter.
    expected.extend(head(tags::ACQUISITION_CONTEXT_SEQUENCE, b"SQ"));
    expected.extend([0, 0]);
    expected.extend(DELIMITER);
    expected.extend(head(tags::SPECIMEN_DESCRIPTION_SEQUENCE, b"SQ"));
    expected.extend([0, 0]);
    expected.extend(ITEM);
    expected.extend(short(tags::SPECIMEN_IDENTIFIER, b"LO", b"A "));
    expected.extend(ITEM);
    expected.extend(DELIMITER);
    // Encapsulated Pixel Data: the empty Basic Offset Table, then each
    // Fragment, every Item without its Value Length.
    expected.extend(head(tags::PIXEL_DATA, b"OB"));
    expected.extend([0, 0]);
    expected.extend(ITEM);
    expected.extend(ITEM);
    expected.extend([1, 2]);
    expected.extend(ITEM);
    expected.extend([3, 4, 5, 6]);
    expected.extend(DELIMITER);
    // The Digital Signatures Sequence Item but its certificate, signature
    // and timestamp.
    expected.extend(short(tags::MACID_NUMBER, b"US", &[0, 0]));
    expected.extend(short(tags::DIGITAL_SIGNATURE_UID, b"UI", b"1.2\0"));
    expected.extend(short(
        tags::DIGITAL_SIGNATURE_DATE_TIME,
        b"DT",
        b"20260101000000+0000 ",
    ));
    expected.extend(short(tags::CERTIFICATE_TYPE, b"CS", b"X509_1993_SIG "));
    expected.extend(head(tags::DIGITAL_SIGNATURE_PURPOSE_CODE_SEQUENCE, b"SQ"));
    expected.extend([0, 0]);
    expected.extend(ITEM);
    expected.extend(short(tags::CODE_VALUE, b"SH", b"1 "));
    expected.extend(DELIMITER);

    let stream = stream_bytes(&data_set, &signature, JPEG_BASELINE, JPEG_BASELINE, &signed);
    assert_eq!(stream, expected);
}

#[test]
fn native_pixel_data_keeps_its_value_length() {
    let data_set = InMemDicomObject::from_element_iter([
        DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PrimitiveValue::from(vec![7_u8; 4]),
        ),
        sequence(tags::SPECIMEN_TYPE_CODE_SEQUENCE, vec![code("1", "x")]),
    ]);
    let mut expected = head(tags::PIXEL_DATA, b"OB");
    expected.extend([0, 0, 4, 0, 0, 0, 7, 7, 7, 7]);
    let stream = stream_bytes(
        &data_set,
        &InMemDicomObject::new_empty(),
        EXPLICIT_LE,
        EXPLICIT_LE,
        &[tags::PIXEL_DATA],
    );
    assert_eq!(stream, expected);
}
