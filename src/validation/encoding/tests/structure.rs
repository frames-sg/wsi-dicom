//! Framing, order, VR bytes, delimiters, Group Lengths, groups, Undefined
//! Lengths, padding, Pixel Data and Deflated Data Sets.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::Compression;

use super::*;

const SOP_CLASS: (u16, u16) = (0x0008, 0x0016);
const MODALITY: (u16, u16) = (0x0008, 0x0060);
const PATIENT_ID: (u16, u16) = (0x0010, 0x0020);
const OPTICAL_PATH: (u16, u16) = (0x0048, 0x0105);
const OPTICAL_PATH_ID: (u16, u16) = (0x0048, 0x0106);
const PIXEL_DATA: (u16, u16) = (0x7FE0, 0x0010);

fn sorted() -> Stream {
    let mut stream = Stream::explicit();
    stream
        .text(SOP_CLASS, b"UI", "1.2.840.10008.5.1.4.1.1.77.1.6")
        .text(MODALITY, b"CS", "SM")
        .text(PATIENT_ID, b"LO", "ID");
    stream
}

#[test]
fn conformant_streams_pass_in_every_layout() {
    assert_conformant(EXPLICIT_LE, &sorted().bytes);
    let mut implicit = Stream::implicit();
    implicit
        .text(SOP_CLASS, b"UI", "1.2\0")
        .text(PATIENT_ID, b"LO", "ID")
        .sequence(
            OPTICAL_PATH,
            &[{
                let mut item = Stream::implicit();
                item.text(OPTICAL_PATH_ID, b"SH", "1 ");
                item
            }],
        );
    assert_conformant(IMPLICIT_LE, &implicit.bytes);
    let mut big = Stream::big_endian();
    big.text(MODALITY, b"CS", "SM").defined_sequence(
        OPTICAL_PATH,
        &[{
            let mut item = Stream::big_endian();
            item.text(OPTICAL_PATH_ID, b"SH", "1 ");
            item
        }],
    );
    assert_conformant(EXPLICIT_BE, &big.bytes);
}

#[test]
fn data_elements_are_ordered_and_unique_in_data_sets_and_items() {
    let mut stream = Stream::explicit();
    stream
        .text(PATIENT_ID, b"LO", "ID")
        .text(MODALITY, b"CS", "SM");
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "Modality (0008,0060): follows (0010,0020); the Data Elements of a Data Set shall be in increasing tag order (PS3.5 7.1 and 7.5.1)",
    );
    let mut item = Stream::explicit();
    item.text(OPTICAL_PATH_ID, b"SH", "1 ")
        .text(OPTICAL_PATH_ID, b"SH", "2 ");
    let mut stream = Stream::explicit();
    stream.sequence(OPTICAL_PATH, &[item]);
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "OpticalPathSequence (0048,0105)[1] > OpticalPathIdentifier (0048,0106): occurs more than once in this Item",
    );
    let mut private = Stream::explicit();
    private
        .text((0x0009, 0x0010), b"LO", "ACME")
        .text((0x0009, 0x1002), b"LO", "B ")
        .text((0x0009, 0x1001), b"LO", "A ");
    assert_violation(
        EXPLICIT_LE,
        &private.bytes,
        "(0009,1001): follows (0009,1002); the Data Elements of a Data Set shall be in increasing tag order (PS3.5 7.1 and 7.8)",
    );
}

#[test]
fn explicit_vr_bytes_are_upper_case_vrs_of_part_5() {
    let mut lower = Stream::explicit();
    lower
        .text(MODALITY, b"cs", "SM")
        .text(PATIENT_ID, b"LO", "ID");
    assert_violation(
        EXPLICIT_LE,
        &lower.bytes,
        "Modality (0008,0060): VR bytes \"cs\" are not upper case",
    );
    let mut unknown = Stream::explicit();
    unknown
        .element((0x0009, 0x0010), b"LO", b"ACME")
        .element((0x0009, 0x1000), b"XX", b"ab");
    for (transfer_syntax, annex) in [
        (EXPLICIT_LE, "A.2"),
        (JPEG_BASELINE, "A.4"),
        ("1.2.840.10008.1.2.4.94", "A.6"),
        ("1.2.840.10008.1.2.4.204", "A.11"),
    ] {
        assert_violation(
            transfer_syntax,
            &unknown.bytes,
            &format!("(0009,1000): VR bytes 58H 58H are not a VR of PS3.5 6.2; Data Elements shall be encoded with an Explicit VR of PS3.5 6.2 (PS3.5 7.1.2, 7.8, 7.8.2, and {annex})"),
        );
    }
}

