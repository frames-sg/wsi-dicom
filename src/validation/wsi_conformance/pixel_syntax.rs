//! The Photometric Interpretation and sample depths a VL Whole Slide
//! Microscopy image may declare with its Transfer Syntax. PS3.3 C.8.12.4.1.5
//! requires a Value "appropriate to the compression Transfer Syntax", and the
//! PS3.5 8.2 table of each syntax admits no other Standard Photometric
//! Interpretation, so the permitted Values are the intersection of the two.

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

/// Rows and Columns a motion syntax admits.
#[derive(Clone, Copy)]
enum FrameSize {
    Any,
    AtMost { rows: u64, columns: u64 },
    OneOf(&'static [(u64, u64)]),
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
            frame: FrameSize::AtMost {
                rows: 576,
                columns: 720,
            },
            square_pixels: false,
            stereo: None,
        },
        uids::MPEG2MPHL | uids::MPEG2MPHLF => Motion {
            section: "8.2.6",
            photometric: &["YBR_PARTIAL_420", "MONOCHROME2"],
            bits: [8, 8, 7],
            frame: FrameSize::OneOf(HD_FRAMES),
            square_pixels: true,
            stereo: None,
        },
        uids::MPEG4HP41 | uids::MPEG4HP41F => h264("8.2.7", FrameSize::Any, None),
        uids::MPEG4HP41BD | uids::MPEG4HP41BDF => {
            h264("8.2.7 Table 8-4", FrameSize::OneOf(HD_FRAMES), None)
        }
        uids::MPEG4HP422D | uids::MPEG4HP422DF => {
            h264("8.2.8 Table 8-8", FrameSize::Any, Some(false))
        }
        uids::MPEG4HP423D | uids::MPEG4HP423DF => {
            h264("8.2.8 Table 8-8", FrameSize::Any, Some(true))
        }
        uids::MPEG4HP42STEREO | uids::MPEG4HP42STEREOF => h264("8.2.9", FrameSize::Any, Some(true)),
        uids::HEVCMP51 => h264("8.2.10", FrameSize::Any, None),
        uids::HEVCM10P51 => Motion {
            bits: [16, 10, 9],
            ..h264("8.2.11", FrameSize::Any, None)
        },
        _ => return None,
    })
}

/// A motion syntax cannot encode a VL WSI image: PS3.3 C.8.12.4.1.5 does not
/// permit their YBR_PARTIAL_420, and their other Values contradict the VL WSI
/// Enumerated Values. The departures from the syntax's own PS3.5 attribute
/// rules are listed too.
pub(super) fn reject_motion_syntax(object: &DefaultDicomObject, uid: &str) -> Result<(), String> {
    let Some(motion) = motion(uid) else {
        return Ok(());
    };
    let integer = |tag| {
        object
            .get(tag)
            .and_then(|element| element.to_int::<u64>().ok())
    };
    let shown = |value: Option<u64>| value.map_or_else(|| "absent".to_string(), |v| v.to_string());
    let mut departures = Vec::new();
    let samples = integer(tags::SAMPLES_PER_PIXEL);
    if samples != Some(3) {
        departures.push(format!("Samples per Pixel {} (shall be 3)", shown(samples)));
    }
    let photometric = optional_string(object, tags::PHOTOMETRIC_INTERPRETATION)
        .ok()
        .flatten()
        .unwrap_or_default();
    if !motion.photometric.contains(&photometric.as_str()) {
        departures.push(format!(
            "Photometric Interpretation {photometric:?} (shall be {})",
            motion.photometric.join(" or ")
        ));
    }
    for (tag, name, expected) in [
        (tags::BITS_ALLOCATED, "Bits Allocated", motion.bits[0]),
        (tags::BITS_STORED, "Bits Stored", motion.bits[1]),
        (tags::HIGH_BIT, "High Bit", motion.bits[2]),
        (tags::PIXEL_REPRESENTATION, "Pixel Representation", 0),
        (tags::PLANAR_CONFIGURATION, "Planar Configuration", 0),
    ] {
        let value = integer(tag);
        if value != Some(expected) {
            departures.push(format!("{name} {} (shall be {expected})", shown(value)));
        }
    }
    let rows = integer(tags::ROWS).unwrap_or(0);
    let columns = integer(tags::COLUMNS).unwrap_or(0);
    match motion.frame {
        FrameSize::Any => {}
        FrameSize::AtMost {
            rows: max_rows,
            columns: max_columns,
        } if rows > max_rows || columns > max_columns => departures.push(format!(
            "Rows {rows} and Columns {columns} (at most {max_rows} and {max_columns})"
        )),
        FrameSize::AtMost { .. } => {}
        FrameSize::OneOf(sizes) if !sizes.contains(&(rows, columns)) => departures.push(format!(
            "Rows {rows} and Columns {columns} (shall be {})",
            sizes
                .iter()
                .map(|(rows, columns)| format!("{rows} and {columns}"))
                .collect::<Vec<_>>()
                .join(", or ")
        )),
        FrameSize::OneOf(_) => {}
    }
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
    let departures = if departures.is_empty() {
        String::new()
    } else {
        format!(
            "; the image also departs from PS3.5 {EDITION} {}: {}",
            motion.section,
            departures.join(", ")
        )
    };
    Err(format!(
        "Transfer Syntax {uid} is a motion compression syntax, which VL Whole Slide Microscopy images shall not use (PS3.3 {EDITION} C.8.12.4.1.5){departures}"
    ))
}
