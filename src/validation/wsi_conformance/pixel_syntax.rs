//! The Photometric Interpretation and sample depths an image may declare with
//! its Transfer Syntax. The PS3.5 8.2 table of each syntax admits no other
//! Standard Photometric Interpretation; for a VL Whole Slide Microscopy image
//! PS3.3 C.8.12.4.1.5 also requires a Value "appropriate to the compression
//! Transfer Syntax", so its permitted Values are the intersection of the two.
//! A motion syntax fixes the Pixel Data attributes it encodes (PS3.5 8.2.5 to
//! 8.2.11).

use dicom_dictionary_std::{tags, uids};
use dicom_object::DefaultDicomObject;

use super::fields::optional_string;
use crate::validation::iod::EDITION;

/// What one Transfer Syntax admits in a VL WSI image.
struct Permitted {
    photometric: &'static [&'static str],
    /// Bits Allocated 8, Bits Stored 8, High Bit 7 only.
    eight_bit: bool,
    /// The PS3.5 statement of the syntax's table.
    citation: &'static str,
}

const MONOCHROME_OR_RGB: &[&str] = &["MONOCHROME2", "RGB"];

fn permitted(uid: &str) -> Permitted {
    let (photometric, eight_bit, citation): (&[&str], bool, &str) = match uid {
        uids::JPEG_BASELINE8_BIT => (
            &["MONOCHROME2", "YBR_FULL_422", "RGB"],
            true,
            "8.2.1 Table 8.2.1-1",
        ),
        // Table 8.2.1-1 lists JPEG Extended for monochrome images only.
        uids::JPEG_EXTENDED12_BIT => (&["MONOCHROME2"], true, "8.2.1 Table 8.2.1-1"),
        uids::JPEG_LOSSLESS | uids::JPEG_LOSSLESS_SV1 => {
            (MONOCHROME_OR_RGB, false, "8.2.1 Table 8.2.1-2")
        }
        uids::RLE_LOSSLESS => (MONOCHROME_OR_RGB, false, "8.2.2 Table 8.2.2-1"),
        uids::JPEGLS_LOSSLESS | uids::JPEGLS_NEAR_LOSSLESS => {
            (MONOCHROME_OR_RGB, false, "8.2.3 Table 8.2.3-1")
        }
        uids::JPEG2000_LOSSLESS => (
            &["MONOCHROME2", "YBR_RCT", "RGB"],
            false,
            "8.2.4 Table 8.2.4-1",
        ),
        uids::JPEG2000 => (
            &["MONOCHROME2", "YBR_RCT", "YBR_ICT", "RGB"],
            false,
            "8.2.4 Table 8.2.4-1",
        ),
        // Table 8.2.4-1 lists the Part 1 syntaxes only, but 8.2.4 makes a Part
        // 1 reversible (irreversible) color transformation YBR_RCT (YBR_ICT)
        // in any JPEG 2000 syntax, and C.8.12.4.1.5 admits YBR_RCT or RGB for
        // every reversible one and YBR_ICT or RGB for every irreversible one.
        uids::JPEG2000MC_LOSSLESS => (&["MONOCHROME2", "YBR_RCT", "RGB"], false, "8.2.4"),
        uids::JPEG2000MC => (
            &["MONOCHROME2", "YBR_RCT", "YBR_ICT", "RGB"],
            false,
            "8.2.4",
        ),
        uids::HTJ2K_LOSSLESS | uids::HTJ2K_LOSSLESS_RPCL => (
            &["MONOCHROME2", "YBR_RCT", "RGB"],
            false,
            "8.2.14 Table 8.2.14-1",
        ),
        uids::HTJ2K => (
            &["MONOCHROME2", "YBR_RCT", "YBR_ICT", "RGB"],
            false,
            "8.2.14 Table 8.2.14-1",
        ),
        uids::JPEGXL_LOSSLESS | uids::JPEGXL => (
            &["MONOCHROME2", "YBR_RCT", "RGB"],
            false,
            "8.2.15 Table 8.2.15-1",
        ),
        uids::JPEGXLJPEG_RECOMPRESSION => (
            &["MONOCHROME2", "YBR_FULL_422", "RGB"],
            true,
            "8.2.15 Table 8.2.15-1",
        ),
        uids::JPIP_REFERENCED | uids::JPIP_REFERENCED_DEFLATE => {
            (&["MONOCHROME2", "YBR_ICT", "YBR_RCT"], false, "A.6")
        }
        uids::JPIPHTJ2K_REFERENCED | uids::JPIPHTJ2K_REFERENCED_DEFLATE => {
            (&["MONOCHROME2", "YBR_ICT", "YBR_RCT"], false, "A.11")
        }
        // Native, Encapsulated Uncompressed and Deflated Image Frame syntaxes
        // have no color transformation (PS3.5 8.2, 8.2.16).
        _ => (MONOCHROME_OR_RGB, false, "8.2"),
    };
    Permitted {
        photometric,
        eight_bit,
        citation,
    }
}

