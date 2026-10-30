use std::borrow::Cow;
use std::path::PathBuf;

use dicom_core::value::{PixelFragmentSequence, Value};
use dicom_core::{Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::iod::EDITION;
use super::transfer_syntaxes::{pixel_encoding, smpte_st2110, Fragments, PixelEncoding};
use super::{failed_check, intrinsic_check, ValidationCheck, ValidationStatus};

pub(super) fn run_intrinsic_pixel_structure_check(
    file: &PathBuf,
    object: &DefaultDicomObject,
    max_frame_bytes: usize,
) -> ValidationCheck {
    match check_pixel_structure(object, max_frame_bytes) {
        Ok(()) => intrinsic_check(
            "intrinsic-pixel-structure",
            Some(file.clone()),
            ValidationStatus::Passed,
            "intrinsic Pixel Data structure is valid".into(),
        ),
        Err(message) => failed_check("intrinsic-pixel-structure", Some(file), message),
    }
}

/// The Pixel Data encoding PS3.5 requires of the declared Transfer Syntax:
/// which pixel element is present, and the Native length or encapsulated
/// Fragment structure. Framing that only the encoded bytes show (the
/// encapsulated VR, Item Lengths and delimiters) is the encoding check's.
fn check_pixel_structure(
    object: &DefaultDicomObject,
    max_frame_bytes: usize,
) -> Result<(), String> {
    let transfer_syntax = object.meta().transfer_syntax.trim_end_matches('\0');
    if let Some((name, sections)) = smpte_st2110(transfer_syntax) {
        return Err(format!(
            "Transfer Syntax {transfer_syntax} ({name}) describes a DICOM-RTV Flow, carried outside any file, and defines no encoding of a Data Set (PS3.5 {EDITION} {sections}); a file's Transfer Syntax UID identifies the Transfer Syntax that encodes its Data Set (PS3.10 {EDITION} 7.1 and 7.2), so no file uses it"
        ));
    }
    let encoding = pixel_encoding(transfer_syntax);
    let Some(pixel_data) = check_pixel_elements(object, transfer_syntax, encoding)? else {
        return Ok(());
    };
    let frame_count = dicom_frame_count(object)?;
    match (encoding, pixel_data.value()) {
        (PixelEncoding::Encapsulated(fragments), Value::PixelSequence(sequence)) => {
            check_encapsulated(
                object,
                sequence,
                fragments,
                frame_count,
                max_frame_bytes,
            )
        }
        (PixelEncoding::Encapsulated(_), _) => Err(format!(
            "Encapsulated Transfer Syntax {transfer_syntax} requires Pixel Data (7FE0,0010) in Encapsulated Format in the top-level Data Set (PS3.5 {EDITION} A.4)"
        )),
        (_, Value::Primitive(_)) => {
            check_native(object, pixel_data, transfer_syntax, frame_count)
        }
        (_, Value::PixelSequence(_)) => Err(format!(
            "native transfer syntax {transfer_syntax} cannot contain encapsulated Pixel Data; Native Format has an Explicit Value Length (PS3.5 {EDITION} A.1 and A.2)"
        )),
        (_, _) => Err("Pixel Data has an unusable value representation".into()),
    }
}

/// Which of the mutually exclusive pixel elements the top-level Data Set
/// holds (PS3.5 8.2, A.4, A.6, A.11), and the Pixel Data element to check
/// further, if any.
fn check_pixel_elements<'a>(
    object: &'a DefaultDicomObject,
    transfer_syntax: &str,
    encoding: PixelEncoding,
) -> Result<Option<&'a InMemElement>, String> {
    const URL: (Tag, &str) = (
        tags::PIXEL_DATA_PROVIDER_URL,
        "Pixel Data Provider URL (0028,7FE0)",
    );
    const PIXELS: [(Tag, &str); 3] = [
        (tags::PIXEL_DATA, "Pixel Data (7FE0,0010)"),
        (tags::FLOAT_PIXEL_DATA, "Float Pixel Data (7FE0,0008)"),
        (
            tags::DOUBLE_FLOAT_PIXEL_DATA,
            "Double Float Pixel Data (7FE0,0009)",
        ),
    ];
    let present = |elements: &[(Tag, &'static str)]| -> Vec<&'static str> {
        elements
            .iter()
            .filter(|(tag, _)| object.get(*tag).is_some())
            .map(|(_, name)| *name)
            .collect()
    };
    let pixels = present(&PIXELS);
    let all = present(&[URL])
        .into_iter()
        .chain(pixels.iter().copied())
        .collect::<Vec<_>>();
    if all.len() > 1 {
        return Err(format!(
            "{} are present together; the top-level Data Set shall hold only one of Pixel Data Provider URL, Pixel Data, Float Pixel Data and Double Float Pixel Data (PS3.5 {EDITION} 8.2)",
            all.join(" and ")
        ));
    }
    let float = pixels.first().is_some_and(|name| *name != PIXELS[0].1);
    match encoding {
        PixelEncoding::Referenced => {
            if let Some(name) = pixels.first() {
                return Err(format!(
                    "{name} is present, but JPIP Referenced Transfer Syntax {transfer_syntax} references integer Pixel Data through Pixel Data Provider URL (0028,7FE0) instead (PS3.5 {EDITION} A.6 and A.11)"
                ));
            }
            check_provider_url(object, transfer_syntax)?;
            Ok(None)
        }
        PixelEncoding::Encapsulated(_) if float => Err(format!(
            "{} is present, but Encapsulated Transfer Syntax {transfer_syntax} is used only with integer Pixel Data (7FE0,0010) in the top-level Data Set (PS3.5 {EDITION} A.4)",
            pixels[0]
        )),
        // Floating point Pixel Data is always in Native Format (PS3.5 8.2).
        PixelEncoding::Native if float => Ok(None),
        // A Data Set without the Image Pixel Module, such as an SR or RT
        // object, has no Pixel Data to check.
        _ if pixels.is_empty() && object.get(tags::ROWS).is_none() => Ok(None),
        _ => object.element(tags::PIXEL_DATA).map(Some).map_err(|_| {
            format!(
                "Pixel Data (7FE0,0010) is missing; a Data Set with the Image Pixel Module holds Pixel Data, Float Pixel Data or Double Float Pixel Data (PS3.3 {EDITION} C.7.6.3)"
            )
        }),
    }
}

/// PS3.5 A.6 and A.11 replace Pixel Data with a Pixel Data Provider URL,
/// whose transport is HTTP or HTTPS (PS3.5 8.4.1).
fn check_provider_url(object: &DefaultDicomObject, transfer_syntax: &str) -> Result<(), String> {
    let url = object
        .element(tags::PIXEL_DATA_PROVIDER_URL)
        .map_err(|_| {
            format!(
                "Pixel Data Provider URL (0028,7FE0) is missing; JPIP Referenced Transfer Syntax {transfer_syntax} references the Pixel Data through it (PS3.5 {EDITION} A.6 and A.11)"
            )
        })?
        .to_str()
        .map_err(|err| format!("Pixel Data Provider URL (0028,7FE0) is not text: {err}"))?;
    let url = url.trim_matches([' ', '\0']);
    let scheme = url.split_once(':').map(|(scheme, _)| scheme);
    if !scheme.is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    }) {
        return Err(format!(
            "Pixel Data Provider URL (0028,7FE0) {} does not use HTTP or HTTPS; the JPIP transport protocol shall be HTTP or HTTPS (PS3.5 {EDITION} 8.4.1)",
            crate::validation::disclosure::shown(tags::PIXEL_DATA_PROVIDER_URL, url)
        ));
    }
    Ok(())
}

