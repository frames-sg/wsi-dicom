//! JPEG (ISO/IEC 10918-1) interchange format streams of the JPEG Transfer
//! Syntaxes (PS3.5 8.2.1 and A.4.1).

use super::markers::{
    ends_with_end_of_image, header_segments, is_start_of_frame, second_stream_fragment,
    start_of_frame_name, FrameHeader, ScanHeader, EOI, SOI, SOS,
};
use super::{icc, signature, Frame, FrameReport};

const INTERCHANGE: &str = "PS3.5 A.4.1";
const CONSISTENT: &str = "PS3.5 8.2.1";
const PLANES: &str = "PS3.3 C.7.6.3.1.1";
const COLOR: &str = "PS3.5 8.2.1 and PS3.3 C.8.12.4.1.5";

/// The JPEG coding process a Transfer Syntax identifies (PS3.5 Table A.4-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Process {
    /// Process 1, 1.2.840.10008.1.2.4.50.
    Baseline,
    /// Processes 2 (8-bit) and 4 (12-bit), 1.2.840.10008.1.2.4.51.
    Extended,
    /// Process 14, 1.2.840.10008.1.2.4.57.
    Lossless,
    /// Process 14 with Selection Value 1, 1.2.840.10008.1.2.4.70.
    LosslessSv1,
}

impl Process {
    fn start_of_frame(self) -> u8 {
        match self {
            Self::Baseline => 0xC0,
            Self::Extended => 0xC1,
            Self::Lossless | Self::LosslessSv1 => 0xC3,
        }
    }

    fn is_lossless(self) -> bool {
        matches!(self, Self::Lossless | Self::LosslessSv1)
    }

    fn admits_precision(self, precision: u8) -> bool {
        match self {
            Self::Baseline => precision == 8,
            Self::Extended => matches!(precision, 8 | 12),
            Self::Lossless | Self::LosslessSv1 => (2..=16).contains(&precision),
        }
    }

    fn precisions(self) -> &'static str {
        match self {
            Self::Baseline => "8 (Process 1)",
            Self::Extended => "8 (Process 2) or 12 (Process 4)",
            Self::Lossless | Self::LosslessSv1 => "2 to 16 (Process 14)",
        }
    }
}

/// Tables and color declarations met before the first scan.
#[derive(Default)]
struct Header<'a> {
    frame: Option<FrameHeader>,
    scan: Option<ScanHeader>,
    quantization: [bool; 4],
    dc: [bool; 4],
    ac: [bool; 4],
    jfif: bool,
    adobe_transform: Option<u8>,
    icc_chunks: Vec<icc::Chunk<'a>>,
}

pub(super) fn check(frame: &Frame<'_>, process: Process, report: &mut FrameReport<'_>) {
    let bytes = frame.bytes;
    let syntax = frame.declared.syntax.as_str();
    if !bytes.starts_with(&[0xFF, SOI]) {
        report.violation("PS3.10 7.2 and PS3.5 A.4.1", || {
            format!(
                "begins with {}, not the SOI marker of the JPEG stream Transfer Syntax {syntax} identifies",
                signature(bytes)
            )
        });
        return;
    }
    let Some(header) = read_header(bytes, report) else {
        return;
    };
    let Some(sof) = &header.frame else {
        report.violation(INTERCHANGE, || {
            "has no SOF marker segment before its first scan, so it is not in the JPEG interchange format".into()
        });
        return;
    };
    check_process(sof, process, syntax, report);
    check_frame_header(frame, sof, report);
    if let Some(scan) = &header.scan {
        check_tables(&header, sof, scan, report);
        check_predictor(scan, process, syntax, report);
    }
    check_color(frame, &header, sof, report);
    if let Some(profile) = &frame.profile {
        icc::compare(&header.icc_chunks, profile, report);
    }
    if !ends_with_end_of_image(bytes) {
        report.violation(INTERCHANGE, || {
            "does not end with EOI (FFD9H), so the Frame is not one complete, separately encoded JPEG stream".into()
        });
    }
    if let Some(fragment) = second_stream_fragment(bytes, frame.fragment_starts) {
        report.violation(INTERCHANGE, || {
            format!("its Fragment {fragment} begins another JPEG stream after the previous Fragment ended one, so its Fragments hold data of more than one Frame")
        });
    }
}

