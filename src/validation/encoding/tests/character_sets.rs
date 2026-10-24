//! Specific Character Set Values and the encoded bytes of text Values.

use super::*;

const CHARACTER_SET: (u16, u16) = (0x0008, 0x0005);
const INSTITUTION: (u16, u16) = (0x0008, 0x0080);
const STATION: (u16, u16) = (0x0008, 0x1010);
const PATIENT_NAME: (u16, u16) = (0x0010, 0x0010);
const PATIENT_ID: (u16, u16) = (0x0010, 0x0020);
const COMMENTS: (u16, u16) = (0x0010, 0x4000);

/// A Data Set with Specific Character Set `terms` (absent when `None`) and
/// one text element of `vr` holding `value`, padded to even length.
fn text(terms: Option<&str>, tag: (u16, u16), vr: &[u8; 2], value: &[u8]) -> Vec<u8> {
    let mut stream = Stream::explicit();
    if let Some(terms) = terms {
        let mut terms = terms.as_bytes().to_vec();
        if terms.len() % 2 == 1 {
            terms.push(b' ');
        }
        stream.element(CHARACTER_SET, b"CS", &terms);
    }
    let mut value = value.to_vec();
    if value.len() % 2 == 1 {
        value.push(b' ');
    }
    stream.element(tag, vr, &value);
    stream.bytes
}

#[track_caller]
fn violation(terms: Option<&str>, tag: (u16, u16), vr: &[u8; 2], value: &[u8], expected: &str) {
    assert_violation(EXPLICIT_LE, &text(terms, tag, vr, value), expected);
}

#[track_caller]
fn conformant(terms: Option<&str>, tag: (u16, u16), vr: &[u8; 2], value: &[u8]) {
    assert_conformant(EXPLICIT_LE, &text(terms, tag, vr, value));
}

#[test]
fn a_character_set_is_named_once() {
    violation(
        Some("ISO_IR 100\\ISO 2022 IR 100"),
        PATIENT_ID,
        b"LO",
        b"ID",
        "SpecificCharacterSet (0008,0005): Value 2 \"ISO 2022 IR 100\" names the character set of Value 1 \"ISO_IR 100\" again; a character set shall not be used more than once (PS3.3 C.12.1.1.2)",
    );
    violation(
        Some("\\ISO 2022 IR 6"),
        PATIENT_ID,
        b"LO",
        b"ID",
        "Value 2 \"ISO 2022 IR 6\" names the character set of Value 1 \"\" again",
    );
    conformant(
        Some("ISO 2022 IR 100\\ISO 2022 IR 87"),
        PATIENT_ID,
        b"LO",
        b"ID",
    );
}

#[test]
fn unicode_and_gb_character_sets_are_the_only_value() {
    for term in ["ISO_IR 192", "GB18030", "GBK"] {
        violation(
            Some(&format!("ISO 2022 IR 6\\{term}")),
            PATIENT_ID,
            b"LO",
            b"ID",
            &format!("\"{term}\" is Value 2 of 2; it may only be Value 1 and the only Value (PS3.3 C.12.1.1.2)"),
        );
    }
    conformant(Some("ISO_IR 192"), PATIENT_ID, b"LO", "Ünïcödé".as_bytes());
}

#[test]
fn utf_8_is_minimal_and_valid() {
    for overlong in [&b"A\xC0\xAF"[..], b"\xE0\x80\xAF", b"\xF0\x80\x80\xAF"] {
        violation(
            Some("ISO_IR 192"),
            PATIENT_ID,
            b"LO",
            overlong,
            "overlong UTF-8 sequence at byte",
        );
    }
    violation(
        Some("ISO_IR 192"),
        PATIENT_ID,
        b"LO",
        b"\xFF",
        "PatientID (0010,0020): bytes at byte 0 are not a ISO_IR 192 character (PS3.3 C.12.1.1.2)",
    );
    violation(
        Some("ISO_IR 192"),
        PATIENT_ID,
        b"LO",
        b"\xED\xA0\x80",
        "bytes at byte 0 are not a ISO_IR 192 character",
    );
}

