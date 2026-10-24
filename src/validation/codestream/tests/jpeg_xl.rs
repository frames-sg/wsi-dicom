//! Vectors encoded by libjxl 0.11.2 (cjxl).

use super::{assert_conformant, assert_violation, check_frame, declared, hex};

const LOSSLESS: &str = "1.2.840.10008.1.2.4.110";
const JPEG_RECOMPRESSION: &str = "1.2.840.10008.1.2.4.111";
const ANY: &str = "1.2.840.10008.1.2.4.112";

/// 8x8 8-bit grey, `cjxl -d 0`: a bare Modular codestream.
fn grey_lossless() -> Vec<u8> {
    hex("ff0a414050dc080804010068004b2825d6818282a4b86a3b1c509f75777737000000000000e01c")
}

/// 8x8 8-bit RGB, `cjxl -d 1`: a bare VarDCT codestream.
fn rgb_lossy() -> Vec<u8> {
    hex("ff0a410600130801007801671328010050a132cab8c1cbb99e2f3f74584cdbb8ced056db1696248a8c639374028724b148038842660e65b02a49a4d66da85baa6554fdc001f897f7bfdf9e765eafd6f769779feb30c1a8aa203cc74817a58551d1dc1b94a40280a808")
}

/// 8x8 8-bit RGB, `cjxl -d 0 --container=1`: one jxlc box.
fn rgb_lossless_container() -> Vec<u8> {
    hex("0000000c4a584c200d0a870a00000014667479706a786c20000000006a786c200000006d6a786c63ff0a4140240804010068014b28b06db43fafc2282828290a0a20bbb1318c8a8a61540ca3630c1e4ad363440c630cdee51863f45c207a00df0007000c6924db8edbe849bfcb2d5a20a9bedeff7ffd48eb075686eef0b3bfaa90df70c7c680b1fc421610c801")
}

/// 8x8 grey JPEG transcoded by `cjxl`: jxlp boxes around a jbrd box.
fn grey_recompressed() -> Vec<u8> {
    hex("0000000c4a584c200d0a870a00000014667479706a786c20000000006a786c20000000136a786c7000000000ff0a414050dc08000001046a627264c10d20082d030200848090ab4a000040c851d982a138ff27888c11ca38a714b41f214398a50e32cc40e90e35581f329c7ccc80430e2a2c72d0b003db470dfdfb23cb1c76dc8186a2fb5f8c71c624976cf2c928a73c33cd35db7c33ce390b3d34d1451b7d34d2494b3d35d5555b7d35d6598b3d36d9659b7d36da69cb3d37dd75db7d37de790b3e38e1851b7e38e289472ef9e494576ef9e598671ebae8a3935ebae9a7a39e7aecb2cf4e7bedb6df8e7beec10b3f3cf1c51b7f3cf2c9472ffdf4d4576ffdf5d8670f7ef8e28f4f7ef9e69f8f7efaf0c72ffffcf4d76ffffdf8e7ff0ff0030000000b0880e000104a4649460001010000010001000003000000fd6a786c7080000001e05b001000a803087808780878feff0e0000180008000100000000008080258963c4dda487a8ea1a830b10fc0a040a242928284c43566c0c8e36c6c622b4c3c69ac3e8ce0d00900b8ed790c301b9a013404c96ab3f000cababffe43ed8114f85b3f1f4972452998dcb98580d52292f84143cc9283fc5f412f0b42114b1a41250a5fabff21a8096b0e68e21f75532aa41c1653c2d020b7411e2089e6c04a6a22832605ba38cc15589a39ae044f98a5603452f8711f82411d7446a34be9280032a081f8541ea445371f71cd039f019c12a05000000000050924809d9edb6df5408047adcbfeb347b4b6dc74902d248806d03")
}

/// 8x8 16-bit grey, `cjxl -d 0`: a container with a level box.
fn grey_16_bit() -> Vec<u8> {
    hex("0000000c4a584c200d0a870a00000014667479706a786c20000000006a786c20000000096a786c6c0a000000626a786c63ff0a41f0031437020804010030014b386998ca83f74b28488a0d4a20003acd7dee739ffbdce73e775e5e5e5e5e5e5e5e765ede8a9797979797375e5e5e525e5ede78797979797979e3e5e5e5e5e5e58d979797979797375e5e5e")
}

fn grey(syntax: &str) -> super::Declared {
    declared(syntax, (8, 8), 1, 8, "MONOCHROME2")
}