fn read_header<'a>(bytes: &'a [u8], report: &mut FrameReport<'_>) -> Option<Header<'a>> {
    let mut header = Header::default();
    for segment in header_segments(bytes) {
        let segment = match segment {
            Ok(segment) => segment,
            Err(reason) => {
                report.violation(INTERCHANGE, || {
                    format!("JPEG header is malformed: {reason}")
                });
                return None;
            }
        };
        let payload = segment.payload;
        match segment.marker {
            SOI => {
                report.violation(INTERCHANGE, || {
                    "a second SOI precedes the first scan: the Frame holds more than one JPEG stream".into()
                });
                return None;
            }
            EOI => {
                report.violation(INTERCHANGE, || {
                    "reaches EOI without a scan: an abbreviated table-specification stream, not a Frame in the interchange format".into()
                });
                return None;
            }
            0xDB => {
                if let Err(reason) = define_quantization_tables(payload, &mut header.quantization) {
                    report.violation(INTERCHANGE, || format!("DQT is malformed: {reason}"));
                    return None;
                }
            }
            0xC4 => {
                if let Err(reason) = define_huffman_tables(payload, &mut header.dc, &mut header.ac)
                {
                    report.violation(INTERCHANGE, || format!("DHT is malformed: {reason}"));
                    return None;
                }
            }
            0xE0 if payload.starts_with(b"JFIF\0") => header.jfif = true,
            0xEE if payload.len() >= 12 && payload.starts_with(b"Adobe") => {
                header.adobe_transform = Some(payload[11]);
            }
            0xE2 => header.icc_chunks.extend(icc::Chunk::parse(payload)),
            0xDE => {
                report.violation(INTERCHANGE, || {
                    "DHP declares the hierarchical mode, which no JPEG Transfer Syntax of Table A.4-3 identifies".into()
                });
                return None;
            }
            marker if is_start_of_frame(marker) => {
                if header.frame.is_some() {
                    report.violation(INTERCHANGE, || {
                        format!(
                            "{} follows another SOF before the first scan",
                            start_of_frame_name(marker)
                        )
                    });
                    return None;
                }
                match FrameHeader::parse(marker, payload) {
                    Ok(sof) => header.frame = Some(sof),
                    Err(reason) => {
                        report.violation(INTERCHANGE, || {
                            format!("JPEG header is malformed: {reason}")
                        });
                        return None;
                    }
                }
            }
            SOS => match ScanHeader::parse(payload) {
                Ok(scan) => header.scan = Some(scan),
                Err(reason) => {
                    report.violation(INTERCHANGE, || {
                        format!("JPEG header is malformed: {reason}")
                    });
                    return None;
                }
            },
            _ => {}
        }
    }
    Some(header)
}

fn define_quantization_tables(payload: &[u8], defined: &mut [bool; 4]) -> Result<(), String> {
    let mut rest = payload;
    while let [parameters, tail @ ..] = rest {
        let (precision, table) = (parameters >> 4, usize::from(parameters & 0x0F));
        let size = if precision == 0 { 64 } else { 128 };
        if table > 3 || precision > 1 || tail.len() < size {
            return Err(format!(
                "table {table} with precision {precision} does not fit"
            ));
        }
        defined[table] = true;
        rest = &tail[size..];
    }
    Ok(())
}

fn define_huffman_tables(
    payload: &[u8],
    dc: &mut [bool; 4],
    ac: &mut [bool; 4],
) -> Result<(), String> {
    let mut rest = payload;
    while let [parameters, tail @ ..] = rest {
        let (class, table) = (parameters >> 4, usize::from(parameters & 0x0F));
        let Some(counts) = tail.get(..16) else {
            return Err("code length counts are truncated".into());
        };
        let values = counts
            .iter()
            .map(|count| usize::from(*count))
            .sum::<usize>();
        if table > 3 || class > 1 || tail.len() < 16 + values {
            return Err(format!("table class {class} id {table} does not fit"));
        }
        if class == 0 {
            dc[table] = true;
        } else {
            ac[table] = true;
        }
        rest = &tail[16 + values..];
    }
    Ok(())
}

fn check_process(sof: &FrameHeader, process: Process, syntax: &str, report: &mut FrameReport<'_>) {
    if sof.marker != process.start_of_frame() {
        report.violation("PS3.5 A.4.1 Table A.4-3", || {
            format!(
                "{} is not the JPEG coding process that Transfer Syntax {syntax} identifies ({})",
                sof.name(),
                start_of_frame_name(process.start_of_frame())
            )
        });
    }
    if !process.admits_precision(sof.precision) {
        report.violation("PS3.5 A.4.1 Table A.4-3", || {
            format!(
                "{} sample precision {} is not the {} of Transfer Syntax {syntax}",
                sof.name(),
                sof.precision,
                process.precisions()
            )
        });
    }
}

