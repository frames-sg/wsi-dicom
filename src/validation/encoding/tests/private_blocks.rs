//! PS3.5 7.8.1 Private Creator reservations.

use super::*;

const CREATOR: (u16, u16) = (0x0029, 0x0010);
const ELEMENT: (u16, u16) = (0x0029, 0x1001);
const OPTICAL_PATH: (u16, u16) = (0x0048, 0x0105);

fn block(creator: &str) -> Stream {
    let mut stream = Stream::explicit();
    stream
        .text(CREATOR, b"LO", creator)
        .text(ELEMENT, b"LO", "V ");
    stream
}

#[test]
fn a_reserved_block_passes() {
    assert_conformant(EXPLICIT_LE, &block("ACME").bytes);
    let mut implicit = Stream::implicit();
    implicit
        .text(CREATOR, b"LO", "ACME")
        .text(ELEMENT, b"LO", "V ");
    assert_conformant(IMPLICIT_LE, &implicit.bytes);
}

#[test]
fn private_elements_need_their_private_creator() {
    let mut stream = Stream::explicit();
    stream.text(ELEMENT, b"LO", "V ");
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "(0029,1001): no Private Creator (0029,0010) reserves its block in this Data Set (PS3.5 7.8.1)",
    );
}

#[test]
fn items_reserve_their_own_blocks() {
    let mut item = Stream::explicit();
    item.text((0x0048, 0x0106), b"SH", "1 ")
        .text(ELEMENT, b"LO", "V ");
    let mut stream = block("ACME");
    stream.sequence(OPTICAL_PATH, &[item]);
    assert_violation(
        EXPLICIT_LE,
        &stream.bytes,
        "OpticalPathSequence (0048,0105)[1] > (0029,1001): no Private Creator (0029,0010) reserves its block in this Item; Items do not inherit the reservations of the Data Set that contains them (PS3.5 7.8.1)",
    );
    // PS3.3 C.12.1.1.9.1: Modified Attributes Sequence Items likewise.
    let mut modified_item = Stream::explicit();
    modified_item.text(ELEMENT, b"LO", "V ");
    let mut modified = Stream::explicit();
    modified.sequence((0x0400, 0x0550), &[modified_item]);
    let mut original = Stream::explicit();
    original.sequence((0x0400, 0x0561), &[modified]);
    assert_violation(
        EXPLICIT_LE,
        &original.bytes,
        "ModifiedAttributesSequence (0400,0550)[1] > (0029,1001): no Private Creator (0029,0010) reserves its block in this Item",
    );
}

#[test]
fn private_creators_hold_one_unique_default_repertoire_code() {
    assert_violation(
        EXPLICIT_LE,
        &block("  ").bytes,
        "(0029,0010): Private Creator without an identification code",
    );
    let mut latin = Stream::explicit();
    latin
        .text((0x0008, 0x0005), b"CS", "ISO_IR 100")
        .element(CREATOR, b"LO", b"ACM\xC9")
        .text(ELEMENT, b"LO", "V ");
    assert_violation(
        EXPLICIT_LE,
        &latin.bytes,
        "(0029,0010): Private Creator byte C9H is outside the Default Character Repertoire",
    );
    let mut repeated = Stream::explicit();
    repeated
        .text(CREATOR, b"LO", "ACME")
        .text((0x0029, 0x0011), b"LO", "ACME");
    assert_violation(
        EXPLICIT_LE,
        &repeated.bytes,
        "(0029,0011): identification code \"ACME\" already reserves block (0029,0010); a Private Creator identifier may be used only once within a Group (PS3.5 7.8.1)",
    );
    let mut implicit = Stream::implicit();
    implicit.text(CREATOR, b"LO", "AC\\M");
    assert_violation(
        IMPLICIT_LE,
        &implicit.bytes,
        "(0029,0010): Private Creator holds several Values; its VM shall be 1 (PS3.5 7.8.1)",
    );
}

#[test]
fn private_groups_do_not_use_the_unreservable_elements() {
    for element in [0x0001, 0x0100] {
        let mut stream = Stream::explicit();
        stream.text((0x0029, element), b"LO", "V ");
        assert_violation(
            EXPLICIT_LE,
            &stream.bytes,
            &format!("(0029,{element:04X}): Elements (gggg,0001-000F) and (gggg,0100-0FFF) of an odd group shall not be used (PS3.5 7.8.1)"),
        );
    }
}