#[test]
fn reserved_bytes_after_a_long_vr_are_zero() {
    let mut stream = Stream::explicit();
    stream
        .tag((0x0028, 0x2000))
        .raw(b"OB")
        .raw(&[0x01, 0x00])
        .u32(2)
        .raw(&[0, 0]);
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "ICCProfile (0028,2000): reserved bytes after VR OB are 0100H; they shall be 0000H (PS3.5 7.1.2)",
    );
}

#[test]
fn delimiters_have_zero_length_and_no_value() {
    let mut stream = Stream::explicit();
    stream
        .header(OPTICAL_PATH, b"SQ", u32::MAX)
        .item(u32::MAX)
        .text(OPTICAL_PATH_ID, b"SH", "1 ")
        .tag((0xFFFE, 0xE00D))
        .u32(4)
        .tag((0xFFFE, 0xE0DD))
        .u32(2);
    let message = check(EXPLICIT_LE, &stream.bytes).message;
    assert!(
        message.contains("Item Delimitation Item (FFFE,E00D) has Value Length 00000004H; it shall be 00000000H with no Value (PS3.5 7.5.1)"),
        "{message}"
    );
    assert!(
        message.contains("OpticalPathSequence (0048,0105): Sequence Delimitation Item (FFFE,E0DD) has Value Length 00000002H; it shall be 00000000H with no Value (PS3.5 7.5.2)"),
        "{message}"
    );
}

#[test]
fn delimiters_belong_to_their_sequences() {
    let mut item = Stream::explicit();
    item.text(OPTICAL_PATH_ID, b"SH", "1 ").item_end();
    let mut stream = Stream::explicit();
    stream.defined_sequence(OPTICAL_PATH, &[item]);
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "Item Delimitation Item (FFFE,E00D) in an Item of Explicit Length",
    );
}

#[test]
fn a_file_holds_one_data_set() {
    let mut stray = sorted();
    stray.sequence_end();
    assert_violation(
        EXPLICIT_LE,
        &stray.bytes,
        "(FFFE,E0DD) outside any Sequence; a File contains a single Data Set (PS3.10 7.2)",
    );
    let mut trailing = sorted();
    trailing.raw(&[0, 0, 0]);
    assert_violation(
        EXPLICIT_LE,
        &trailing.bytes,
        "3 trailing bytes after the last Data Element are not a Data Element; a File contains a single Data Set (PS3.10 7.2)",
    );
    // A second Data Set appended starts its tags over.
    let mut second = sorted();
    second.raw(&sorted().bytes);
    assert_violation(
        EXPLICIT_LE,
        &second.bytes,
        "SOPClassUID (0008,0016): occurs more than once in this Data Set",
    );
}

#[test]
fn values_end_where_their_lengths_say() {
    let mut stream = sorted();
    stream.header((0x0010, 0x0030), b"DA", 8).raw(b"2024");
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "PatientBirthDate (0010,0030) needs 8 bytes but the Data Set ends after 4",
    );
    let mut item = Stream::explicit();
    item.text(OPTICAL_PATH_ID, b"SH", "1 ");
    let mut overrun = Stream::explicit();
    overrun
        .header(OPTICAL_PATH, b"SQ", 8 + 4)
        .item(item.bytes.len() as u32)
        .raw(&item.bytes);
    assert_violation(
        EXPLICIT_LE,
        &overrun.bytes,
        "OpticalPathSequence (0048,0105)[1]: Item Length 10 extends 6 bytes beyond its Sequence",
    );
}

#[test]
fn group_lengths_match_their_groups() {
    let mut stream = Stream::explicit();
    stream
        .element((0x0008, 0x0000), b"UL", &20u32.to_le_bytes())
        .text(SOP_CLASS, b"UI", "1.2\0")
        .text(MODALITY, b"CS", "SM")
        .text(PATIENT_ID, b"LO", "ID");
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "(0008,0000): Group Length 20 but the group's elements take 22 bytes; a Group Length shall be consistent with the encoding (PS3.5 7.2)",
    );
    let mut consistent = Stream::explicit();
    consistent
        .element((0x0008, 0x0000), b"UL", &22u32.to_le_bytes())
        .text(SOP_CLASS, b"UI", "1.2\0")
        .text(MODALITY, b"CS", "SM")
        .text(PATIENT_ID, b"LO", "ID");
    assert_conformant(EXPLICIT_LE, &consistent.bytes);
}

