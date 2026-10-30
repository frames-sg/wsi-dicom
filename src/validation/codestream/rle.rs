//! RLE compressed Frames (PS3.5 8.2.2 and Annex G): the header, and a walk
//! of each segment's packets that validates the encoding without
//! reconstructing pixels.

use super::{Frame, FrameReport};

const HEADER_BYTES: usize = 64;
const MAX_SEGMENTS: usize = 15;

pub(super) fn check(frame: &Frame<'_>, report: &mut FrameReport<'_>) {
    let bytes = frame.bytes;
    let Some(header) = bytes.get(..HEADER_BYTES) else {
        let length = bytes.len();
        report.violation("PS3.5 G.5", || {
            format!("is {length} bytes, shorter than the 64-byte RLE Header")
        });
        return;
    };
    let words: Vec<usize> = header
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| usize::try_from(u32::from_le_bytes(*word)).unwrap_or(usize::MAX))
        .collect();
    let (count, offsets) = (words[0], &words[1..]);
    if count == 0 || count > MAX_SEGMENTS {
        report.violation("PS3.5 G.5", || {
            format!("RLE Header declares {count} RLE Segments; it describes 1 to 15")
        });
        return;
    }
    let declared = frame.declared;
    // Each byte of the Padded Composite Pixel Code is one segment (G.2).
    if let (Some(samples), Some(allocated)) = (
        declared.samples_per_pixel.value,
        declared.bits_allocated.value,
    ) {
        let expected = (u64::from(samples) * u64::from(allocated)).div_ceil(8);
        if expected != count as u64 {
            report.violation("PS3.5 8.2.2 and PS3.5 G.2", || {
                format!("RLE Header declares {count} RLE Segments but Samples per Pixel (0028,0002) {samples} with Bits Allocated (0028,0100) {allocated} makes {expected} Byte Segments")
            });
        }
    }
    if offsets[count..].iter().any(|offset| *offset != 0) {
        report.violation("PS3.5 G.5", || {
            format!(
                "RLE Header offsets after its {count} segments are {}, but all unused segment offsets are zero",
                list(&offsets[count..])
            )
        });
    }
    let offsets = &offsets[..count];
    if offsets[0] != HEADER_BYTES
        || offsets.windows(2).any(|pair| pair[0] >= pair[1])
        || offsets[count - 1] >= bytes.len()
    {
        report.violation("PS3.5 G.4 and PS3.5 G.5", || {
            format!(
                "RLE Segment offsets {} do not start at 64 and increase within the Frame's {} bytes",
                list(offsets),
                bytes.len()
            )
        });
        return;
    }
    let pixels = declared
        .rows
        .value
        .zip(declared.columns.value)
        .and_then(|(rows, columns)| {
            Some((usize::try_from(rows).ok()?, usize::try_from(columns).ok()?))
        });
    for (index, start) in offsets.iter().enumerate() {
        let end = offsets.get(index + 1).copied().unwrap_or(bytes.len());
        let segment = &bytes[*start..end];
        let number = index + 1;
        if !segment.len().is_multiple_of(2) {
            report.violation("PS3.5 G.3", || {
                format!(
                    "RLE Segment {number} is {} bytes; each RLE Segment is an even number of bytes",
                    segment.len()
                )
            });
        }
        if let Some(offset) = forbidden_header(segment) {
            let in_frame = start + offset;
            report.violation("PS3.5 G.3.1", || {
                format!("RLE Segment {number} has a run header of -128 (80H) at byte {offset} of the segment (byte {in_frame} of the Frame); the value of -128 may not be used to prefix a byte value")
            });
        }
        if let Some((rows, columns)) = pixels {
            walk_segment(segment, number, rows, columns, report);
        }
    }
}

/// The offset of the first run header of -128 in `segment` (G.3.1), found by
/// stepping from header to header: a literal run header n is followed by
/// n + 1 bytes, any other header by the one byte it replicates. The walk
/// stops at the end of the segment, so it needs no Rows or Columns.
fn forbidden_header(segment: &[u8]) -> Option<usize> {
    let mut position = 0usize;
    while let Some(&header) = segment.get(position) {
        let step = match header as i8 {
            -128 => return Some(position),
            literal @ 0..=127 => usize::from(literal as u8) + 2,
            _ => 2,
        };
        position = position.checked_add(step)?;
    }
    None
}

/// Packet rules found while walking one segment, each reported once.
#[derive(Default)]
struct Findings {
    crossing: Option<(usize, usize)>,
    hidden_repeat: Option<usize>,
}

/// Walks the packets of one segment, which encodes `rows` rows of `columns`
/// bytes (G.3.1, G.3.2). Each step consumes at least one byte.
fn walk_segment(
    segment: &[u8],
    number: usize,
    rows: usize,
    columns: usize,
    report: &mut FrameReport<'_>,
) {
    let Some(expected) = rows.checked_mul(columns).filter(|_| columns > 0) else {
        return;
    };
    let mut findings = Findings::default();
    let (mut position, mut produced) = (0usize, 0usize);
    while produced < expected {
        let Some(&header) = segment.get(position) else {
            report.violation("PS3.5 8.2.2", || {
                format!("RLE Segment {number} decodes to {produced} bytes, fewer than the {expected} of Rows (0028,0010) by Columns (0028,0011)")
            });
            break;
        };
        position += 1;
        let run = match header as i8 {
            // -128 outputs nothing (G.3.2); forbidden_header reports it.
            -128 => continue,
            literal @ 0..=127 => {
                let count = usize::from(literal as u8) + 1;
                let Some(bytes) = segment.get(position..position + count) else {
                    report.violation("PS3.5 G.3.1", || {
                        format!("RLE Segment {number} ends inside a literal run of {count} bytes")
                    });
                    return;
                };
                if findings.hidden_repeat.is_none()
                    && bytes
                        .windows(3)
                        .any(|window| window[0] == window[1] && window[1] == window[2])
                {
                    findings.hidden_repeat = Some(produced);
                }
                position += count;
                count
            }
            replicate => {
                if position >= segment.len() {
                    report.violation("PS3.5 G.3.1", || {
                        format!("RLE Segment {number} ends inside a replicate run")
                    });
                    return;
                }
                position += 1;
                usize::from(replicate.unsigned_abs()) + 1
            }
        };
        let column = produced % columns;
        if findings.crossing.is_none() && column + run > columns {
            findings.crossing = Some((produced / columns, column));
        }
        produced += run;
    }
    if let Some((row, column)) = findings.crossing {
        report.violation("PS3.5 G.3.1", || {
            format!("RLE Segment {number} has a run starting at row {row}, column {column} that crosses the row boundary; each row is encoded separately")
        });
    }
    if let Some(offset) = findings.hidden_repeat {
        report.violation("PS3.5 G.3.1", || {
            format!("RLE Segment {number} has a literal run at decoded byte {offset} that contains a three-byte repeat, which is encoded as a Replicate Run")
        });
    }
    let trailing = &segment[position.min(segment.len())..];
    match trailing {
        [] | [0] => {}
        [_] => report.violation("PS3.5 G.3", || {
            format!("RLE Segment {number} is padded with a nonzero byte; the pad byte is zero")
        }),
        _ => report.violation("PS3.5 8.2.2", || {
            format!(
                "RLE Segment {number} has {} bytes after the runs that decode its {expected} bytes",
                trailing.len()
            )
        }),
    }
}

fn list(values: &[usize]) -> String {
    values
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