/// The Standard Photometric Interpretations PS3.5 admits with `uid`, and the
/// table or section stating them; `None` for a syntax that restricts none
/// (Encapsulated Uncompressed, Deflated Image Frame, a private syntax) or
/// whose own attribute rules hold it (a motion syntax).
pub(super) fn standard_photometric(uid: &str) -> Option<(&'static [&'static str], &'static str)> {
    const LOSSLESS: &[&str] = &[
        "MONOCHROME1",
        "MONOCHROME2",
        "PALETTE COLOR",
        "YBR_FULL",
        "RGB",
    ];
    const J2K_LOSSLESS: &[&str] = &[
        "MONOCHROME1",
        "MONOCHROME2",
        "PALETTE COLOR",
        "YBR_RCT",
        "RGB",
        "YBR_FULL",
    ];
    const J2K: &[&str] = &[
        "MONOCHROME1",
        "MONOCHROME2",
        "YBR_RCT",
        "YBR_ICT",
        "RGB",
        "YBR_FULL",
    ];
    Some(match uid {
        uids::JPEG_BASELINE8_BIT => (
            &["MONOCHROME1", "MONOCHROME2", "YBR_FULL_422", "RGB"],
            "8.2.1 Table 8.2.1-1",
        ),
        uids::JPEG_EXTENDED12_BIT => (&["MONOCHROME1", "MONOCHROME2"], "8.2.1 Table 8.2.1-1"),
        uids::JPEG_LOSSLESS | uids::JPEG_LOSSLESS_SV1 => (LOSSLESS, "8.2.1 Table 8.2.1-2"),
        uids::RLE_LOSSLESS => (LOSSLESS, "8.2.2 Table 8.2.2-1"),
        uids::JPEGLS_LOSSLESS => (LOSSLESS, "8.2.3 Table 8.2.3-1"),
        uids::JPEGLS_NEAR_LOSSLESS => (
            &["MONOCHROME1", "MONOCHROME2", "YBR_FULL", "RGB"],
            "8.2.3 Table 8.2.3-1",
        ),
        uids::JPEG2000_LOSSLESS => (J2K_LOSSLESS, "8.2.4 Table 8.2.4-1"),
        uids::JPEG2000 => (J2K, "8.2.4 Table 8.2.4-1"),
        // Table 8.2.4-1 lists the Part 1 syntaxes only; 8.2.4 gives the Part 2
        // ones the same color transformations.
        uids::JPEG2000MC_LOSSLESS => (J2K_LOSSLESS, "8.2.4"),
        uids::JPEG2000MC => (J2K, "8.2.4"),
        uids::HTJ2K_LOSSLESS | uids::HTJ2K_LOSSLESS_RPCL => (J2K_LOSSLESS, "8.2.14 Table 8.2.14-1"),
        uids::HTJ2K => (J2K, "8.2.14 Table 8.2.14-1"),
        uids::JPEGXL_LOSSLESS | uids::JPEGXL => (
            &["MONOCHROME1", "MONOCHROME2", "XYB", "YBR_RCT", "RGB"],
            "8.2.15 Table 8.2.15-1",
        ),
        uids::JPEGXLJPEG_RECOMPRESSION => (
            &["MONOCHROME2", "YBR_FULL_422", "XYB", "RGB"],
            "8.2.15 Table 8.2.15-1",
        ),
        uids::JPIP_REFERENCED | uids::JPIP_REFERENCED_DEFLATE => {
            (&["MONOCHROME1", "MONOCHROME2", "YBR_ICT", "YBR_RCT"], "A.6")
        }
        uids::JPIPHTJ2K_REFERENCED | uids::JPIPHTJ2K_REFERENCED_DEFLATE => (
            &["MONOCHROME1", "MONOCHROME2", "YBR_ICT", "YBR_RCT"],
            "A.11",
        ),
        _ => return None,
    })
}