/// PS3.5 A.4 and the Fragment rule of the syntax (A.4.x, 8.2.5-8.2.11),
/// with the offset tables of PS3.3 C.7.6.3.1.8.
fn check_encapsulated(
    object: &DefaultDicomObject,
    sequence: &PixelFragmentSequence<Vec<u8>>,
    fragments: Fragments,
    frame_count: usize,
    max_frame_bytes: usize,
) -> Result<(), String> {
    let items = sequence.fragments();
    if items.is_empty() {
        return Err(format!(
            "encapsulated Pixel Data has no Fragment; at least one Frame, and so one Fragment, shall be present (PS3.5 {EDITION} 8.2)"
        ));
    }
    let extended_offsets = optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE)?;
    let extended_lengths = optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE_LENGTHS)?;
    let extended = extended_offsets
        .as_deref()
        .is_some_and(|offsets| !offsets.is_empty());
    if extended && !sequence.offset_table().is_empty() {
        return Err(format!(
            "the Basic Offset Table holds {} offsets although Extended Offset Table (7FE0,0001) is present; the Basic Offset Table Item shall then be empty (PS3.5 {EDITION} A.4, PS3.3 {EDITION} C.7.6.3.1.8)",
            sequence.offset_table().len()
        ));
    }
    if let Fragments::Video {
        section,
        single_fragment,
        empty_offset_table,
    } = fragments
    {
        if empty_offset_table && !sequence.offset_table().is_empty() {
            return Err(format!(
                "the Basic Offset Table holds {} offsets; it shall be empty (present but zero length) for this MPEG2 Transfer Syntax (PS3.5 {EDITION} {section})",
                sequence.offset_table().len()
            ));
        }
        if single_fragment && items.len() != 1 {
            return Err(format!(
                "Pixel Data holds {} Fragments; for a Non-Fragmentable Transfer Syntax one Fragment shall contain the whole bit stream (PS3.5 {EDITION} {section})",
                items.len()
            ));
        }
        // One stream carries every Frame, so Frames map to Fragments only
        // through an Extended Offset Table.
        if !extended {
            return Ok(());
        }
    }
    assemble_encapsulated_frames(
        sequence,
        frame_count,
        extended_offsets.as_deref(),
        extended_lengths.as_deref(),
        0,
        max_frame_bytes,
    )?;
    if extended && items.len() != frame_count {
        return Err(format!(
            "Extended Offset Table (7FE0,0001) is present, but {} Fragments encode {frame_count} Frames; it addresses the one and only Fragment of every Frame (PS3.3 {EDITION} C.7.6.3.1.8)",
            items.len()
        ));
    }
    if let Fragments::OnePerFrame {
        section,
        native_payload,
    } = fragments
    {
        if items.len() != frame_count {
            return Err(format!(
                "{} Fragments encode {frame_count} Frames; each Frame shall be encoded in one and only one Fragment (PS3.5 {EDITION} {section})",
                items.len()
            ));
        }
        if native_payload {
            let frame = NativeFrame::of(object)?;
            let expected = frame.bytes(1)?;
            if let Some((index, item)) = items
                .iter()
                .enumerate()
                .find(|(_, item)| !is_padded_length(item.len() as u64, expected))
            {
                return Err(format!(
                    "Fragment {} holds {} bytes; each Fragment holds one Frame encoded as in Native Format, {expected} bytes for {} padded to even length (PS3.5 {EDITION} {section})",
                    index + 1,
                    item.len(),
                    frame.describe(1)
                ));
            }
        }
    }
    Ok(())
}