#[test]
fn text_beyond_the_default_repertoire_needs_specific_character_set() {
    violation(
        None,
        PATIENT_ID,
        b"LO",
        b"\xC9A",
        "PatientID (0010,0020): byte C9H at byte 0 is outside the Default Character Repertoire, but Specific Character Set (0008,0005) is absent (PS3.5 6.1.2.2)",
    );
    conformant(Some("ISO_IR 100"), PATIENT_ID, b"LO", b"\xC9A");
}

#[test]
fn the_c1_set_is_not_used() {
    violation(
        Some("ISO_IR 100"),
        PATIENT_ID,
        b"LO",
        b"A\x85",
        "PatientID (0010,0020): byte 85H at byte 1 is a C1 control character; the C1 set shall not be used (PS3.5 6.1.1)",
    );
    // Decoders of multi-byte sets absorb C1 bytes into characters.
    violation(
        Some("\\ISO 2022 IR 149"),
        PATIENT_ID,
        b"LO",
        b"\x1b$)C\x85\xA1",
        "byte 85H at byte 4 is a C1 control character",
    );
}

#[test]
fn code_extension_needs_a_multi_valued_specific_character_set() {
    for (terms, why) in [
        (None, "Specific Character Set (0008,0005) is absent"),
        (
            Some("ISO_IR 100"),
            "Specific Character Set \"ISO_IR 100\" has a single Value",
        ),
        (
            Some("ISO_IR 192"),
            "Specific Character Set \"ISO_IR 192\" prohibits Code Extension",
        ),
        (
            Some("GB18030"),
            "Specific Character Set \"GB18030\" prohibits Code Extension",
        ),
    ] {
        violation(
            terms,
            PATIENT_ID,
            b"LO",
            b"\x1b(BAB",
            &format!("PatientID (0010,0020): ESC 02/08 04/02 at byte 0, but {why}; Code Extension shall not be used (PS3.5 6.1.2.5.4 and 6.1.3)"),
        );
    }
}

#[test]
fn escape_sequences_designate_declared_sets_of_the_tables() {
    let terms = Some("\\ISO 2022 IR 87");
    violation(
        terms,
        PATIENT_ID,
        b"LO",
        b"\x1b$)C\xB0\xA1",
        "PatientID (0010,0020): ESC 02/04 02/09 04/03 at byte 0 designates ISO-IR 149, which no Value of Specific Character Set names (PS3.5 6.1.2.3 and 6.1.2.4)",
    );
    for (escape, expected) in [
        (&b"\x1bn"[..], "ESC 06/14 at byte 0 is a locking shift; G0 and G1 are always invoked in GL and GR and locking shifts shall not be used (PS3.5 6.1.1 and 6.1.2.5.2)"),
        (b"\x1bN", "ESC 04/14 at byte 0 is a single shift; code elements G2 and G3 are not used (PS3.5 6.1.2.5.2)"),
        (b"\x1bE", "ESC 04/05 at byte 0 is a C1 control function; the C1 code element shall not be used (PS3.5 6.1.1 and 6.1.2.5.1)"),
        (b"\x1b!@", "ESC 02/01 04/00 at byte 0 designates a control set; C0 shall be the primary control set and C1 shall not be used (PS3.5 6.1.2.5.1)"),
        (b"\x1b(Z", "ESC 02/08 05/10 at byte 0 is not an escape sequence of PS3.3 Tables C.12-3 and C.12-4 (PS3.5 6.1.2.5.2 and 6.1.3)"),
        (b"AB\x1b$", "incomplete escape sequence ESC 02/04 at byte 2; ESC shall only begin ISO 2022 character set control sequences (PS3.5 6.1.3)"),
        (b"A\x0eB", "SO (0EH) at byte 1 is a locking shift; locking shifts shall not be used (PS3.5 6.1.1 and 6.1.2.5.2)"),
    ] {
        violation(terms, PATIENT_ID, b"LO", escape, expected);
    }
}

