//! Codestream rule tests: real codestreams from the crate's encoders (and
//! small vectors from dcmtk and libjxl where the crate has no encoder), and
//! malformed ones made by patching their marker bytes.

mod deflate;
mod j2k;
mod jpeg;
mod jpeg_ls;
mod jpeg_xl;
mod rle;
mod rule;

use std::path::Path;

use super::{Attribute, Codec, Declared, Frame, FrameReport, CODESTREAM_RULE};
use crate::validation::iod::Family;
use crate::validation::{ValidationCheck, ValidationStatus};

/// Image Pixel attributes of a test Frame, unsigned and with Bits Allocated
/// the byte multiple of `bits_stored`.
fn declared(
    syntax: &str,
    (rows, columns): (u32, u32),
    samples: u32,
    bits_stored: u32,
    photometric: &str,
) -> Declared {
    let attribute = |name, value| Attribute {
        name,
        value: Some(value),
    };
    Declared {
        syntax: syntax.into(),
        rows: attribute("Rows (0028,0010)", rows),
        columns: attribute("Columns (0028,0011)", columns),
        samples_per_pixel: attribute("Samples per Pixel (0028,0002)", samples),
        bits_stored: attribute("Bits Stored (0028,0101)", bits_stored),
        bits_allocated: attribute("Bits Allocated (0028,0100)", bits_stored.div_ceil(8) * 8),
        pixel_representation: attribute("Pixel Representation (0028,0103)", 0),
        photometric: Some(photometric.into()),
    }
}

/// The rule's verdict on one Frame held by one Fragment.
fn check_frame(bytes: &[u8], declared: &Declared) -> ValidationCheck {
    check_fragments(&[bytes], declared)
}

/// The rule's verdict on one Frame spanning `fragments`.
fn check_fragments(fragments: &[&[u8]], declared: &Declared) -> ValidationCheck {
    let bytes = fragments.concat();
    let fragment_starts: Vec<usize> = fragments
        .iter()
        .scan(0, |end, fragment| {
            *end += fragment.len();
            Some(*end)
        })
        .take(fragments.len() - 1)
        .collect();
    let codec = Codec::of(&declared.syntax).expect("a codestream transfer syntax");
    let mut family = Family::default();
    let mut report = FrameReport {
        family: &mut family,
        number: 1,
    };
    codec.check(
        &Frame {
            bytes: &bytes,
            fragment_starts: &fragment_starts,
            declared,
            profile: None,
        },
        &mut report,
    );
    family.check(Path::new("frame.dcm"), CODESTREAM_RULE, "codestream")
}

#[track_caller]
fn assert_conformant(check: &ValidationCheck) {
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
}

#[track_caller]
fn assert_violation(check: &ValidationCheck, expected: &str) {
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(expected),
        "expected {expected:?} in {:?}",
        check.message
    );
}

/// Offset of the first occurrence of `pattern`.
#[track_caller]
fn find(bytes: &[u8], pattern: &[u8]) -> usize {
    bytes
        .windows(pattern.len())
        .position(|window| window == pattern)
        .expect("pattern present")
}

/// `bytes` with `inserted` placed at `at`.
fn insert(bytes: &[u8], at: usize, inserted: &[u8]) -> Vec<u8> {
    [&bytes[..at], inserted, &bytes[at..]].concat()
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).expect("hex digits"))
        .collect()
}

/// Deterministic interleaved samples with smooth gradients and some texture.
fn samples(width: u32, height: u32, components: u32) -> Vec<u8> {
    (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                (0..components).map(move |c| ((x * 7 + y * 13 + c * 61 + (x ^ y)) & 0xFF) as u8)
            })
        })
        .collect()
}