/// PS3.5 8.1.1 Native length and A.1/A.2 Value Representation.
fn check_native(
    object: &DefaultDicomObject,
    pixel_data: &InMemElement,
    transfer_syntax: &str,
    frame_count: usize,
) -> Result<(), String> {
    let bytes = pixel_data
        .to_bytes()
        .map_err(|err| format!("failed to inspect primitive Pixel Data: {err}"))?;
    if bytes.is_empty() {
        return Err("primitive Pixel Data is empty".into());
    }
    let frame = NativeFrame::of(object)?;
    let frames = frame_count as u64;
    let expected = frame.bytes(frames)?;
    if !is_padded_length(bytes.len() as u64, expected) {
        if primitive_pixel_data_contains_encapsulated_item_stream(&bytes) {
            return Err(format!(
                "native transfer syntax {transfer_syntax} cannot contain an encapsulated item stream in Pixel Data"
            ));
        }
        return Err(format!(
            "Pixel Data (7FE0,0010) holds {} bytes; Native Format requires {expected} bytes for {}, padded only to the next even length (PS3.5 {EDITION} 8.1.1)",
            bytes.len(),
            frame.describe(frames)
        ));
    }
    // Implicit VR conveys no VR; there Pixel Data is OW (PS3.5 A.1).
    if transfer_syntax != uids::IMPLICIT_VR_LITTLE_ENDIAN {
        let vr = pixel_data.vr();
        if frame.bits_allocated > 8 && vr != VR::OW {
            return Err(format!(
                "Native Pixel Data (7FE0,0010) has VR {vr:?} with Bits Allocated {}; Bits Allocated greater than 8 requires OW (PS3.5 {EDITION} A.2)",
                frame.bits_allocated
            ));
        }
        if !matches!(vr, VR::OB | VR::OW) {
            return Err(format!(
                "Native Pixel Data (7FE0,0010) has VR {vr:?}; it shall be OB or OW (PS3.5 {EDITION} A.2)"
            ));
        }
    }
    Ok(())
}