/// The Standard Transfer Syntaxes of Native Format Pixel Data (PS3.5 A.1, A.2,
/// A.3 and A.5).
pub(super) fn is_native(uid: &str) -> bool {
    [
        uids::IMPLICIT_VR_LITTLE_ENDIAN,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
        // Explicit VR Big Endian, retired but still a Native syntax (A.3).
        "1.2.840.10008.1.2.2",
        uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN,
    ]
    .contains(&uid)
}

/// Photometric Interpretations not encodable in Native Format (PS3.5 8.2).
pub(super) const NOT_NATIVE: [&str; 3] = ["YBR_RCT", "YBR_ICT", "YBR_PARTIAL_420"];

/// The JPEG-LS, JPEG 2000, HTJ2K and JPEG XL syntaxes, whose codestream fixes
/// how components interleave, so Planar Configuration shall be 0 (PS3.5
/// 8.2.3, 8.2.4, 8.2.14 and 8.2.15).
pub(super) fn planar_configuration_section(uid: &str) -> Option<&'static str> {
    Some(match uid {
        uids::JPEGLS_LOSSLESS | uids::JPEGLS_NEAR_LOSSLESS => "8.2.3",
        uids::JPEG2000_LOSSLESS | uids::JPEG2000 | uids::JPEG2000MC_LOSSLESS | uids::JPEG2000MC => {
            "8.2.4"
        }
        uids::HTJ2K_LOSSLESS | uids::HTJ2K_LOSSLESS_RPCL | uids::HTJ2K => "8.2.14",
        uids::JPEGXL_LOSSLESS | uids::JPEGXLJPEG_RECOMPRESSION | uids::JPEGXL => "8.2.15",
        _ => return None,
    })
}

/// PS3.3 C.8.12.4.1.5 with the PS3.5 8.2 table of `uid`; `allocated` is
/// Bits Allocated, already equal to Bits Stored with High Bit one less.
pub(super) fn validate_photometric_for_syntax(
    uid: &str,
    photometric: &str,
    allocated: u64,
) -> Result<(), String> {
    let permitted = permitted(uid);
    if !permitted.photometric.contains(&photometric) {
        return Err(format!(
            "Photometric Interpretation {photometric} is not permitted with Transfer Syntax {uid}; a VL WSI image in this syntax uses {} (PS3.3 {EDITION} C.8.12.4.1.5, PS3.5 {EDITION} {})",
            permitted.photometric.join(", "),
            permitted.citation
        ));
    }
    if permitted.eight_bit && allocated != 8 {
        let reason = if uid == uids::JPEG_EXTENDED12_BIT {
            ", and its 16-bit form (Bits Stored 12, High Bit 11) is excluded by VL WSI Bits Stored equal to Bits Allocated"
        } else {
            ""
        };
        return Err(format!(
            "Transfer Syntax {uid} admits Bits Allocated 8, Bits Stored 8, High Bit 7 for {photometric}, found Bits Allocated {allocated}{reason} (PS3.5 {EDITION} {})",
            permitted.citation
        ));
    }
    Ok(())
}

