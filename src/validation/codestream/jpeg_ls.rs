//! JPEG-LS (ISO/IEC 14495-1) interchange format streams (PS3.5 8.2.3 and
//! A.4.3).

use super::markers::{
    ends_with_end_of_image, header_segments, is_start_of_frame, second_stream_fragment,
    FrameHeader, ScanHeader, EOI, SOI, SOS,
};
use super::{signature, Frame, FrameReport};

const INTERCHANGE: &str = "PS3.5 A.4.3";
const CONSISTENT: &str = "PS3.5 8.2.3";
const START_OF_FRAME_55: u8 = 0xF7;
const PRESET_PARAMETERS: u8 = 0xF8;

pub(super) fn check(frame: &Frame<'_>, near_lossless: bool, report: &mut FrameReport<'_>) {
    let bytes = frame.bytes;
    let syntax = frame.declared.syntax.as_str();
    if !bytes.starts_with(&[0xFF, SOI]) {
        report.violation("PS3.10 7.2 and PS3.5 A.4.3", || {
            format!(
                "begins with {}, not the SOI marker of the JPEG-LS stream Transfer Syntax {syntax} identifies",
                signature(bytes)
            )
        });
        return;
    }
    let mut sof = None;
    let mut scan = None;
    // Mapping tables defined by LSE ID 2 (ISO/IEC 14495-1 C.2.4).
    let mut mapping_tables = [false; 256];
    for segment in header_segments(bytes) {
        let segment = match segment {
            Ok(segment) => segment,
            Err(reason) => {
                report.violation(INTERCHANGE, || {
                    format!("JPEG-LS header is malformed: {reason}")
                });
                return;
            }
        };
        match segment.marker {
            SOI | EOI => {
                report.violation(INTERCHANGE, || {
                    "reaches SOI or EOI before a scan, so the Frame is not one JPEG-LS interchange format stream".into()
                });
                return;
            }
            START_OF_FRAME_55 => match FrameHeader::parse(START_OF_FRAME_55, segment.payload) {
                Ok(header) => sof = Some(header),
                Err(reason) => {
                    report.violation(INTERCHANGE, || {
                        format!("JPEG-LS header is malformed: {reason}")
                    });
                    return;
                }
            },
            marker if is_start_of_frame(marker) => {
                let name = super::markers::start_of_frame_name(marker);
                report.violation("PS3.10 7.2 and PS3.5 A.4.3", || {
                    format!("{name} is an ISO/IEC 10918-1 frame, not the JPEG-LS SOF55 of Transfer Syntax {syntax}")
                });
                return;
            }
            PRESET_PARAMETERS => {
                if let [2, table, ..] = segment.payload {
                    mapping_tables[usize::from(*table)] = true;
                }
            }
            SOS => match ScanHeader::parse(segment.payload) {
                Ok(header) => scan = Some(header),
                Err(reason) => {
                    report.violation(INTERCHANGE, || {
                        format!("JPEG-LS header is malformed: {reason}")
                    });
                    return;
                }
            },
            _ => {}
        }
    }
    let Some(sof) = sof else {
        report.violation(INTERCHANGE, || {
            "has no SOF55 frame header before its first scan, so it is not in the JPEG-LS interchange format".into()
        });
        return;
    };
    check_frame_header(frame, &sof, report);
    if let Some(scan) = scan {
        check_scan(&scan, &mapping_tables, near_lossless, syntax, report);
    }
    if !ends_with_end_of_image(bytes) {
        report.violation(INTERCHANGE, || {
            "does not end with EOI (FFD9H), so the Frame is not one complete, separately encoded JPEG-LS stream".into()
        });
    }
    if let Some(fragment) = second_stream_fragment(bytes, frame.fragment_starts) {
        report.violation(INTERCHANGE, || {
            format!("its Fragment {fragment} begins another JPEG-LS stream after the previous Fragment ended one, so its Fragments hold data of more than one Frame")
        });
    }
}

fn check_frame_header(frame: &Frame<'_>, sof: &FrameHeader, report: &mut FrameReport<'_>) {
    let declared = frame.declared;
    declared.bits_stored.compare(
        report,
        CONSISTENT,
        "SOF55 sample precision",
        u32::from(sof.precision),
    );
    // Zero defers a dimension to an LSE oversize-dimension segment, which
    // only dimensions beyond the range of Rows and Columns need.
    if sof.lines != 0 {
        declared.rows.compare(
            report,
            CONSISTENT,
            "SOF55 number of lines",
            u32::from(sof.lines),
        );
    }
    if sof.samples_per_line != 0 {
        declared.columns.compare(
            report,
            CONSISTENT,
            "SOF55 samples per line",
            u32::from(sof.samples_per_line),
        );
    }
    declared.samples_per_pixel.compare(
        report,
        CONSISTENT,
        "SOF55 number of components",
        u32::try_from(sof.components.len()).unwrap_or(u32::MAX),
    );
    if sof.subsamples() {
        report.violation("PS3.3 C.7.6.3.1.1", || {
            format!(
                "SOF55 sampling factors {} encode planes of different sizes, which Photometric Interpretation {} does not describe",
                sof.sampling_text(),
                declared.photometric()
            )
        });
    }
}

/// NEAR selects lossless (0) or near-lossless coding, and a nonzero mapping
/// table selector needs the table in the stream. Only the first scan is
/// checked: later scans follow entropy-coded data, which is not scanned.
fn check_scan(
    scan: &ScanHeader,
    mapping_tables: &[bool; 256],
    near_lossless: bool,
    syntax: &str,
    report: &mut FrameReport<'_>,
) {
    let near = scan.parameters[0];
    if near != 0 && !near_lossless {
        report.violation("PS3.5 A.4.3", || {
            format!(
                "SOS NEAR {near} selects near-lossless coding, but Transfer Syntax {syntax} identifies the lossless mode (NEAR 0)"
            )
        });
    }
    for &(component, table) in &scan.components {
        if table != 0 && !mapping_tables[usize::from(table)] {
            report.violation(INTERCHANGE, || {
                format!(
                    "SOS component {component} uses mapping table {table}, which no LSE segment specifies: abbreviated format instead of the interchange format"
                )
            });
        }
    }
}