/// The size of one Frame in Native Format (PS3.5 8.1.1): Rows x Columns
/// Pixels of Samples per Pixel cells, each Bits Allocated bits. YBR_FULL_422
/// stores two samples per Pixel (PS3.3 C.7.6.3.1.2).
#[derive(Clone, Copy, Debug)]
pub(in crate::validation) struct NativeFrame {
    rows: u64,
    columns: u64,
    samples: u64,
    bits_allocated: u64,
    stored_samples: u64,
}

impl NativeFrame {
    pub(in crate::validation) fn of(item: &InMemDicomObject) -> Result<Self, String> {
        let attribute = |tag, name: &str| {
            item.get(tag)
                .and_then(|element| element.to_int::<u64>().ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    format!(
                        "{name} is missing or zero, so the Native Pixel Data length cannot be determined (PS3.5 {EDITION} 8.1.1)"
                    )
                })
        };
        let rows = attribute(tags::ROWS, "Rows (0028,0010)")?;
        let columns = attribute(tags::COLUMNS, "Columns (0028,0011)")?;
        let samples = attribute(tags::SAMPLES_PER_PIXEL, "Samples per Pixel (0028,0002)")?;
        let bits_allocated = attribute(tags::BITS_ALLOCATED, "Bits Allocated (0028,0100)")?;
        let ybr_full_422 = item
            .get(tags::PHOTOMETRIC_INTERPRETATION)
            .and_then(|element| element.to_str().ok())
            .is_some_and(|value| value.trim_matches([' ', '\0']) == "YBR_FULL_422");
        Self::new(rows, columns, samples, bits_allocated, ybr_full_422)
            .ok_or_else(|| "Native Pixel Data size overflows 64 bits".to_string())
    }

    /// The Frame of these Image Pixel Attribute Values; `None` when its sample
    /// count overflows.
    pub(in crate::validation) fn new(
        rows: u64,
        columns: u64,
        samples: u64,
        bits_allocated: u64,
        ybr_full_422: bool,
    ) -> Option<Self> {
        let stored_samples = if ybr_full_422 {
            columns.div_ceil(2).checked_mul(4)
        } else {
            columns.checked_mul(samples)
        }?
        .checked_mul(rows)?;
        Some(Self {
            rows,
            columns,
            samples,
            bits_allocated,
            stored_samples,
        })
    }

    /// Bytes of `frames` concatenated Frames, before even-length padding.
    pub(in crate::validation) fn bytes(self, frames: u64) -> Result<u64, String> {
        self.stored_samples
            .checked_mul(self.bits_allocated)
            .and_then(|bits| bits.checked_mul(frames))
            .map(|bits| bits.div_ceil(8))
            .ok_or_else(|| "Native Pixel Data size overflows 64 bits".to_string())
    }

    pub(in crate::validation) fn describe(self, frames: u64) -> String {
        format!(
            "{} Rows x {} Columns x {} Samples per Pixel x {frames} Frame(s) at {} Bits Allocated",
            self.rows, self.columns, self.samples, self.bits_allocated
        )
    }
}