#[test]
fn the_value_1_set_is_active_at_value_line_and_delimiter_boundaries() {
    violation(
        Some("\\ISO 2022 IR 87"),
        PATIENT_ID,
        b"LO",
        b"\x1b$B0!",
        "PatientID (0010,0020): ISO-IR 87 is still designated to G0 before the end of the Value at byte 6; the Value 1 character set shall be active there (PS3.5 6.1.2.5.3)",
    );
    violation(
        Some("\\ISO 2022 IR 87"),
        COMMENTS,
        b"LT",
        b"\x1b$B0!\r\n\x1b(B",
        "PatientComments (0010,4000): ISO-IR 87 is still designated to G0 before CR at byte 5",
    );
    let latin_korean = Some("ISO 2022 IR 100\\ISO 2022 IR 149");
    violation(
        latin_korean,
        STATION,
        b"SH",
        b"\x1b$)C\xB0\xA1\\A",
        "StationName (0008,1010): ISO-IR 149 is still designated to G1 before the 05/12 Value delimiter at byte 6",
    );
    violation(
        latin_korean,
        PATIENT_NAME,
        b"PN",
        b"A=\x1b$)C\xB0\xA1^B",
        "PatientName (0010,0010): ISO-IR 149 is still designated to G1 before the \"^\" component delimiter at byte 8; the Value 1 character set shall be active there (PS3.5 6.1.2.5.3 and 6.2.1.2)",
    );
    // Without Value 1, the Default Character Repertoire returns at the end.
    violation(
        Some("\\ISO 2022 IR 87"),
        PATIENT_NAME,
        b"PN",
        b"=\x1b$B;3",
        "PatientName (0010,0010): ISO-IR 87 is still designated to G0 before the end of the Value at byte 6; the Value 1 character set shall be active there (PS3.5 6.1.2.5.3 and 6.2.1.2)",
    );
    // Each name component designates its set again before using it.
    violation(
        Some("\\ISO 2022 IR 149"),
        PATIENT_NAME,
        b"PN",
        b"Hong=\x1b$)C\xFB\xF3^\xD1\xCE",
        "PatientName (0010,0010): GR byte D1H at byte 12 with no character set designated to G1",
    );
    // A set designated in one line is designated again in the next.
    violation(
        Some("\\ISO 2022 IR 149"),
        COMMENTS,
        b"LT",
        b"\x1b$)C\xB0\xA1\r\n\xB0\xA1",
        "PatientComments (0010,4000): GR byte B0H at byte 8 with no character set designated to G1",
    );
}

#[test]
fn the_part_5_annex_person_names_pass() {
    // PS3.5 H.3.1, with "^" (5EH) bytes inside two-byte characters.
    let japanese =
        b"Yamada^Tarou=\x1b$B;3ED\x1b(B^\x1b$BB@O:\x1b(B=\x1b$B$d$^$@\x1b(B^\x1b$B$?$m$&\x1b(B";
    conformant(Some("\\ISO 2022 IR 87"), PATIENT_NAME, b"PN", japanese);
    // PS3.5 I.2: G1 needs no return when Value 1 has only G0.
    let korean = b"Hong^Gildong=\x1b$)C\xFB\xF3^\x1b$)C\xD1\xCE\xD4\xD7=\x1b$)C\xC8\xAB^\x1b$)C\xB1\xE6\xB5\xBF";
    conformant(Some("\\ISO 2022 IR 149"), PATIENT_NAME, b"PN", korean);
    conformant(
        Some("ISO_IR 13"),
        PATIENT_NAME,
        b"PN",
        b"\xD4\xCF\xC0\xDE^\xC0\xDB\xB3",
    );
}

#[test]
fn the_alphabetic_component_group_uses_value_1_without_escapes() {
    violation(
        Some("\\ISO 2022 IR 87"),
        PATIENT_NAME,
        b"PN",
        b"\x1b$B;3\x1b(B",
        "PatientName (0010,0010): ESC 02/04 04/02 at byte 0 in the alphabetic component group; ISO 2022 escapes shall not be used there (PS3.5 6.2.1.2)",
    );
    violation(
        Some("ISO_IR 192"),
        PATIENT_NAME,
        b"PN",
        "中^A".as_bytes(),
        "PatientName (0010,0010): U+4E2D at byte 2 in the alphabetic component group, outside U+0020-U+1FFF and the permitted Japanese punctuation and kana (PS3.5 6.2.1.2)",
    );
    conformant(
        Some("ISO_IR 192"),
        PATIENT_NAME,
        b"PN",
        "Wang^XiaoDong=王^小東=".as_bytes(),
    );
    violation(
        Some("GB18030"),
        PATIENT_NAME,
        b"PN",
        b"\xD6\xD0^A",
        "U+4E2D at byte",
    );
    conformant(
        Some("GB18030"),
        PATIENT_NAME,
        b"PN",
        b"Wang^XiaoDong=\xCD\xF5^\xD0\xA1",
    );
}