#[test]
fn reserved_groups_are_not_used() {
    for group in [0x0001, 0x0003, 0x0005, 0x0007, 0xFFFF] {
        let mut stream = sorted();
        stream.text((group, 0x0010), b"LO", "X ");
        assert_violation(
            EXPLICIT_LE,
            &stream.bytes,
            &format!("({group:04X},0010): group {group:04X} shall not be used (PS3.5 7.1, 7.5.1 and 7.8.1)"),
        );
    }
}

#[test]
fn items_hold_no_command_file_meta_or_directory_groups() {
    for (group, element, vr, value, tail) in [
        (0x0000, 0x0000, b"UL", &4u32.to_le_bytes()[..], ""),
        (
            0x0002,
            0x0010,
            b"UI",
            &b"1.2.840.10008.1.2\0"[..],
            "; group 0002 belongs only to the File Meta Information (PS3.10 7.1)",
        ),
        (0x0006, 0x0001, b"UL", &4u32.to_le_bytes()[..], ""),
    ] {
        let mut item = Stream::explicit();
        item.element((group, element), vr, value)
            .text(OPTICAL_PATH_ID, b"SH", "1 ");
        let mut stream = Stream::explicit();
        stream.sequence(OPTICAL_PATH, &[item]);
        assert_violation(
            EXPLICIT_LE,
            &stream.bytes,
            &format!(
                "group {group:04X} shall not be present within Sequence Items (PS3.5 7.5.1){tail}"
            ),
        );
    }
}

#[test]
fn repeating_groups_are_the_overlay_groups_only() {
    let mut stream = sorted();
    stream
        .element((0x5000, 0x0005), b"US", &[1, 0])
        .element((0x6000, 0x0010), b"US", &[1, 0])
        .element((0x6020, 0x0010), b"US", &[1, 0]);
    let message = check(EXPLICIT_LE, &stream.bytes).message;
    for group in ["5000", "6020"] {
        assert!(
            message.contains(&format!("({group},"))
                && message.contains(
                    "Repeating Groups are only allowed in the even groups 6000-601E (PS3.5 7.6)"
                ),
            "{message}"
        );
    }
    assert!(!message.contains("(6000,0010)"), "{message}");
}

#[test]
fn undefined_length_is_for_sequences_unknown_and_encapsulated_pixel_data() {
    let mut other = sorted();
    other
        .header((0x0028, 0x1201), b"OW", u32::MAX)
        .sequence_end();
    assert_violation(
        EXPLICIT_LE,
        &other.bytes,
        "RedPaletteColorLookupTableData (0028,1201): Undefined Length on VR OW; it is permitted only for SQ, UN and encapsulated Pixel Data (PS3.5 7.1.1)",
    );
    let mut pixels = sorted();
    pixels
        .header(PIXEL_DATA, b"OB", u32::MAX)
        .item(0)
        .item(2)
        .raw(&[0xFF, 0xD9])
        .sequence_end();
    assert_conformant(JPEG_BASELINE, &pixels.bytes);
    assert_violation(
        EXPLICIT_LE,
        &pixels.bytes,
        "PixelData (7FE0,0010): Undefined Length, but the Transfer Syntax encodes Pixel Data in Native Format with an Explicit Length (PS3.5 7.1.1 and A.2)",
    );
    let mut fragment = sorted();
    fragment
        .header(PIXEL_DATA, b"OB", u32::MAX)
        .item(u32::MAX)
        .sequence_end();
    assert_violation(
        JPEG_BASELINE,
        &fragment.bytes,
        "where a fragment Item of Explicit Length or the Sequence Delimitation Item is expected",
    );
}