/// Whether a Value of `actual` bytes is `expected` bytes padded to the next
/// even length, as PS3.5 8.1.1 allows and requires.
pub(in crate::validation) fn is_padded_length(actual: u64, expected: u64) -> bool {
    expected
        .checked_add(expected % 2)
        .is_some_and(|padded| actual == padded)
}

/// The Frames of the Pixel Data. Without the Multi-frame Module the image has
/// one Frame; an IOD that requires Number of Frames is the presence check's to
/// judge.
pub(super) fn dicom_frame_count(
    object: &dicom_object::DefaultDicomObject,
) -> Result<usize, String> {
    let Some(element) = object.get(tags::NUMBER_OF_FRAMES) else {
        return Ok(1);
    };
    match element.to_int::<usize>() {
        Ok(frame_count) if frame_count > 0 => Ok(frame_count),
        Ok(_) => Err("DICOM Number of Frames must be greater than zero".into()),
        Err(err) => Err(format!("failed to read DICOM Number of Frames: {err}")),
    }
}

fn primitive_pixel_data_contains_encapsulated_item_stream(bytes: &[u8]) -> bool {
    const ITEM_TAG: [u8; 4] = [0xFE, 0xFF, 0x00, 0xE0];
    const SEQUENCE_DELIMITATION_TAG: [u8; 4] = [0xFE, 0xFF, 0xDD, 0xE0];
    const ITEM_HEADER_BYTES: usize = 8;

    // A malformed writer can assign a native transfer syntax and then serialize encapsulated
    // items with a defined Pixel Data length. In that case the object parser exposes primitive
    // bytes, so require a complete item-stream parse rather than relying on the first tag alone.
    let Some(mut cursor) = item_end(bytes, 0) else {
        return false;
    };
    let basic_offset_table_length = cursor - ITEM_HEADER_BYTES;
    if !basic_offset_table_length.is_multiple_of(4) {
        return false;
    }

    let mut fragment_count = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(&ITEM_TAG) {
            let Some(next_cursor) = item_end(bytes, cursor) else {
                return false;
            };
            if !(next_cursor - cursor - ITEM_HEADER_BYTES).is_multiple_of(2) {
                return false;
            }
            fragment_count += 1;
            cursor = next_cursor;
            continue;
        }
        if bytes[cursor..].starts_with(&SEQUENCE_DELIMITATION_TAG) {
            let Some(header_end) = cursor.checked_add(ITEM_HEADER_BYTES) else {
                return false;
            };
            if header_end != bytes.len() {
                return false;
            }
            let Ok(length_bytes) = bytes[cursor + 4..header_end].try_into() else {
                return false;
            };
            let length = u32::from_le_bytes(length_bytes);
            return fragment_count > 0 && length == 0;
        }
        return false;
    }

    fragment_count > 0
}

fn item_end(bytes: &[u8], offset: usize) -> Option<usize> {
    const ITEM_TAG: [u8; 4] = [0xFE, 0xFF, 0x00, 0xE0];
    const ITEM_HEADER_BYTES: usize = 8;

    let header_end = offset.checked_add(ITEM_HEADER_BYTES)?;
    let header = bytes.get(offset..header_end)?;
    if !header.starts_with(&ITEM_TAG) {
        return None;
    }
    let length = u32::from_le_bytes(header[4..8].try_into().ok()?);
    if length == u32::MAX {
        return None;
    }
    header_end
        .checked_add(usize::try_from(length).ok()?)
        .filter(|end| *end <= bytes.len())
}