/// Rows and Columns a motion syntax admits, with the frame rates.
#[derive(Clone, Copy)]
enum FrameSize {
    /// MPEG2 Main Level, Table 8-1: at most 720 Columns, and at most 480
    /// Rows at 30 (29.97) frames per second or 576 at 25.
    MainLevel,
    /// Rows and Columns from this list, and one of these frame rates.
    OneOf(&'static [(u64, u64)], &'static [f64]),
    /// Rows, Columns and frame rates from this list (Table 8-4).
    Formats(&'static [(u64, u64, f64)]),
    /// Within the limits of a video coding level.
    Level(Level),
}

/// The frame size and throughput limits of an H.264 level (ITU-T H.264
/// Table A-1, in 16x16 macroblocks) or an HEVC level (ITU-T H.265 Table A.8
/// and A.9, in luma samples).
#[derive(Clone, Copy)]
struct Level {
    name: &'static str,
    /// Largest frame, in units of `unit` luma samples.
    max_frame: u64,
    /// Largest throughput, in the same units per second.
    max_rate: u64,
    /// Samples per unit side: 16 for macroblocks, 1 for luma samples.
    unit: u64,
}

/// The PS3.5 8.2.5 to 8.2.11 Pixel Data attributes of a motion syntax.
struct Motion {
    section: &'static str,
    photometric: &'static [&'static str],
    /// Bits Allocated, Bits Stored, High Bit.
    bits: [u64; 3],
    frame: FrameSize,
    /// "Pixel Aspect Ratio (0028,0034) shall be absent."
    square_pixels: bool,
    /// Stereo Pairs Present: `Some(true)` YES, `Some(false)` NO or absent.
    stereo: Option<bool>,
}

const HD_FRAMES: &[(u64, u64)] = &[(720, 1280), (1080, 1920)];
/// Table 8-2: 25, 30 (or 30/1.001), 50 and 60 (or 60/1.001) frames per
/// second.
const HD_RATES: &[f64] = &[25.0, 30.0, 30.0 / 1.001, 50.0, 60.0, 60.0 / 1.001];
/// Table 8-4.
const BD_FORMATS: &[(u64, u64, f64)] = &[
    (1080, 1920, 25.0),
    (1080, 1920, 30.0 / 1.001),
    (1080, 1920, 24.0),
    (1080, 1920, 24.0 / 1.001),
    (720, 1280, 50.0),
    (720, 1280, 60.0 / 1.001),
    (720, 1280, 24.0),
    (720, 1280, 24.0 / 1.001),
];
const H264_LEVEL_4_1: Level = Level {
    name: "MPEG-4 AVC/H.264 Level 4.1 (ITU-T H.264 Table A-1: MaxFS 8192 and MaxMBPS 245760 macroblocks)",
    max_frame: 8192,
    max_rate: 245_760,
    unit: 16,
};
const H264_LEVEL_4_2: Level = Level {
    name: "MPEG-4 AVC/H.264 Level 4.2 (ITU-T H.264 Table A-1: MaxFS 8704 and MaxMBPS 522240 macroblocks)",
    max_frame: 8704,
    max_rate: 522_240,
    unit: 16,
};
const HEVC_LEVEL_5_1: Level = Level {
    name: "HEVC/H.265 Level 5.1 (ITU-T H.265 Tables A.8 and A.9: MaxLumaPs 8912896 and MaxLumaSr 534773760 samples)",
    max_frame: 8_912_896,
    max_rate: 534_773_760,
    unit: 1,
};

