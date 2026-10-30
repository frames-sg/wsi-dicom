use super::{
    assert_conformant, assert_violation, check_fragments, check_frame, declared, find, hex,
};

const LOSSLESS: &str = "1.2.840.10008.1.2.4.80";
const NEAR_LOSSLESS: &str = "1.2.840.10008.1.2.4.81";

/// A 6x4 RGB frame that dcmtk 3.7 (CharLS) encoded losslessly, line
/// interleaved.
pub(super) fn dcmtk_lossless() -> Vec<u8> {
    hex("ffd8fff70011080004000603011100021100031100ffda000c030100020003000001000000016d9000000cf9897ad95fe6924faedcb76acf957800000276000002ee367400000177ca3700a72360001c0c80ffd9")
}

/// The same frame near-lossless with NEAR 3.
fn dcmtk_near_lossless() -> Vec<u8> {
    hex("ffd8fff70011080004000603011100021100031100ffda000c03010002000300030100005400000273a6ff6d55f5c3ff7f3f810040088002cfc100200020c0000017c0ffffd9")
}

fn rgb(syntax: &str) -> super::Declared {
    declared(syntax, (4, 6), 3, 8, "RGB")
}

/// Offset of the SOS parameters that follow its component selectors.
fn scan_parameters(stream: &[u8]) -> usize {
    find(stream, &[0xFF, 0xDA]) + 4 + 1 + 3 * 2
}

#[test]
fn dcmtk_frames_are_conformant() {
    assert_conformant(&check_frame(&dcmtk_lossless(), &rgb(LOSSLESS)));
    assert_conformant(&check_frame(&dcmtk_lossless(), &rgb(NEAR_LOSSLESS)));
    assert_conformant(&check_frame(&dcmtk_near_lossless(), &rgb(NEAR_LOSSLESS)));
}

#[test]
fn frame_header_agrees_with_the_image_pixel_attributes() {
    let stream = dcmtk_lossless();
    for (declared, expected) in [
        (
            declared(LOSSLESS, (8, 6), 3, 8, "RGB"),
            "SOF55 number of lines is 4 but Rows (0028,0010) is 8 (PS3.5 2026d 8.2.3)",
        ),
        (
            declared(LOSSLESS, (4, 4), 3, 8, "RGB"),
            "SOF55 samples per line is 6 but Columns (0028,0011) is 4",
        ),
        (
            declared(LOSSLESS, (4, 6), 1, 8, "MONOCHROME2"),
            "SOF55 number of components is 3 but Samples per Pixel (0028,0002) is 1",
        ),
        (
            declared(LOSSLESS, (4, 6), 3, 12, "RGB"),
            "SOF55 sample precision is 8 but Bits Stored (0028,0101) is 12",
        ),
    ] {
        assert_violation(&check_frame(&stream, &declared), expected);
    }
    let mut subsampled = stream.clone();
    subsampled[find(&stream, &[0xFF, 0xF7]) + 11] = 0x21;
    assert_violation(
        &check_frame(&subsampled, &rgb(LOSSLESS)),
        "SOF55 sampling factors 2x1, 1x1, 1x1 encode planes of different sizes, which Photometric Interpretation RGB does not describe (PS3.3 2026d C.7.6.3.1.1)",
    );
}

#[test]
fn lossless_transfer_syntax_admits_only_near_zero() {
    assert_violation(
        &check_frame(&dcmtk_near_lossless(), &rgb(LOSSLESS)),
        "SOS NEAR 3 selects near-lossless coding, but Transfer Syntax 1.2.840.10008.1.2.4.80 identifies the lossless mode (NEAR 0) (PS3.5 2026d A.4.3)",
    );
}

#[test]
fn frames_carry_all_parameter_specifications() {
    // Component 2 selects mapping table 5, which no LSE segment defines.
    let mut stream = dcmtk_lossless();
    let sos = find(&stream, &[0xFF, 0xDA]);
    stream[sos + 5 + 3] = 5;
    assert_violation(
        &check_frame(&stream, &rgb(LOSSLESS)),
        "SOS component 2 uses mapping table 5, which no LSE segment specifies: abbreviated format instead of the interchange format (PS3.5 2026d A.4.3)",
    );
    // An LSE ID 2 segment specifying table 5 (one-byte entries) completes it.
    let table = [0xFF, 0xF8, 0x00, 0x06, 0x02, 0x05, 0x01, 0x00];
    let defined = super::insert(&stream, 2, &table);
    assert_conformant(&check_frame(&defined, &rgb(LOSSLESS)));

    let original = dcmtk_lossless();
    let frame_header = find(&original, &[0xFF, 0xF7]);
    let without_frame_header = [
        &original[..frame_header],
        &original[frame_header + 2 + 0x11..],
    ]
    .concat();
    assert_violation(
        &check_frame(&without_frame_header, &rgb(LOSSLESS)),
        "has no SOF55 frame header before its first scan, so it is not in the JPEG-LS interchange format",
    );
    let parameters = scan_parameters(&original);
    assert_eq!(original[parameters], 0, "NEAR of the lossless frame");
}

#[test]
fn each_frame_is_one_complete_separately_encoded_stream() {
    let stream = dcmtk_lossless();
    assert_violation(
        &check_frame(&stream[..stream.len() - 2], &rgb(LOSSLESS)),
        "does not end with EOI (FFD9H), so the Frame is not one complete, separately encoded JPEG-LS stream (PS3.5 2026d A.4.3)",
    );
    assert_violation(
        &check_fragments(&[&stream, &stream], &rgb(LOSSLESS)),
        "its Fragment 2 begins another JPEG-LS stream after the previous Fragment ended one",
    );
}

#[test]
fn stream_kind_is_the_one_its_transfer_syntax_identifies() {
    let baseline = crate::test_support::encode_test_jpeg(6, 4, [1, 2, 3]);
    assert_violation(
        &check_frame(&baseline, &rgb(LOSSLESS)),
        "SOF0 (baseline DCT) is an ISO/IEC 10918-1 frame, not the JPEG-LS SOF55 of Transfer Syntax 1.2.840.10008.1.2.4.80 (PS3.10 2026d 7.2 and PS3.5 2026d A.4.3)",
    );
    assert_violation(
        &check_frame(&[0xFF, 0x4F, 0xFF, 0x51], &rgb(LOSSLESS)),
        "begins with bytes FF 4F FF 51 (a JPEG 2000 codestream: SOC, SIZ), not the SOI marker",
    );
}