fn rgb(syntax: &str) -> super::Declared {
    declared(syntax, (8, 8), 3, 8, "RGB")
}

#[test]
fn encoder_codestreams_are_conformant_for_their_transfer_syntaxes() {
    assert_conformant(&check_frame(&grey_lossless(), &grey(LOSSLESS)));
    assert_conformant(&check_frame(&grey_lossless(), &grey(ANY)));
    assert_conformant(&check_frame(&rgb_lossless_container(), &rgb(LOSSLESS)));
    assert_conformant(&check_frame(&rgb_lossy(), &rgb(ANY)));
    assert_conformant(&check_frame(
        &grey_recompressed(),
        &grey(JPEG_RECOMPRESSION),
    ));
    assert_conformant(&check_frame(&grey_recompressed(), &grey(ANY)));
    assert_conformant(&check_frame(
        &grey_16_bit(),
        &declared(LOSSLESS, (8, 8), 1, 16, "MONOCHROME2"),
    ));
}

#[test]
fn headers_agree_with_the_image_pixel_attributes() {
    let codestream = grey_lossless();
    for (declared, expected) in [
        (
            declared(LOSSLESS, (16, 8), 1, 8, "MONOCHROME2"),
            "Frame 1: JPEG XL image height is 8 but Rows (0028,0010) is 16 (PS3.5 2026c 8.2.15)",
        ),
        (
            declared(LOSSLESS, (8, 4), 1, 8, "MONOCHROME2"),
            "JPEG XL image width is 8 but Columns (0028,0011) is 4",
        ),
        (
            declared(LOSSLESS, (8, 8), 3, 8, "RGB"),
            "JPEG XL channel count (color plus extra) is 1 but Samples per Pixel (0028,0002) is 3",
        ),
        (
            declared(LOSSLESS, (8, 8), 1, 16, "MONOCHROME2"),
            "JPEG XL bits per sample is 8 but Bits Stored (0028,0101) is 16",
        ),
    ] {
        assert_violation(&check_frame(&codestream, &declared), expected);
    }
    assert_violation(
        &check_frame(&grey_16_bit(), &grey(LOSSLESS)),
        "JPEG XL bits per sample is 16 but Bits Stored (0028,0101) is 8",
    );
}

#[test]
fn coding_mode_is_the_one_its_transfer_syntax_identifies() {
    assert_violation(
        &check_frame(&rgb_lossy(), &rgb(LOSSLESS)),
        "first frame uses VarDCT encoding, which is lossy, not the lossless JPEG XL coding Transfer Syntax 1.2.840.10008.1.2.4.110 identifies (PS3.5 2026c A.4.12)",
    );
    assert_violation(
        &check_frame(&grey_recompressed(), &grey(LOSSLESS)),
        "carries JPEG bitstream reconstruction (jbrd) data of a transcoded JPEG, not the lossless JPEG XL coding Transfer Syntax 1.2.840.10008.1.2.4.110 identifies",
    );
    let bare = check_frame(&grey_lossless(), &grey(JPEG_RECOMPRESSION));
    assert_violation(&bare, "has no JPEG bitstream reconstruction (jbrd) box, so it is not the reversible JPEG transcoding Transfer Syntax 1.2.840.10008.1.2.4.111 identifies (PS3.5 2026c A.4.12)");
    assert_violation(&bare, "first frame uses Modular encoding");
}

#[test]
fn frames_hold_a_jpeg_xl_codestream() {
    assert_violation(
        &check_frame(&crate::test_support::encode_test_gray_jpeg(8, 8, 1), &grey(LOSSLESS)),
        "begins with bytes FF D8 FF DB (a JPEG stream: SOI), not the JPEG XL codestream or container signature Transfer Syntax 1.2.840.10008.1.2.4.110 identifies (PS3.10 2026c 7.2 and PS3.5 2026c A.4.12)",
    );
    let container = rgb_lossless_container();
    assert_violation(
        &check_frame(&container[..40], &rgb(LOSSLESS)),
        "JPEG XL container is malformed: box at byte 32 runs past the Frame",
    );
}

#[test]
fn truncated_headers_are_reported_without_panicking() {
    let codestream = grey_lossless();
    // The first frame header begins at byte 7.
    for length in 2..8 {
        assert_violation(
            &check_frame(&codestream[..length], &grey(LOSSLESS)),
            "JPEG XL headers are truncated or malformed",
        );
    }
    let container = grey_recompressed();
    for length in 0..container.len() {
        let _ = check_frame(&container[..length], &grey(JPEG_RECOMPRESSION));
    }
}
