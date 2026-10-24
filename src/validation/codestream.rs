//! Codestream headers inside encapsulated Pixel Data, checked against the
//! Image Pixel attributes and the Transfer Syntax that identifies their coding
//! process (PS3.5 8.2, A.4 and Annex G).
//!
//! Only headers and markers are parsed and nothing is decoded, so a Frame
//! costs its header (and, for JPEG 2000, one hop per tile-part). RLE and
//! Deflated Image Frames are the exceptions: RLE packets are walked, which
//! validates the encoding without reconstructing the image, and a Deflate bit
//! stream is inflated and counted, which no header could describe.

mod deflate;
mod icc;
mod j2k;
mod jpeg;
mod jpeg_ls;
mod jpeg_xl;
mod markers;
mod rle;
#[cfg(test)]
mod tests;

use std::path::Path;

use dicom_core::value::Value;
use dicom_core::Tag;
use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::iod::{Family, EDITION};
use super::pixel_structure::{dicom_frame_count, optional_u64_values, visit_encapsulated_frames};
use super::wsi_conformance::rule_path;
use super::{intrinsic_check, ValidationCheck, ValidationStatus};

const CODESTREAM_RULE: &str = "intrinsic-wsi-dicom-codestream";

/// The codestream check of one object. A Transfer Syntax that encapsulates no
/// codestream this rule parses (native, Deflated Explicit VR, video, JPIP
/// referenced or private syntaxes) passes with nothing to check; the syntax
/// itself is the pixel-structure and image-metadata checks' to judge.
pub(super) fn run_codestream_check(
    path: &Path,
    object: &DefaultDicomObject,
    max_frame_bytes: usize,
) -> ValidationCheck {
    let syntax = object.meta().transfer_syntax.trim_end_matches('\0');
    let Some(codec) = Codec::of(syntax).map(|codec| codec.limited_to(max_frame_bytes)) else {
        return intrinsic_check(
            CODESTREAM_RULE,
            rule_path(path),
            ValidationStatus::Passed,
            format!(
                "Transfer Syntax {syntax} encapsulates no JPEG, JPEG-LS, JPEG 2000, JPEG XL, RLE or Deflated Image Frame codestream; nothing to check"
            ),
        );
    };
    let mut family = Family::default();
    if let Err(reason) = check_frames(object, syntax, codec, max_frame_bytes, &mut family) {
        family.push(|| reason);
    }
    family.check(path, CODESTREAM_RULE, &codec.citation())
}

/// The codestream check of Frames retrieved apart from their Data Set, such
/// as the parts of a DICOMweb Frames response (PS3.18 10.4.1.1.6), which hold
/// "only the compressed octet stream without the fragment delimiters" (PS3.18
/// 8.7.3.3.2). Each `(Frame Number, bytes)` is held to the Image Pixel
/// attributes of `object` and to `syntax`, the Transfer Syntax its media type
/// names, a Deflated Image Frame inflated up to `max_frame_bytes`. `None` when
/// `syntax` encapsulates no codestream this rule parses.
pub(crate) fn check_retrieved_frames(
    path: &Path,
    object: &InMemDicomObject,
    syntax: &str,
    frames: &[(usize, &[u8])],
    max_frame_bytes: usize,
) -> Option<ValidationCheck> {
    let codec = Codec::of(syntax)?.limited_to(max_frame_bytes);
    let mut family = Family::default();
    let declared = Declared::read(object, syntax);
    let profiles = matches!(codec, Codec::Jpeg(_)).then(|| icc::FrameProfiles::read(object));
    for &(number, bytes) in frames {
        let input = Frame {
            bytes,
            fragment_starts: &[],
            declared: &declared,
            profile: profiles
                .as_ref()
                .and_then(|profiles| profiles.for_frame(number.saturating_sub(1))),
        };
        codec.check(
            &input,
            &mut FrameReport {
                family: &mut family,
                number,
            },
        );
    }
    Some(family.check(path, CODESTREAM_RULE, &codec.citation()))
}