fn check_frame_header(frame: &Frame<'_>, sof: &FrameHeader, report: &mut FrameReport<'_>) {
    let declared = frame.declared;
    let name = sof.marker_name();
    declared.bits_stored.compare(
        report,
        CONSISTENT,
        &format!("{name} sample precision"),
        u32::from(sof.precision),
    );
    // Zero lines defers the height to a DNL marker after the first scan.
    if sof.lines != 0 {
        declared.rows.compare(
            report,
            CONSISTENT,
            &format!("{name} number of lines"),
            u32::from(sof.lines),
        );
    }
    declared.columns.compare(
        report,
        CONSISTENT,
        &format!("{name} samples per line"),
        u32::from(sof.samples_per_line),
    );
    let components = u32::try_from(sof.components.len()).unwrap_or(u32::MAX);
    declared.samples_per_pixel.compare(
        report,
        CONSISTENT,
        &format!("{name} number of components"),
        components,
    );
    // Only YBR_FULL_422 describes chrominance planes smaller than the
    // luminance plane (PS3.3 C.7.6.3.1.1 Note).
    if sof.subsamples() && !declared.photometric_is(&["YBR_FULL_422"]) {
        report.violation(PLANES, || {
            format!(
                "{name} sampling factors {} encode planes of different sizes, which Photometric Interpretation {} does not describe",
                sof.sampling_text(),
                declared.photometric()
            )
        });
    }
}

/// The interchange format carries every table the scan uses (PS3.5 A.4.1).
/// Only the first scan's tables are checked: later scans follow entropy-coded
/// data, which is not scanned.
fn check_tables(
    header: &Header<'_>,
    sof: &FrameHeader,
    scan: &ScanHeader,
    report: &mut FrameReport<'_>,
) {
    let dct = sof.marker != 0xC3;
    for &(selector, tables) in &scan.components {
        let Some(&(_, _, quantization)) = sof
            .components
            .iter()
            .find(|component| component.0 == selector)
        else {
            report.violation(INTERCHANGE, || {
                format!(
                    "SOS selects component {selector}, which {} does not declare",
                    sof.name()
                )
            });
            continue;
        };
        let (dc, ac) = (usize::from(tables >> 4), usize::from(tables & 0x0F));
        let mut missing = Vec::new();
        if dct
            && !header
                .quantization
                .get(usize::from(quantization))
                .copied()
                .unwrap_or(false)
        {
            missing.push(format!("quantization table {quantization} (DQT)"));
        }
        if !header.dc.get(dc).copied().unwrap_or(false) {
            missing.push(format!("DC Huffman table {dc} (DHT)"));
        }
        if dct && !header.ac.get(ac).copied().unwrap_or(false) {
            missing.push(format!("AC Huffman table {ac} (DHT)"));
        }
        if !missing.is_empty() {
            report.violation(INTERCHANGE, || {
                format!(
                    "component {selector} of the first scan uses {}, which the stream does not specify: abbreviated format instead of the interchange format",
                    missing.join(" and ")
                )
            });
        }
    }
}

/// The lossless predictor is the scan's Ss parameter (ISO/IEC 10918-1 H.1.2.1).
fn check_predictor(
    scan: &ScanHeader,
    process: Process,
    syntax: &str,
    report: &mut FrameReport<'_>,
) {
    if !process.is_lossless() {
        return;
    }
    let predictor = scan.parameters[0];
    let admitted = match process {
        Process::LosslessSv1 => predictor == 1,
        _ => (1..=7).contains(&predictor),
    };
    if !admitted {
        report.violation("PS3.5 A.4.1 Table A.4-3", || {
            format!(
                "SOS predictor selection value {predictor} is not {} of Transfer Syntax {syntax}",
                if process == Process::LosslessSv1 {
                    "the first-order prediction (Selection Value 1)"
                } else {
                    "a lossless predictor 1 to 7"
                }
            )
        });
    }
}

/// An explicit color declaration in the stream must agree with Photometric
/// Interpretation, which describes what is actually encoded. The Adobe APP14
/// transform flag declares it outright; without one, JFIF APP0 implies YCbCr
/// (ISO/IEC 10918-5). Transform 0 only says the encoder applied none, which
/// YBR_FULL_422 source components also allow, so it is not compared. An
/// absent declaration leaves the components to Photometric Interpretation.
fn check_color(
    frame: &Frame<'_>,
    header: &Header<'_>,
    sof: &FrameHeader,
    report: &mut FrameReport<'_>,
) {
    if sof.components.len() != 3 || !frame.declared.photometric_is(&["RGB"]) {
        return;
    }
    let declaration = match header.adobe_transform {
        Some(1) => "APP14 Adobe transform 1 (YCbCr)",
        None if header.jfif => "APP0 JFIF, which implies YCbCr components",
        _ => return,
    };
    report.violation(COLOR, || {
        format!("{declaration} declares color-transformed components but Photometric Interpretation is RGB")
    });
}