#[test]
fn backslash_bytes_inside_multi_byte_characters_are_not_present() {
    violation(
        Some("GBK"),
        INSTITUTION,
        b"LO",
        b"\x81\x5CA",
        "InstitutionName (0008,0080): byte 5CH at byte 1 ends a GBK character; it would separate Values, and 5CH shall not be present in SH, LO, UC and PN (PS3.5 6.2)",
    );
    violation(
        Some("\\ISO 2022 IR 87"),
        INSTITUTION,
        b"LO",
        b"\x1b$B0\\\x1b(B",
        "byte 5CH at byte 4 belongs to a two-byte ISO-IR 87 character",
    );
    // In ST, LT and UT 5CH is a character.
    conformant(Some("GBK"), COMMENTS, b"LT", b"\x81\x5CA");
}

#[test]
fn text_lines_end_in_cr_lf() {
    violation(
        None,
        COMMENTS,
        b"LT",
        b"A\nB",
        "PatientComments (0010,4000): LF at byte 1 without a preceding CR; a new line in ST, LT and UT shall be CR LF (PS3.5 6.1.3)",
    );
    violation(
        None,
        COMMENTS,
        b"LT",
        b"A\rB",
        "PatientComments (0010,4000): CR at byte 1 not followed by LF",
    );
    conformant(None, COMMENTS, b"LT", b"A\r\nB\x0cC\tD");
}

#[test]
fn escape_sequences_are_not_counted_in_value_lengths() {
    let terms = Some("\\ISO 2022 IR 87");
    // Sixteen two-byte characters: at the SH maximum.
    let mut sixteen = b"\x1b$B".to_vec();
    sixteen.extend(std::iter::repeat_n(*b"0!", 16).flatten());
    sixteen.extend_from_slice(b"\x1b(B");
    conformant(terms, STATION, b"SH", &sixteen);
    let mut seventeen = b"\x1b$B".to_vec();
    seventeen.extend(std::iter::repeat_n(*b"0!", 17).flatten());
    seventeen.extend_from_slice(b"\x1b(B");
    violation(
        terms,
        STATION,
        b"SH",
        &seventeen,
        "StationName (0008,1010) Value 1: 17 characters, not counting escape sequences, exceed the 16-character maximum (PS3.5 6.2)",
    );
    let mut long_group = b"A=\x1b$B".to_vec();
    long_group.extend(std::iter::repeat_n(*b"0!", 65).flatten());
    long_group.extend_from_slice(b"\x1b(B");
    violation(
        terms,
        PATIENT_NAME,
        b"PN",
        &long_group,
        "PatientName (0010,0010) Value 1 component group 2: 65 characters, not counting escape sequences, exceed the 64-character maximum (PS3.5 6.2 and 6.2.1.2)",
    );
}

#[test]
fn text_in_items_follows_the_item_character_set() {
    // Modified Attributes Sequence Items may carry Specific Character Set.
    let mut item = Stream::explicit();
    item.text(CHARACTER_SET, b"CS", "ISO_IR 100")
        .element(PATIENT_ID, b"LO", b"\xC9A");
    let mut modified = Stream::explicit();
    modified.sequence((0x0400, 0x0550), &[item]);
    let mut stream = Stream::explicit();
    stream
        .sequence((0x0400, 0x0561), &[modified])
        // The Item's character set does not leak into the Data Set.
        .element((0x4008, 0x0300), b"ST", b"\xC9A");
    let check = check(EXPLICIT_LE, &stream.bytes);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(check.message.contains("1 violation:"), "{}", check.message);
    assert!(
        check.message.contains(
            "(4008,0300): byte C9H at byte 0 is outside the Default Character Repertoire"
        ),
        "{}",
        check.message
    );
}