fn check_frames(
    object: &DefaultDicomObject,
    syntax: &str,
    codec: Codec,
    max_frame_bytes: usize,
    family: &mut Family,
) -> Result<(), String> {
    let pixel_data = object.element(tags::PIXEL_DATA).map_err(|_| {
        "Pixel Data (7FE0,0010) is missing, so no codestream was checked".to_string()
    })?;
    let Value::PixelSequence(sequence) = pixel_data.value() else {
        return Err(format!(
            "Pixel Data (7FE0,0010) is not encapsulated although Transfer Syntax {syntax} requires it ({})",
            cite("PS3.5 A.4")
        ));
    };
    let frame_count = dicom_frame_count(object)?;
    let extended_offsets = optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE)?;
    let extended_lengths = optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE_LENGTHS)?;
    let declared = Declared::read(object, syntax);
    let profiles = matches!(codec, Codec::Jpeg(_)).then(|| icc::FrameProfiles::read(object));
    visit_encapsulated_frames(
        sequence,
        frame_count,
        extended_offsets.as_deref(),
        extended_lengths.as_deref(),
        max_frame_bytes,
        |frame| {
            let mut report = FrameReport {
                family: &mut *family,
                number: frame.index + 1,
            };
            let bytes = match frame.bytes() {
                Ok(bytes) => bytes,
                Err(reason) => {
                    report.family.push(|| reason);
                    return;
                }
            };
            let fragment_starts: Vec<usize> = frame
                .fragments
                .iter()
                .scan(0usize, |end, fragment| {
                    *end += fragment.len();
                    Some(*end)
                })
                .take(frame.fragments.len() - 1)
                .filter(|start| *start < bytes.len())
                .collect();
            let input = Frame {
                bytes: &bytes,
                fragment_starts: &fragment_starts,
                declared: &declared,
                profile: profiles
                    .as_ref()
                    .and_then(|profiles| profiles.for_frame(frame.index)),
            };
            codec.check(&input, &mut report);
        },
    )
    .map_err(|reason| {
        format!(
            "Pixel Data Frames cannot be delimited, so no codestream was checked: {reason} ({})",
            cite("PS3.5 A.4")
        )
    })
}

#[derive(Clone, Copy, Debug)]
enum Codec {
    Jpeg(jpeg::Process),
    JpegLs { near_lossless: bool },
    J2k(j2k::Syntax),
    JpegXl(jpeg_xl::Mode),
    Rle,
    Deflate { max_frame_bytes: usize },
}

/// The Frame limit up to which a Deflated Image Frame is inflated, unless a
/// validation run's options set another: the default `max_pixel_frame_bytes`.
const DEFAULT_MAX_FRAME_BYTES: usize = 512 * 1024 * 1024;

impl Codec {
    fn of(syntax: &str) -> Option<Self> {
        Some(match syntax {
            uids::JPEG_BASELINE8_BIT => Self::Jpeg(jpeg::Process::Baseline),
            uids::JPEG_EXTENDED12_BIT => Self::Jpeg(jpeg::Process::Extended),
            uids::JPEG_LOSSLESS => Self::Jpeg(jpeg::Process::Lossless),
            uids::JPEG_LOSSLESS_SV1 => Self::Jpeg(jpeg::Process::LosslessSv1),
            uids::JPEGLS_LOSSLESS => Self::JpegLs {
                near_lossless: false,
            },
            uids::JPEGLS_NEAR_LOSSLESS => Self::JpegLs {
                near_lossless: true,
            },
            uids::JPEGXL_LOSSLESS => Self::JpegXl(jpeg_xl::Mode::Lossless),
            uids::JPEGXLJPEG_RECOMPRESSION => Self::JpegXl(jpeg_xl::Mode::JpegRecompression),
            uids::JPEGXL => Self::JpegXl(jpeg_xl::Mode::Any),
            uids::RLE_LOSSLESS => Self::Rle,
            uids::DEFLATED_IMAGE_FRAME_COMPRESSION => Self::Deflate {
                max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            },
            _ => Self::J2k(j2k::Syntax::of(syntax)?),
        })
    }

    /// This codec with the Frame limit of a validation run.
    fn limited_to(self, max_frame_bytes: usize) -> Self {
        match self {
            Self::Deflate { .. } => Self::Deflate { max_frame_bytes },
            other => other,
        }
    }

    fn citation(self) -> String {
        let sections = match self {
            Self::Jpeg(_) => "8.2.1 and A.4.1",
            Self::JpegLs { .. } => "8.2.3 and A.4.3",
            Self::J2k(syntax) if syntax.is_htj2k() => "8.2.14, 10.18 and A.4.4",
            Self::J2k(_) => "8.2.4 and A.4.4",
            Self::JpegXl(_) => "8.2.15 and A.4.12",
            Self::Rle => "8.2.2 and Annex G",
            Self::Deflate { .. } => "8.2.16 and A.4.13",
        };
        format!("DICOM PS3.5 {EDITION} {sections}")
    }

    fn check(self, frame: &Frame<'_>, report: &mut FrameReport<'_>) {
        match self {
            Self::Jpeg(process) => jpeg::check(frame, process, report),
            Self::JpegLs { near_lossless } => jpeg_ls::check(frame, near_lossless, report),
            Self::J2k(syntax) => j2k::check(frame, syntax, report),
            Self::JpegXl(mode) => jpeg_xl::check(frame, mode, report),
            Self::Rle => rle::check(frame, report),
            Self::Deflate { max_frame_bytes } => deflate::check(frame, max_frame_bytes, report),
        }
    }
}

/// One Frame's encoded bytes with what the Data Set declares about it.
struct Frame<'a> {
    bytes: &'a [u8],
    /// Where the Frame's second and later Fragments begin in `bytes`.
    fragment_starts: &'a [usize],
    declared: &'a Declared,
    /// The ICC Profile that describes this Frame, when the codec can embed one.
    profile: Option<icc::Profile<'a>>,
}