fn motion(uid: &str) -> Option<Motion> {
    let h264 = |section, frame, stereo| Motion {
        section,
        photometric: &["YBR_PARTIAL_420"],
        bits: [8, 8, 7],
        frame,
        square_pixels: true,
        stereo,
    };
    Some(match uid {
        // 8.2.5 lists YBR_PARTIAL_420 and, for single component data,
        // MONOCHROME2; Table 8-1 bounds Rows and Columns.
        uids::MPEG2MPML | uids::MPEG2MPMLF => Motion {
            section: "8.2.5",
            photometric: &["YBR_PARTIAL_420", "MONOCHROME2"],
            bits: [8, 8, 7],
            frame: FrameSize::MainLevel,
            square_pixels: false,
            stereo: None,
        },
        uids::MPEG2MPHL | uids::MPEG2MPHLF => Motion {
            section: "8.2.6",
            photometric: &["YBR_PARTIAL_420", "MONOCHROME2"],
            bits: [8, 8, 7],
            frame: FrameSize::OneOf(HD_FRAMES, HD_RATES),
            square_pixels: true,
            stereo: None,
        },
        uids::MPEG4HP41 | uids::MPEG4HP41F => h264("8.2.7", FrameSize::Level(H264_LEVEL_4_1), None),
        uids::MPEG4HP41BD | uids::MPEG4HP41BDF => {
            h264("8.2.7 Table 8-4", FrameSize::Formats(BD_FORMATS), None)
        }
        uids::MPEG4HP422D | uids::MPEG4HP422DF => h264(
            "8.2.8 Table 8-8",
            FrameSize::Level(H264_LEVEL_4_2),
            Some(false),
        ),
        uids::MPEG4HP423D | uids::MPEG4HP423DF => h264(
            "8.2.8 Table 8-8",
            FrameSize::Level(H264_LEVEL_4_2),
            Some(true),
        ),
        uids::MPEG4HP42STEREO | uids::MPEG4HP42STEREOF => {
            h264("8.2.9", FrameSize::Level(H264_LEVEL_4_2), Some(true))
        }
        uids::HEVCMP51 => h264("8.2.10", FrameSize::Level(HEVC_LEVEL_5_1), None),
        uids::HEVCM10P51 => Motion {
            bits: [16, 10, 9],
            ..h264("8.2.11", FrameSize::Level(HEVC_LEVEL_5_1), None)
        },
        _ => return None,
    })
}

/// The frame rates an object declares, in frames per second: its Cine Rate
/// (0018,0040), and those its Frame Time (0018,1063) or Frame Time Vector
/// (0018,1065) give, in ms.
fn frame_rates(object: &DefaultDicomObject) -> Vec<f64> {
    let numbers = |tag| {
        object
            .get(tag)
            .and_then(|element| element.to_multi_float64().ok())
            .unwrap_or_default()
    };
    let mut rates: Vec<f64> = numbers(tags::CINE_RATE)
        .into_iter()
        .filter(|rate| *rate > 0.0)
        .collect();
    rates.extend(
        numbers(tags::FRAME_TIME)
            .into_iter()
            .chain(numbers(tags::FRAME_TIME_VECTOR).into_iter().skip(1))
            .filter(|time| time.is_finite() && *time > 0.0)
            .map(|time| 1000.0 / time),
    );
    rates
}

/// Whether two frame rates are the same, to the 0.2% that Cine Rate's
/// integer Value and a Frame Time in ms to two decimals allow.
fn same_rate(left: f64, right: f64) -> bool {
    (left - right).abs() <= right * 0.002
}