#[test]
fn undefined_length_unknown_items_stay_in_implicit_vr() {
    let mut implicit_item = Stream::implicit();
    implicit_item.text(OPTICAL_PATH_ID, b"SH", "1 ");
    let mut implicit = sorted();
    implicit
        .header((0x0048, 0x0105), b"UN", u32::MAX)
        .item(u32::MAX)
        .raw(&implicit_item.bytes)
        .item_end()
        .sequence_end();
    assert_conformant(EXPLICIT_LE, &implicit.bytes);

    let mut explicit_item = Stream::explicit();
    explicit_item.text(OPTICAL_PATH_ID, b"SH", "1 ");
    let mut explicit = sorted();
    explicit
        .header((0x0048, 0x0105), b"UN", u32::MAX)
        .item(u32::MAX)
        .raw(&explicit_item.bytes)
        .item_end()
        .sequence_end();
    assert_violation(
        EXPLICIT_LE,
        &explicit.bytes,
        "OpticalPathSequence (0048,0105): UN of Undefined Length whose Items are in Explicit VR; they shall remain in Implicit VR form (PS3.5 6.2.2)",
    );
}

#[test]
fn padding_is_space_for_text_null_for_uids_and_ob() {
    let mut text = Stream::explicit();
    text.text(MODALITY, b"CS", "S\0");
    assert_violation(
        EXPLICIT_LE,
        &text.bytes,
        "Modality (0008,0060): CS Value padded with NULL (00H); character string Values are padded with SPACE (20H) (PS3.5 6.2)",
    );
    let mut uid = Stream::explicit();
    uid.text(SOP_CLASS, b"UI", "1.2 ");
    assert_violation(
        EXPLICIT_LE,
        &uid.bytes,
        "SOPClassUID (0008,0016): UID Value padded with SPACE (20H)",
    );
    // An ICC profile header declaring 131 bytes, padded with 01H.
    let mut profile = vec![0u8; 132];
    profile[..4].copy_from_slice(&131u32.to_be_bytes());
    profile[131] = 1;
    let mut icc = Stream::explicit();
    icc.element((0x0028, 0x2000), b"OB", &profile);
    assert_violation(
        EXPLICIT_LE,
        &icc.bytes,
        "ICCProfile (0028,2000): OB Value padded with 01H; OB Values are padded with a single trailing NULL (00H) (PS3.5 6.2)",
    );
    profile[131] = 0;
    let mut padded = Stream::explicit();
    padded.element((0x0028, 0x2000), b"OB", &profile);
    assert_conformant(EXPLICIT_LE, &padded.bytes);
}

/// A native single-frame image of `rows` x 1 pixels with `bits` bits.
fn native_image(bits: u16, rows: u16, vr: &[u8; 2], pixels: &[u8]) -> Stream {
    let mut stream = Stream::explicit();
    stream
        .us((0x0028, 0x0002), 1)
        .us((0x0028, 0x0010), rows)
        .us((0x0028, 0x0011), 1)
        .us((0x0028, 0x0100), bits)
        .element(PIXEL_DATA, vr, pixels);
    stream
}

#[test]
fn native_pixel_data_vr_follows_bits_allocated() {
    assert_violation(
        EXPLICIT_LE,
        &native_image(16, 1, b"OB", &[1, 2]).bytes,
        "PixelData (7FE0,0010): VR OB with Bits Allocated (0028,0100) 16; native Pixel Data with Bits Allocated above 8 shall be OW (PS3.5 A.2)",
    );
    assert_conformant(EXPLICIT_LE, &native_image(16, 1, b"OW", &[1, 2]).bytes);
    // Three 8-bit pixels and the pad byte.
    assert_violation(
        EXPLICIT_LE,
        &native_image(8, 3, b"OB", &[1, 2, 3, 9]).bytes,
        "PixelData (7FE0,0010): OB Value padded with 09H",
    );
    assert_conformant(EXPLICIT_LE, &native_image(8, 3, b"OB", &[1, 2, 3, 0]).bytes);
    // A nested native image in an encapsulated syntax (PS3.5 A.4).
    let mut icon = Stream::explicit();
    icon.sequence((0x0088, 0x0200), &[native_image(16, 1, b"OB", &[1, 2])]);
    assert_violation(
        JPEG_BASELINE,
        &icon.bytes,
        "IconImageSequence (0088,0200)[1] > PixelData (7FE0,0010): VR OB with Bits Allocated (0028,0100) 16; native Pixel Data with Bits Allocated above 8 shall be OW (PS3.5 A.4)",
    );
}