/// Violations of one Frame, reported with its Frame Number.
struct FrameReport<'a> {
    family: &'a mut Family,
    number: usize,
}

impl FrameReport<'_> {
    /// Counts a violation of the `reference`d sections; `message` is formatted
    /// only while it will be shown.
    fn violation(&mut self, reference: &'static str, message: impl FnOnce() -> String) {
        let number = self.number;
        self.family
            .push(|| format!("Frame {number}: {} ({})", message(), cite(reference)));
    }
}

/// "PS3.5 8.2.4 and PS3.3 C.7.6.3.1.1" with the pinned edition after each part.
fn cite(reference: &str) -> String {
    reference
        .split(" and ")
        .map(|piece| match piece.split_once(' ') {
            Some((part, section)) => format!("{part} {EDITION} {section}"),
            None => piece.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// The Image Pixel attributes a codestream must agree with. Absent or
/// malformed attributes are reported by the IOD checks and skipped here.
#[derive(Debug)]
struct Declared {
    syntax: String,
    rows: Attribute,
    columns: Attribute,
    samples_per_pixel: Attribute,
    bits_stored: Attribute,
    bits_allocated: Attribute,
    pixel_representation: Attribute,
    photometric: Option<String>,
}

impl Declared {
    fn read(object: &InMemDicomObject, syntax: &str) -> Self {
        let attribute = |tag: Tag, name: &'static str| Attribute {
            name,
            value: object
                .element(tag)
                .ok()
                .and_then(|element| element.to_int::<u32>().ok()),
        };
        Self {
            syntax: syntax.to_string(),
            rows: attribute(tags::ROWS, "Rows (0028,0010)"),
            columns: attribute(tags::COLUMNS, "Columns (0028,0011)"),
            samples_per_pixel: attribute(tags::SAMPLES_PER_PIXEL, "Samples per Pixel (0028,0002)"),
            bits_stored: attribute(tags::BITS_STORED, "Bits Stored (0028,0101)"),
            bits_allocated: attribute(tags::BITS_ALLOCATED, "Bits Allocated (0028,0100)"),
            pixel_representation: attribute(
                tags::PIXEL_REPRESENTATION,
                "Pixel Representation (0028,0103)",
            ),
            photometric: object
                .element(tags::PHOTOMETRIC_INTERPRETATION)
                .ok()
                .and_then(|element| element.to_str().ok())
                .map(|value| value.trim_matches([' ', '\0']).to_string()),
        }
    }

    fn photometric_is(&self, values: &[&str]) -> bool {
        self.photometric
            .as_deref()
            .is_some_and(|photometric| values.contains(&photometric))
    }

    fn photometric(&self) -> &str {
        self.photometric.as_deref().unwrap_or("absent")
    }
}

/// One unsigned Image Pixel attribute, by name for messages.
#[derive(Clone, Copy, Debug)]
struct Attribute {
    name: &'static str,
    value: Option<u32>,
}

impl Attribute {
    /// Reports a codestream `field` whose value differs from this attribute's.
    fn compare(
        self,
        report: &mut FrameReport<'_>,
        reference: &'static str,
        field: &str,
        actual: u32,
    ) {
        if let Some(expected) = self.value.filter(|expected| *expected != actual) {
            report.violation(reference, || {
                format!("{field} is {actual} but {} is {expected}", self.name)
            });
        }
    }
}

/// Big-endian reads within a marker segment; `None` past its end.
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(count)?;
        let taken = self.bytes.get(self.position..end)?;
        self.position = end;
        Some(taken)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|bytes| bytes[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

/// The codestream without the zero bytes that pad a Frame to even length
/// after its end marker (PS3.5 A.4).
fn without_trailing_padding(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |last| last + 1);
    &bytes[..end]
}

/// What the first bytes of a Frame are, for a codestream of the wrong kind.
fn signature(bytes: &[u8]) -> String {
    const KNOWN: [(&[u8], &str); 6] = [
        (
            &[0xFF, 0x4F, 0xFF, 0x51],
            "a JPEG 2000 codestream: SOC, SIZ",
        ),
        (
            &[0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20],
            "a JP2 file: signature box",
        ),
        (
            &[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20],
            "a JPEG XL container: signature box",
        ),
        (&[0xFF, 0x0A], "a JPEG XL codestream"),
        (&[0xFF, 0xD8, 0xFF, 0xF7], "a JPEG-LS stream: SOI, SOF55"),
        (&[0xFF, 0xD8], "a JPEG stream: SOI"),
    ];
    let shown = &bytes[..bytes.len().min(4)];
    let hex = shown
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    match KNOWN.iter().find(|(prefix, _)| bytes.starts_with(prefix)) {
        Some((_, name)) => format!("bytes {hex} ({name})"),
        None if bytes.is_empty() => "no bytes".to_string(),
        None => format!("bytes {hex}"),
    }
}