pub(super) fn optional_u64_values(
    object: &dicom_object::DefaultDicomObject,
    tag: dicom_core::Tag,
) -> Result<Option<Vec<u64>>, String> {
    let Ok(element) = object.element(tag) else {
        return Ok(None);
    };
    element
        .to_multi_int::<u64>()
        .map(Some)
        .map_err(|err| format!("failed to read DICOM element {tag}: {err}"))
}

pub(super) fn assemble_encapsulated_frames(
    sequence: &PixelFragmentSequence<Vec<u8>>,
    frame_count: usize,
    extended_offsets: Option<&[u64]>,
    extended_lengths: Option<&[u64]>,
    max_frames: usize,
    max_frame_bytes: usize,
) -> Result<Vec<Vec<u8>>, String> {
    let fragments = sequence.fragments();
    let (offsets, lengths) =
        frame_offsets_and_lengths(sequence, frame_count, extended_offsets, extended_lengths)?;

    visit_encapsulated_frame_spans(
        fragments,
        frame_count,
        offsets,
        lengths,
        max_frame_bytes,
        |_, _, _, _| Ok(()),
    )?;

    let selected_frames = max_frames.min(frame_count);
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(selected_frames)
        .map_err(|_| "Pixel Data frame list exceeds available memory".to_string())?;
    visit_encapsulated_frame_spans(
        fragments,
        frame_count,
        offsets,
        lengths,
        max_frame_bytes,
        |frame_index, start, end, output_len| {
            if frame_index < selected_frames {
                let mut frame = Vec::new();
                frame.try_reserve_exact(output_len).map_err(|_| {
                    format!("Pixel Data frame {frame_index} exceeds available memory")
                })?;
                for fragment in &fragments[start..end] {
                    let remaining = output_len.saturating_sub(frame.len());
                    if remaining == 0 {
                        break;
                    }
                    frame.extend_from_slice(&fragment[..fragment.len().min(remaining)]);
                }
                frames.push(frame);
            }
            Ok(())
        },
    )?;
    Ok(frames)
}

/// One Frame of encapsulated Pixel Data: the Fragments it spans and the
/// length of its payload within them.
pub(super) struct EncapsulatedFrame<'a> {
    pub(super) index: usize,
    pub(super) fragments: &'a [Vec<u8>],
    len: usize,
}

impl<'a> EncapsulatedFrame<'a> {
    /// The Frame payload, borrowed when it lies in one Fragment.
    pub(super) fn bytes(&self) -> Result<Cow<'a, [u8]>, String> {
        if let [fragment] = self.fragments {
            return Ok(Cow::Borrowed(&fragment[..self.len.min(fragment.len())]));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(self.len)
            .map_err(|_| format!("Pixel Data frame {} exceeds available memory", self.index))?;
        for fragment in self.fragments {
            let remaining = self.len - bytes.len();
            bytes.extend_from_slice(&fragment[..fragment.len().min(remaining)]);
        }
        Ok(Cow::Owned(bytes))
    }

    /// The Frame payload as slices of its Fragments, in order, without
    /// copying them.
    pub(super) fn parts(&self) -> impl Iterator<Item = &'a [u8]> {
        let mut remaining = self.len;
        self.fragments.iter().map(move |fragment| {
            let part = &fragment[..fragment.len().min(remaining)];
            remaining -= part.len();
            part
        })
    }
}