/// How `rows` by `columns` at `rates` departs from `frame`.
fn frame_departures(frame: FrameSize, rows: u64, columns: u64, rates: &[f64]) -> Vec<String> {
    let mut departures = Vec::new();
    match frame {
        FrameSize::MainLevel => {
            if columns > 720 {
                departures.push(format!("Columns {columns} (at most 720, Table 8-1)"));
            }
            for rate in rates {
                let limit = if same_rate(*rate, 25.0) {
                    576
                } else if same_rate(*rate, 30.0) || same_rate(*rate, 30.0 / 1.001) {
                    480
                } else {
                    departures.push(format!(
                        "frame rate {rate:.3} per second (25 or 30, Table 8-1)"
                    ));
                    continue;
                };
                if rows > limit {
                    departures.push(format!(
                        "Rows {rows} at {rate:.3} frames per second (at most {limit}, Table 8-1)"
                    ));
                }
            }
            if rates.is_empty() && rows > 576 {
                departures.push(format!("Rows {rows} (at most 576, Table 8-1)"));
            }
        }
        FrameSize::OneOf(sizes, allowed) => {
            if !sizes.contains(&(rows, columns)) {
                departures.push(format!(
                    "Rows {rows} and Columns {columns} (shall be {})",
                    sizes
                        .iter()
                        .map(|(rows, columns)| format!("{rows} and {columns}"))
                        .collect::<Vec<_>>()
                        .join(", or ")
                ));
            }
            for rate in rates {
                if !allowed.iter().any(|allowed| same_rate(*rate, *allowed)) {
                    departures.push(format!(
                        "frame rate {rate:.3} per second (25, 30, 50 or 60, or 30/1.001 or 60/1.001, Table 8-2)"
                    ));
                }
            }
        }
        FrameSize::Formats(formats) => {
            if !formats.iter().any(|(format_rows, format_columns, _)| {
                (*format_rows, *format_columns) == (rows, columns)
            }) {
                departures.push(format!(
                    "Rows {rows} and Columns {columns} (1080 and 1920, or 720 and 1280, Table 8-4)"
                ));
            }
            for rate in rates {
                if !formats
                    .iter()
                    .any(|(format_rows, format_columns, allowed)| {
                        (*format_rows, *format_columns) == (rows, columns)
                            && same_rate(*rate, *allowed)
                    })
                {
                    departures.push(format!(
                        "{rows} by {columns} at frame rate {rate:.3} per second (not a format of Table 8-4)"
                    ));
                }
            }
        }
        FrameSize::Level(level) => level_departures(level, rows, columns, rates, &mut departures),
    }
    departures
}

/// A level's frame size, frame side (at most the square root of 8 times the
/// largest frame) and throughput limits.
fn level_departures(
    level: Level,
    rows: u64,
    columns: u64,
    rates: &[f64],
    departures: &mut Vec<String>,
) {
    let width = columns.div_ceil(level.unit);
    let height = rows.div_ceil(level.unit);
    let frame = width.saturating_mul(height);
    let side = ((level.max_frame * 8) as f64).sqrt();
    if frame > level.max_frame || width as f64 > side || height as f64 > side {
        departures.push(format!(
            "Rows {rows} and Columns {columns} (beyond the frame size of {})",
            level.name
        ));
        return;
    }
    for rate in rates {
        if frame as f64 * rate > level.max_rate as f64 * 1.002 {
            departures.push(format!(
                "{rows} by {columns} at frame rate {rate:.3} per second (beyond the throughput of {})",
                level.name
            ));
        }
    }
}

/// How an object's Pixel Data attributes depart from those PS3.5 requires of
/// motion syntax `uid`, and the section requiring them; `None` when `uid` is
/// not a motion syntax.
/// Whether `uid` is one of the video Transfer Syntaxes, MPEG2, MPEG-4
/// AVC/H.264 or HEVC/H.265 (PS3.5 8.2.5 to 8.2.11).
pub(super) fn is_motion_syntax(uid: &str) -> bool {
    motion(uid).is_some()
}