#[test]
fn private_sequences_hold_no_pixel_overlay_or_waveform_data() {
    let mut nested = Stream::explicit();
    nested.element((0x6000, 0x3000), b"OW", &[0, 0]);
    let mut item = Stream::explicit();
    item.sequence((0x0040, 0x0555), &[nested]);
    let mut stream = Stream::explicit();
    stream
        .text((0x0009, 0x0010), b"LO", "ACME")
        .sequence((0x0009, 0x1001), &[item]);
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "(0009,1001)[1] > AcquisitionContextSequence (0040,0555)[1] > OverlayData (6000,3000): shall not be included in a Private Sequence Item or an Item nested within one (PS3.5 7.8.2)",
    );
}

#[test]
fn specific_character_set_appears_in_items_only_where_the_iod_defines_it() {
    let mut item = Stream::explicit();
    item.text((0x0008, 0x0005), b"CS", "ISO_IR 100")
        .text(OPTICAL_PATH_ID, b"SH", "1 ");
    let mut stream = Stream::explicit();
    stream.sequence(OPTICAL_PATH, &[item.clone()]);
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "OpticalPathSequence (0048,0105)[1] > SpecificCharacterSet (0008,0005): the IOD does not define Specific Character Set for the Items of this Sequence (PS3.5 7.5.3)",
    );
    // Modified Attributes Sequence Items hold any Attributes.
    let mut modified = Stream::explicit();
    modified.sequence((0x0400, 0x0550), &[item]);
    let mut original = Stream::explicit();
    original.sequence((0x0400, 0x0561), &[modified]);
    assert_conformant(EXPLICIT_LE, &original.bytes);
}

fn deflated(data_set: &[u8], pad: bool) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data_set).unwrap();
    let mut compressed = encoder.finish().unwrap();
    if pad && compressed.len() % 2 == 1 {
        compressed.push(0);
    }
    compressed
}

#[test]
fn deflated_data_sets_are_inflated_and_padded_to_even_length() {
    // Data Sets whose deflated streams have each parity.
    let (mut odd, mut even) = (None, None);
    for size in 1..64 {
        let mut stream = sorted();
        stream.text((0x0010, 0x4000), b"LT", &"A".repeat(size * 2));
        let length = deflated(&stream.bytes, false).len();
        if length % 2 == 1 {
            odd.get_or_insert(stream.bytes);
        } else {
            even.get_or_insert(stream.bytes);
        }
    }
    let (odd, even) = (odd.unwrap(), even.unwrap());
    assert_conformant(DEFLATED, &deflated(&odd, true));
    assert_conformant(DEFLATED, &deflated(&even, true));
    assert_violation(
        DEFLATED,
        &deflated(&odd, false),
        "is followed by []; an odd-length stream is followed by a single trailing NULL byte, an even-length one by none (PS3.5 A.5)",
    );
    let mut extra = deflated(&even, true);
    extra.push(0);
    assert_violation(DEFLATED, &extra, "is followed by [00]");

    let mut unsorted = Stream::explicit();
    unsorted
        .text(PATIENT_ID, b"LO", "ID")
        .text(MODALITY, b"CS", "SM");
    assert_violation(
        DEFLATED,
        &deflated(&unsorted.bytes, true),
        "Modality (0008,0060): follows (0010,0020)",
    );
}

#[test]
fn inflation_is_bounded_by_max_input_bytes() {
    let mut stream = sorted();
    stream.element((0x0009, 0x0010), b"LO", b"ACME").element(
        (0x0009, 0x1000),
        b"OB",
        &vec![0; 4096],
    );
    let path = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(path.path(), file(DEFLATED, &deflated(&stream.bytes, true))).unwrap();
    let check = super::super::run_encoding_check(path.path(), None, 1024);
    assert!(
        check.message.contains(
            "the inflated Data Set exceeds 1024 bytes (max_input_bytes); the rest is not checked"
        ),
        "{}",
        check.message
    );
}

#[test]
fn unreadable_file_meta_and_private_syntaxes_skip_the_check() {
    let unreadable = check_file(b"not a DICOM file");
    assert_eq!(
        unreadable.status,
        ValidationStatus::Skipped,
        "{}",
        unreadable.message
    );
    let private = check("1.2.3.4", &sorted().bytes);
    assert_eq!(
        private.status,
        ValidationStatus::Skipped,
        "{}",
        private.message
    );
    assert!(
        private.message.contains("Transfer Syntax 1.2.3.4"),
        "{}",
        private.message
    );
}