/// Calls `visit` with every Frame in order, once the whole Fragment-to-Frame
/// mapping is known to be valid, without copying Fragments.
pub(super) fn visit_encapsulated_frames(
    sequence: &PixelFragmentSequence<Vec<u8>>,
    frame_count: usize,
    extended_offsets: Option<&[u64]>,
    extended_lengths: Option<&[u64]>,
    max_frame_bytes: usize,
    mut visit: impl FnMut(EncapsulatedFrame<'_>),
) -> Result<(), String> {
    let fragments = sequence.fragments();
    let (offsets, lengths) =
        frame_offsets_and_lengths(sequence, frame_count, extended_offsets, extended_lengths)?;
    visit_encapsulated_frame_spans(
        fragments,
        frame_count,
        offsets,
        lengths,
        max_frame_bytes,
        |_, _, _, _| Ok(()),
    )?;
    visit_encapsulated_frame_spans(
        fragments,
        frame_count,
        offsets,
        lengths,
        max_frame_bytes,
        |index, start, end, len| {
            visit(EncapsulatedFrame {
                index,
                fragments: &fragments[start..end],
                len,
            });
            Ok(())
        },
    )
}

fn frame_offsets_and_lengths<'a>(
    sequence: &'a PixelFragmentSequence<Vec<u8>>,
    frame_count: usize,
    extended_offsets: Option<&'a [u64]>,
    extended_lengths: Option<&'a [u64]>,
) -> Result<(EncapsulatedFrameOffsets<'a>, Option<&'a [u64]>), String> {
    if sequence.fragments().is_empty() {
        return Err("Pixel Data has no fragments".to_string());
    }

    let basic_offsets = sequence.offset_table();
    let offsets = match extended_offsets {
        Some(offsets) if !offsets.is_empty() => EncapsulatedFrameOffsets::Extended(offsets),
        _ if !basic_offsets.is_empty() => EncapsulatedFrameOffsets::Basic(basic_offsets),
        _ => EncapsulatedFrameOffsets::None,
    };
    let lengths = extended_lengths.filter(|lengths| !lengths.is_empty());
    if lengths.is_some() && extended_offsets.is_none_or(<[u64]>::is_empty) {
        return Err("Extended Offset Table Lengths requires an Extended Offset Table".to_string());
    }
    if let Some(lengths) = lengths {
        if lengths.len() != frame_count {
            return Err(format!(
                "Extended Offset Table Lengths has {} entries for {frame_count} frames",
                lengths.len()
            ));
        }
    }
    Ok((offsets, lengths))
}

#[derive(Clone, Copy)]
enum EncapsulatedFrameOffsets<'a> {
    None,
    Basic(&'a [u32]),
    Extended(&'a [u64]),
}

impl EncapsulatedFrameOffsets<'_> {
    fn len(self) -> usize {
        match self {
            Self::None => 0,
            Self::Basic(offsets) => offsets.len(),
            Self::Extended(offsets) => offsets.len(),
        }
    }

    fn get(self, index: usize) -> Option<u64> {
        match self {
            Self::None => None,
            Self::Basic(offsets) => offsets.get(index).copied().map(u64::from),
            Self::Extended(offsets) => offsets.get(index).copied(),
        }
    }
}