pub(super) fn motion_departures(
    object: &DefaultDicomObject,
    uid: &str,
) -> Option<(&'static str, Vec<String>)> {
    let motion = motion(uid)?;
    let integer = |tag| {
        object
            .get(tag)
            .and_then(|element| element.to_int::<u64>().ok())
    };
    let shown = |value: Option<u64>| value.map_or_else(|| "absent".to_string(), |v| v.to_string());
    let mut departures = Vec::new();
    let photometric = optional_string(object, tags::PHOTOMETRIC_INTERPRETATION)
        .ok()
        .flatten()
        .unwrap_or_default();
    if !motion.photometric.contains(&photometric.as_str()) {
        departures.push(format!(
            "Photometric Interpretation {} (shall be {})",
            crate::validation::disclosure::shown(tags::PHOTOMETRIC_INTERPRETATION, &photometric),
            motion.photometric.join(" or ")
        ));
    }
    // Table 8.2.5-1: single component MPEG2 data is MONOCHROME2, one sample
    // and no Planar Configuration; every other motion syntax is three.
    let single = photometric == "MONOCHROME2" && motion.photometric.contains(&"MONOCHROME2");
    let samples = integer(tags::SAMPLES_PER_PIXEL);
    let expected_samples = if single { 1 } else { 3 };
    if samples != Some(expected_samples) {
        departures.push(format!(
            "Samples per Pixel {} (shall be {expected_samples})",
            shown(samples)
        ));
    }
    for (tag, name, expected) in [
        (tags::BITS_ALLOCATED, "Bits Allocated", motion.bits[0]),
        (tags::BITS_STORED, "Bits Stored", motion.bits[1]),
        (tags::HIGH_BIT, "High Bit", motion.bits[2]),
        (tags::PIXEL_REPRESENTATION, "Pixel Representation", 0),
    ] {
        let value = integer(tag);
        if value != Some(expected) {
            departures.push(format!("{name} {} (shall be {expected})", shown(value)));
        }
    }
    let planar = integer(tags::PLANAR_CONFIGURATION);
    match (single, planar) {
        (true, Some(value)) => departures.push(format!(
            "Planar Configuration {value} (shall be absent for single component data, Table 8.2.5-1)"
        )),
        (false, value) if value != Some(0) => {
            departures.push(format!("Planar Configuration {} (shall be 0)", shown(value)))
        }
        _ => {}
    }
    let rows = integer(tags::ROWS).unwrap_or(0);
    let columns = integer(tags::COLUMNS).unwrap_or(0);
    departures.extend(frame_departures(
        motion.frame,
        rows,
        columns,
        &frame_rates(object),
    ));
    if motion.square_pixels && object.get(tags::PIXEL_ASPECT_RATIO).is_some() {
        departures.push("Pixel Aspect Ratio present (shall be absent)".to_string());
    }
    if let Some(stereo) = motion.stereo {
        let value = optional_string(object, tags::STEREO_PAIRS_PRESENT)
            .ok()
            .flatten();
        let conforms = if stereo {
            value.as_deref() == Some("YES")
        } else {
            value.as_deref().is_none_or(|value| value == "NO")
        };
        if !conforms {
            departures.push(format!(
                "Stereo Pairs Present {} (shall be {})",
                value.as_deref().unwrap_or("absent"),
                if stereo { "YES" } else { "NO or absent" }
            ));
        }
    }
    Some((motion.section, departures))
}

/// A motion syntax cannot encode a VL WSI image: PS3.3 C.8.12.4.1.5 does not
/// permit their YBR_PARTIAL_420, and their other Values contradict the VL WSI
/// Enumerated Values. The departures from the syntax's own PS3.5 attribute
/// rules are listed too.
pub(super) fn reject_motion_syntax(object: &DefaultDicomObject, uid: &str) -> Result<(), String> {
    let Some((section, departures)) = motion_departures(object, uid) else {
        return Ok(());
    };
    let departures = if departures.is_empty() {
        String::new()
    } else {
        format!(
            "; the image also departs from PS3.5 {EDITION} {section}: {}",
            departures.join(", ")
        )
    };
    Err(format!(
        "Transfer Syntax {uid} is a motion compression syntax, which VL Whole Slide Microscopy images shall not use (PS3.3 {EDITION} C.8.12.4.1.5){departures}"
    ))
}
