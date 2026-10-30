//! Deflated Image Frame Compression Frames made by flate2's raw Deflate
//! encoder, and malformed ones made by cutting or padding them.

use std::io::Write;
use std::path::Path;

use flate2::write::DeflateEncoder;
use flate2::Compression;

use super::{assert_conformant, assert_violation, check_frame, declared};
use super::{Codec, Frame, FrameReport, CODESTREAM_RULE};
use crate::validation::iod::Family;
use crate::validation::ValidationCheck;

const DEFLATED_FRAMES: &str = "1.2.840.10008.1.2.8.1";

fn deflate(native: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(native).expect("deflate");
    encoder.finish().expect("deflate")
}

/// A grey `rows` x 6 Frame whose raw Deflate bit stream has the parity
/// `odd`, with the stream and the Native bytes.
fn stream(rows: u32, odd: bool) -> (Vec<u8>, Vec<u8>) {
    (0u8..=255)
        .map(|seed| {
            let native: Vec<u8> = (0..rows * 6)
                .map(|index| (index as u8).wrapping_mul(seed).wrapping_add(seed))
                .collect();
            (deflate(&native), native)
        })
        .find(|(stream, _)| (stream.len() % 2 == 1) == odd)
        .expect("a stream of each parity")
}

fn grey(rows: u32) -> super::Declared {
    declared(DEFLATED_FRAMES, (rows, 6), 1, 8, "MONOCHROME2")
}

/// The verdict on one Frame under a Frame limit of `max_frame_bytes`.
fn check_limited(
    bytes: &[u8],
    declared: &super::Declared,
    max_frame_bytes: usize,
) -> ValidationCheck {
    let mut family = Family::default();
    Codec::Deflate { max_frame_bytes }.check(
        &Frame {
            bytes,
            fragment_starts: Some(&[]),
            declared,
            profile: None,
        },
        &mut FrameReport {
            family: &mut family,
            number: 1,
        },
    );
    family.check(Path::new("frame.dcm"), CODESTREAM_RULE, "codestream")
}

#[test]
fn even_streams_stand_alone_and_odd_streams_end_in_one_null_byte() {
    let (even, _) = stream(4, false);
    assert_conformant(&check_frame(&even, &grey(4)));
    let (odd, _) = stream(4, true);
    assert_conformant(&check_frame(&[odd.as_slice(), &[0]].concat(), &grey(4)));
    // RGB and 16-bit Frames inflate to their Native length too.
    let rgb: Vec<u8> = (0..4 * 6 * 3).map(|index| index as u8).collect();
    let rgb_stream = deflate(&rgb);
    let padded = [rgb_stream.as_slice(), &vec![0; rgb_stream.len() % 2]].concat();
    assert_conformant(&check_frame(
        &padded,
        &declared(DEFLATED_FRAMES, (4, 6), 3, 8, "RGB"),
    ));
    let deep = deflate(&[7u8; 4 * 6 * 2]);
    let padded = [deep.as_slice(), &vec![0; deep.len() % 2]].concat();
    assert_conformant(&check_frame(
        &padded,
        &declared(DEFLATED_FRAMES, (4, 6), 1, 16, "MONOCHROME2"),
    ));
}

#[test]
fn the_null_pad_follows_only_an_odd_stream_and_nothing_else_follows() {
    let (odd, _) = stream(4, true);
    assert_violation(
        &check_frame(&odd, &grey(4)),
        &format!(
            "its Deflate bit stream of {} bytes is followed by 0 byte(s) []; an odd-length stream is followed by a single trailing NULL byte, an even-length one by none (PS3.5 2026d A.4.13)",
            odd.len()
        ),
    );
    let (even, _) = stream(4, false);
    assert_violation(
        &check_frame(&[even.as_slice(), &[0, 0]].concat(), &grey(4)),
        &format!(
            "its Deflate bit stream of {} bytes is followed by 2 byte(s) [00, 00]",
            even.len()
        ),
    );
    assert_violation(
        &check_frame(&[odd.as_slice(), &[0xAB]].concat(), &grey(4)),
        "is followed by 1 byte(s) [AB]",
    );
}

#[test]
fn the_stream_inflates_to_exactly_the_native_frame() {
    let (even, _) = stream(4, false);
    assert_violation(
        &check_frame(&even, &grey(5)),
        "inflates to 24 bytes, but its Native Frame of 5 Rows x 6 Columns x 1 Samples per Pixel x 1 Frame(s) at 8 Bits Allocated is 30 bytes (PS3.5 2026d A.4.13)",
    );
    assert_violation(
        &check_frame(&even, &grey(2)),
        "inflates to 24 bytes, but its Native Frame of 2 Rows x 6 Columns",
    );
    // Inflation stops once the output passes the Native length.
    let bomb = deflate(&vec![0; 1 << 20]);
    let padded = [bomb.as_slice(), &vec![0; bomb.len() % 2]].concat();
    assert_violation(
        &check_frame(&padded, &grey(4)),
        "inflates to more than its 24-byte Native Frame",
    );
}

#[test]
fn malformed_and_truncated_streams_are_reported() {
    let (even, _) = stream(4, false);
    assert_violation(
        &check_frame(&even[..even.len() / 2], &grey(4)),
        "inside its Deflate bit stream, which has no final block (RFC 1951 3.2.3)",
    );
    // Block type 3 is reserved (RFC 1951 3.2.3).
    assert_violation(
        &check_frame(&[0x07, 0x00], &grey(4)),
        "is not a Deflate bit stream (RFC 1951)",
    );
}

#[test]
fn frames_beyond_the_frame_limit_are_reported_not_inflated() {
    let (even, native) = stream(4, false);
    assert_conformant(&check_limited(&even, &grey(4), native.len()));
    assert_violation(
        &check_limited(&even, &grey(4), native.len() - 1),
        "inflates to a 24-byte Native Frame, beyond the 23-byte Frame limit of this validation, so its Deflate bit stream was not inflated",
    );
}