fn visit_encapsulated_frame_spans(
    fragments: &[Vec<u8>],
    frame_count: usize,
    offsets: EncapsulatedFrameOffsets<'_>,
    lengths: Option<&[u64]>,
    max_frame_bytes: usize,
    mut visit: impl FnMut(usize, usize, usize, usize) -> Result<(), String>,
) -> Result<(), String> {
    if offsets.len() == 0 {
        if frame_count == 1 {
            return validate_and_visit_frame_span(
                fragments,
                lengths,
                max_frame_bytes,
                0,
                0,
                fragments.len(),
                &mut visit,
            );
        }
        if frame_count != fragments.len() {
            return Err(format!(
                "cannot map {} Pixel Data fragments to {frame_count} frames without an offset table",
                fragments.len()
            ));
        }
        for frame_index in 0..frame_count {
            validate_and_visit_frame_span(
                fragments,
                lengths,
                max_frame_bytes,
                frame_index,
                frame_index,
                frame_index + 1,
                &mut visit,
            )?;
        }
        return Ok(());
    }

    if offsets.len() != frame_count {
        return Err(format!(
            "Pixel Data offset table has {} entries for {frame_count} frames",
            offsets.len()
        ));
    }
    if offsets.get(0) != Some(0)
        || (1..frame_count).any(|index| offsets.get(index - 1) >= offsets.get(index))
    {
        return Err("Pixel Data frame offsets are not strictly increasing from zero".to_string());
    }

    let mut fragment_index = 0usize;
    let mut fragment_offset = 0u64;
    for frame_index in 0..frame_count {
        let expected_start = offsets.get(frame_index).expect("validated offset count");
        if fragment_offset != expected_start {
            return Err(format!(
                "Pixel Data frame offset {expected_start} does not identify a fragment boundary"
            ));
        }
        let start = fragment_index;
        let expected_end = offsets.get(frame_index + 1);
        while fragment_index < fragments.len()
            && expected_end.is_none_or(|end| fragment_offset < end)
        {
            let fragment_len = u64::try_from(fragments[fragment_index].len())
                .map_err(|_| "Pixel Data fragment length exceeds u64".to_string())?;
            fragment_offset = fragment_offset
                .checked_add(8)
                .and_then(|offset| offset.checked_add(fragment_len))
                .ok_or_else(|| "Pixel Data fragment offsets overflow u64".to_string())?;
            fragment_index += 1;
        }
        if expected_end.is_some_and(|end| fragment_offset != end) {
            return Err(format!(
                "Pixel Data frame offset {} does not identify a fragment boundary",
                expected_end.expect("checked Some")
            ));
        }
        validate_and_visit_frame_span(
            fragments,
            lengths,
            max_frame_bytes,
            frame_index,
            start,
            fragment_index,
            &mut visit,
        )?;
    }
    if fragment_index != fragments.len() {
        return Err("Pixel Data offset table does not map all fragments".to_string());
    }
    Ok(())
}

fn validate_and_visit_frame_span(
    fragments: &[Vec<u8>],
    lengths: Option<&[u64]>,
    max_frame_bytes: usize,
    frame_index: usize,
    start: usize,
    end: usize,
    visit: &mut impl FnMut(usize, usize, usize, usize) -> Result<(), String>,
) -> Result<(), String> {
    if start >= end || end > fragments.len() {
        return Err(format!(
            "Pixel Data frame {frame_index} has an empty or invalid fragment span"
        ));
    }
    let assembled_len = fragments[start..end]
        .iter()
        .try_fold(0usize, |total, fragment| {
            total
                .checked_add(fragment.len())
                .ok_or_else(|| "assembled Pixel Data frame length overflows usize".to_string())
        })?;
    let output_len = match lengths {
        Some(lengths) => usize::try_from(lengths[frame_index]).map_err(|_| {
            format!("Pixel Data frame {frame_index} length exceeds platform limits")
        })?,
        None => assembled_len,
    };
    if output_len == 0 {
        return Err(format!("Pixel Data frame {frame_index} is empty"));
    }
    if output_len > assembled_len {
        if end == start + 1 && output_len.checked_sub(assembled_len) == Some(8) {
            return Err(format!(
                "Pixel Data frame {frame_index} Extended Offset Table Lengths entry includes the 8-byte Item header; DICOM requires only Frame payload bytes"
            ));
        }
        return Err(format!(
            "Pixel Data frame {frame_index} declares {output_len} bytes but only {assembled_len} are available"
        ));
    }
    if output_len > max_frame_bytes {
        return Err(format!(
            "Pixel Data frame {frame_index} exceeds {max_frame_bytes} byte validation limit"
        ));
    }
    visit(frame_index, start, end, output_len)
}

#[cfg(any(test, feature = "bench-internals"))]
pub(crate) fn fragment_payload_without_padding(fragment: &[u8]) -> &[u8] {
    fragment
}
