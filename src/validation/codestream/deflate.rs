//! Deflated Image Frame Compression (PS3.5 8.2.16 and A.4.13): each Frame is
//! one raw Deflate bit stream (RFC 1951) of its Native Format bytes, padded
//! with a single NULL byte when the stream has an odd length.
//!
//! The stream is inflated into a fixed scratch buffer and only counted, so a
//! hostile Frame costs at most its Native length (bounded by the Frame limit)
//! in work and the buffer in memory.

use flate2::{Decompress, FlushDecompress, Status};

use super::super::pixel_structure::{is_padded_length, NativeFrame};
use super::{Frame, FrameReport};

const PROCESS: &str = "PS3.5 A.4.13";
/// Inflated bytes produced per call; each call's output is discarded.
const SCRATCH_BYTES: usize = 64 * 1024;

pub(super) fn check(frame: &Frame<'_>, max_frame_bytes: usize, report: &mut FrameReport<'_>) {
    // Absent or malformed Image Pixel attributes are the IOD checks' to
    // report; without them the Native length is unknown.
    let Some(expected) = native_bytes(frame) else {
        return;
    };
    if expected > max_frame_bytes as u64 {
        report.violation(PROCESS, || {
            format!(
                "inflates to a {expected}-byte Native Frame, beyond the {max_frame_bytes}-byte Frame limit of this validation, so its Deflate bit stream was not inflated"
            )
        });
        return;
    }
    let Some(stream) = inflate(frame.bytes, expected, report) else {
        return;
    };
    if stream.inflated != expected && !is_padded_length(stream.inflated, expected) {
        report.violation(PROCESS, || {
            format!(
                "inflates to {} bytes, but its Native Frame of {} is {expected} bytes",
                stream.inflated,
                frame_size(frame)
            )
        });
    }
    let trailing = &frame.bytes[stream.consumed..];
    let odd = stream.consumed % 2 == 1;
    let conformant = match trailing {
        [] => !odd,
        [0] => odd,
        _ => false,
    };
    if !conformant {
        let shown = &trailing[..trailing.len().min(4)];
        report.violation(PROCESS, || {
            format!(
                "its Deflate bit stream of {} bytes is followed by {} byte(s) {shown:02X?}; an odd-length stream is followed by a single trailing NULL byte, an even-length one by none",
                stream.consumed,
                trailing.len()
            )
        });
    }
}

/// How a Frame's Deflate bit stream ended.
struct Inflated {
    /// Bytes of the Frame the bit stream occupies.
    consumed: usize,
    /// Bytes it inflates to.
    inflated: u64,
}

/// Inflates `bytes` as one raw Deflate bit stream, counting at most one byte
/// more than an even-padded `expected`; `None` after reporting a stream that
/// is malformed, incomplete or too long.
fn inflate(bytes: &[u8], expected: u64, report: &mut FrameReport<'_>) -> Option<Inflated> {
    let limit = expected + expected % 2;
    let mut inflater = Decompress::new(false);
    let mut scratch = vec![0u8; SCRATCH_BYTES];
    loop {
        let consumed = usize::try_from(inflater.total_in()).unwrap_or(usize::MAX);
        let before = (inflater.total_in(), inflater.total_out());
        let input = bytes.get(consumed..).unwrap_or_default();
        match inflater.decompress(input, &mut scratch, FlushDecompress::Finish) {
            Ok(Status::StreamEnd) => {
                return Some(Inflated {
                    consumed: usize::try_from(inflater.total_in()).unwrap_or(usize::MAX),
                    inflated: inflater.total_out(),
                })
            }
            Ok(Status::Ok | Status::BufError) => {}
            Err(error) => {
                report.violation(PROCESS, || {
                    format!("is not a Deflate bit stream (RFC 1951): {error}")
                });
                return None;
            }
        }
        if inflater.total_out() > limit {
            report.violation(PROCESS, || {
                format!("inflates to more than its {expected}-byte Native Frame")
            });
            return None;
        }
        if (inflater.total_in(), inflater.total_out()) == before {
            let length = bytes.len();
            report.violation(PROCESS, || {
                format!(
                    "ends after {length} bytes inside its Deflate bit stream, which has no final block (RFC 1951 3.2.3)"
                )
            });
            return None;
        }
    }
}

fn native_frame(frame: &Frame<'_>) -> Option<NativeFrame> {
    let declared = frame.declared;
    let value = |attribute: super::Attribute| attribute.value.map(u64::from).filter(|v| *v > 0);
    NativeFrame::new(
        value(declared.rows)?,
        value(declared.columns)?,
        value(declared.samples_per_pixel)?,
        value(declared.bits_allocated)?,
        declared.photometric_is(&["YBR_FULL_422"]),
    )
}

fn native_bytes(frame: &Frame<'_>) -> Option<u64> {
    native_frame(frame)?.bytes(1).ok()
}

fn frame_size(frame: &Frame<'_>) -> String {
    native_frame(frame).map_or_else(String::new, |native| native.describe(1))
}
