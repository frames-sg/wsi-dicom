//! Pixel description content beyond the top-level sample format: Icon Image
//! restrictions, Palette Color Lookup Tables, Pixel Padding and Pixel Aspect
//! Ratio Values.

use std::fmt::Display;

use dicom_core::value::Value;
use dicom_core::{PrimitiveValue, Tag};
use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::super::pixel_structure::{is_padded_length, NativeFrame};
use super::nested::Nested;
use crate::validation::iod::{dicom_edition, EDITION};
use crate::validation::iod::{integer_value, items};

/// Every violation found, joined; the object's pixel description content is
/// independent, so one violation does not hide the others. `iod` decides
/// the entry size of top-level Palette Color Lookup Tables; `nested` holds
/// every Icon Image, at the top level or in an Item such as a DICOMDIR's
/// Directory Record (PS3.3 F.5).
pub(super) fn validate_pixel_attributes_rule(
    object: &DefaultDicomObject,
    iod: &str,
    nested: &Nested<'_>,
) -> Result<(), String> {
    let mut violations = Vec::new();
    pixel_aspect_ratio(object, &mut violations);
    pixel_padding(object, &mut violations);
    for found in &nested.icons {
        if found.in_record() {
            record_icon_image(found.item, &found.icon_label(), &mut violations);
        } else {
            icon_image(found.item, &found.icon_label(), &mut violations);
        }
    }
    let single_sample = integer(object, tags::SAMPLES_PER_PIXEL) == Some(1);
    top_level_palette(object, iod, single_sample, &mut violations);
    for (index, path) in items(object, tags::OPTICAL_PATH_SEQUENCE)
        .iter()
        .enumerate()
    {
        for palette in items(path, tags::PALETTE_COLOR_LOOKUP_TABLE_SEQUENCE) {
            let label = format!(
                "Optical Path Sequence (0048,0105) Item {} Palette Color Lookup Table Sequence (0048,0120)",
                index + 1
            );
            if !single_sample {
                violations.push(format!(
                    "{label} is present, but Samples per Pixel is not 1; Palette Color Lookup Tables are used only with a single sample per pixel (PS3.3 {EDITION} C.7.6.3.1.6 and C.7.9.2)"
                ));
            }
            // C.7-22a belongs to C.7.9, which in an Image IOD requires 16-bit entries.
            palette_lookup_tables(palette, &label, Some(16), &mut violations);
        }
    }
    super::violation_result(violations)
}

/// The Icon Image of a Directory Record. PS3.3 F.7 restricts it in place of
/// C.7.6.1.1.6, which defers to it ("unless otherwise specified"), and the
/// File-set check holds F.7; the Image Pixel Macro still binds its sample
/// format (Table C.7-11c), Pixel Aspect Ratio (C.7.6.3.1.7), Palette Color
/// Lookup Tables and Pixel Data.
fn record_icon_image(icon: &InMemDicomObject, label: &dyn Display, violations: &mut Vec<String>) {
    let allocated = integer(icon, tags::BITS_ALLOCATED);
    let stored = integer(icon, tags::BITS_STORED);
    if let Some(bits) = allocated.filter(|bits| *bits != 1 && bits % 8 != 0) {
        violations.push(format!(
            "{label}: Bits Allocated is {bits}; it shall be either 1 or a multiple of 8 (PS3.3 {EDITION} Table C.7-11c)"
        ));
    }
    if let (Some(high_bit), Some(stored)) = (integer(icon, tags::HIGH_BIT), stored) {
        if high_bit.checked_add(1) != Some(stored) {
            violations.push(format!(
                "{label}: High Bit is {high_bit}; it shall be one less than Bits Stored {stored} (PS3.3 {EDITION} Table C.7-11c)"
            ));
        }
    }
    let mut aspect = Vec::new();
    pixel_aspect_ratio(icon, &mut aspect);
    violations.extend(
        aspect
            .into_iter()
            .map(|violation| format!("{label}: {violation}")),
    );
    if text(icon, tags::PHOTOMETRIC_INTERPRETATION).as_deref() == Some("PALETTE COLOR") {
        palette_lookup_tables(icon, label, None, violations);
    }
    icon_pixel_data(icon, label, violations);
}

/// The Palette Color Lookup Tables of the top-level Data Set: of the Image
/// Pixel Module for PALETTE COLOR, or of the Palette Color Lookup Table
/// Module (C.7.9). In a Parametric Map, whose only Photometric
/// Interpretation is MONOCHROME2, they are the latter's, whose entries are
/// 16 bits in an Image IOD (C.7.6.3.1.5); a Segmentation's C.7.9 entries may
/// be 8 or 16 bits, and so may the Image Pixel Module's.
fn top_level_palette(
    object: &DefaultDicomObject,
    iod: &str,
    single_sample: bool,
    violations: &mut Vec<String>,
) {
    if COLORS
        .iter()
        .all(|(_, descriptor, _, _)| object.get(*descriptor).is_none())
    {
        return;
    }
    let label = "Palette Color Lookup Table Descriptors (0028,1101-1103)";
    if !single_sample {
        violations.push(format!(
            "{label} are present, but Samples per Pixel is not 1; Palette Color Lookup Tables are used only with a single sample per pixel (PS3.3 {EDITION} C.7.6.3.1.6 and C.7.9.2)"
        ));
    }
    let entry_bits = (iod == "A.75").then_some(16);
    palette_lookup_tables(object, &label, entry_bits, violations);
}

/// PS3.3 C.7.6.3.1.7: "Both integer values shall be greater than 0."
fn pixel_aspect_ratio(item: &InMemDicomObject, violations: &mut Vec<String>) {
    let Some(values) = item
        .get(tags::PIXEL_ASPECT_RATIO)
        .and_then(|element| element.to_multi_int::<i64>().ok())
    else {
        return;
    };
    if values.iter().any(|value| *value <= 0) {
        violations.push(format!(
            "Pixel Aspect Ratio (0028,0034) is {}; both integer Values shall be greater than 0 (PS3.3 {EDITION} C.7.6.3.1.7)",
            values
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join("\\")
        ));
    }
}

/// PS3.3 C.7.5.1.1.2: padding Values lie within the stored sample range,
/// and the Value is at the minimum end of the range for MONOCHROME2 and
/// PALETTE COLOR, at the maximum end for MONOCHROME1.
fn pixel_padding(object: &DefaultDicomObject, violations: &mut Vec<String>) {
    let value = integer_value(object, tags::PIXEL_PADDING_VALUE);
    let limit = integer_value(object, tags::PIXEL_PADDING_RANGE_LIMIT);
    if value.is_none() && limit.is_none() {
        return;
    }
    if let Some((minimum, maximum)) = stored_range(object) {
        for (name, padding) in [
            ("Pixel Padding Value (0028,0120)", value),
            ("Pixel Padding Range Limit (0028,0121)", limit),
        ] {
            if let Some(padding) = padding.filter(|padding| !(minimum..=maximum).contains(padding))
            {
                violations.push(format!(
                    "{name} {padding} lies outside the stored sample range {minimum}..={maximum} that Bits Stored and Pixel Representation define (PS3.3 {EDITION} C.7.5.1.1.2)"
                ));
            }
        }
    }
    let (Some(value), Some(limit)) = (value, limit) else {
        return;
    };
    let photometric = text(object, tags::PHOTOMETRIC_INTERPRETATION);
    let misordered = match photometric.as_deref() {
        Some("MONOCHROME2" | "PALETTE COLOR") => value > limit,
        Some("MONOCHROME1") => value < limit,
        _ => false,
    };
    if misordered {
        violations.push(format!(
            "Pixel Padding Value (0028,0120) {value} and Pixel Padding Range Limit (0028,0121) {limit} are in the wrong order for Photometric Interpretation {}; the Value shall be the end of the range closer to the {} pixel value (PS3.3 {EDITION} C.7.5.1.1.2)",
            photometric.unwrap_or_default(),
            if value > limit { "minimum" } else { "maximum" }
        ));
    }
}

/// Smallest and largest stored sample Values (PS3.5 8.1.1).
fn stored_range(item: &InMemDicomObject) -> Option<(i64, i64)> {
    let stored = integer(item, tags::BITS_STORED).filter(|bits| (1..=32).contains(bits))?;
    Some(match integer(item, tags::PIXEL_REPRESENTATION)? {
        0 => (0, (1i64 << stored) - 1),
        _ => (-(1i64 << (stored - 1)), (1i64 << (stored - 1)) - 1),
    })
}

/// PS3.3 C.7.6.1.1.6 restrictions on the Image Pixel Macro of an Icon
/// Image, with the C.7-11c sample format and PS3.5 8.1.1 Native length.
fn icon_image(icon: &InMemDicomObject, label: &dyn Display, violations: &mut Vec<String>) {
    const CITATION: &str = concat!("PS3.3 ", dicom_edition!(), " C.7.6.1.1.6");
    let samples = integer(icon, tags::SAMPLES_PER_PIXEL);
    if samples.is_some_and(|samples| samples != 1) {
        violations.push(format!(
            "{label}: Samples per Pixel is {}; Icon Images have a single sample per pixel ({CITATION})",
            samples.unwrap_or_default()
        ));
    }
    let photometric = text(icon, tags::PHOTOMETRIC_INTERPRETATION);
    if let Some(value) = photometric
        .as_deref()
        .filter(|value| !matches!(*value, "MONOCHROME1" | "MONOCHROME2" | "PALETTE COLOR"))
    {
        violations.push(format!(
            "{label}: Photometric Interpretation is {value}; Icon Images are MONOCHROME1, MONOCHROME2 or PALETTE COLOR ({CITATION})"
        ));
    }
    if icon.get(tags::PLANAR_CONFIGURATION).is_some() {
        violations.push(format!(
            "{label}: Planar Configuration (0028,0006) is present; it shall not be present in an Icon Image ({CITATION})"
        ));
    }
    let allocated = integer(icon, tags::BITS_ALLOCATED);
    let stored = integer(icon, tags::BITS_STORED);
    for (name, bits) in [("Bits Allocated", allocated), ("Bits Stored", stored)] {
        if let Some(bits) = bits.filter(|bits| !matches!(bits, 1 | 8)) {
            violations.push(format!(
                "{label}: {name} is {bits}; Icon Image pixel samples have 1 or 8 bits ({CITATION})"
            ));
        }
    }
    if let (Some(allocated), Some(stored)) = (allocated, stored) {
        if stored > allocated {
            violations.push(format!(
                "{label}: Bits Stored {stored} is larger than Bits Allocated {allocated} (PS3.5 {EDITION} 8.1.1)"
            ));
        }
    }
    let high_bit = integer(icon, tags::HIGH_BIT);
    if let (Some(high_bit), Some(stored)) = (high_bit, stored) {
        if high_bit.checked_add(1) != Some(stored) {
            violations.push(format!(
                "{label}: High Bit is {high_bit}; it shall be one less than Bits Stored {stored} ({CITATION})"
            ));
        }
    }
    if let Some(representation) =
        integer(icon, tags::PIXEL_REPRESENTATION).filter(|value| *value != 0)
    {
        violations.push(format!(
            "{label}: Pixel Representation is {representation}; Icon Images are unsigned (0000H) ({CITATION})"
        ));
    }
    if icon.get(tags::PIXEL_ASPECT_RATIO).is_some() {
        violations.push(format!(
            "{label}: Pixel Aspect Ratio (0028,0034) is present; Icon Image pixels are square and it shall not be present ({CITATION})"
        ));
    }
    if photometric.as_deref() == Some("PALETTE COLOR") {
        if let Some(allocated) = allocated.filter(|bits| *bits != 8) {
            violations.push(format!(
                "{label}: Bits Allocated is {allocated}; an Icon Image with a Palette Color Lookup Table has 8 ({CITATION})"
            ));
        }
        palette_lookup_tables(icon, label, None, violations);
    }
    icon_pixel_data(icon, label, violations);
}

/// The Icon Image's own Pixel Data: one Frame, Native or Encapsulated.
fn icon_pixel_data(icon: &InMemDicomObject, label: &dyn Display, violations: &mut Vec<String>) {
    let Some(element) = icon.get(tags::PIXEL_DATA) else {
        return;
    };
    match element.value() {
        Value::PixelSequence(sequence) => {
            if let Some((index, fragment)) = sequence
                .fragments()
                .iter()
                .enumerate()
                .find(|(_, fragment)| fragment.is_empty() || fragment.len() % 2 == 1)
            {
                violations.push(format!(
                    "{label}: Pixel Data Fragment {} has Item Length {}; Items containing an encoded Fragment shall be made of an even number of bytes greater or equal to two (PS3.5 {EDITION} A.4)",
                    index + 1,
                    fragment.len()
                ));
            } else if sequence.fragments().is_empty() {
                violations.push(format!(
                    "{label}: encapsulated Pixel Data has no Fragment; at least one Frame shall be present (PS3.5 {EDITION} 8.2)"
                ));
            }
        }
        Value::Primitive(_) => {
            let Ok(frame) = NativeFrame::of(icon) else {
                // Missing Rows, Columns, Samples or Bits are Type 1
                // presence violations, reported by the IOD checks.
                return;
            };
            let Ok(expected) = frame.bytes(1) else {
                return;
            };
            let actual = element.to_bytes().map_or(0, |bytes| bytes.len() as u64);
            if !is_padded_length(actual, expected) {
                violations.push(format!(
                    "{label}: Pixel Data (7FE0,0010) holds {actual} bytes; Native Format requires {expected} bytes for {}, padded only to the next even length (PS3.5 {EDITION} 8.1.1)",
                    frame.describe(1)
                ));
            }
        }
        Value::Sequence(_) => {}
    }
}

const COLORS: [(&str, Tag, Tag, Tag); 3] = [
    (
        "Red",
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ),
    (
        "Green",
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ),
    (
        "Blue",
        tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ),
];

/// PS3.3 C.7.6.3.1.5 descriptors and the tables they describe, plain
/// (C.7.6.3.1.6) or segmented (C.7.9.2). `entry_bits` is the third Value the
/// enclosing Module requires, if it narrows "8 or 16".
fn palette_lookup_tables(
    item: &InMemDicomObject,
    label: &dyn Display,
    entry_bits: Option<u16>,
    violations: &mut Vec<String>,
) {
    const CITATION: &str = concat!("PS3.3 ", dicom_edition!(), " C.7.6.3.1.5");
    let descriptors: Vec<(&str, [u16; 3], Tag, Tag)> = COLORS
        .iter()
        .filter_map(|(color, descriptor, data, segmented)| {
            descriptor_values(item, *descriptor).map(|values| (*color, values, *data, *segmented))
        })
        .collect();
    // An absent or malformed descriptor is a presence or VM violation the
    // IOD checks report; comparing the others would repeat it.
    if descriptors.len() != COLORS.len() {
        return;
    }
    for (position, name) in [(0, "first"), (1, "second"), (2, "third")] {
        if descriptors
            .iter()
            .any(|(_, values, _, _)| values[position] != descriptors[0].1[position])
        {
            violations.push(format!(
                "{label}: the {name} Values of the Red, Green and Blue Palette Color Lookup Table Descriptors differ ({}); they shall be identical ({CITATION})",
                descriptors
                    .iter()
                    .map(|(color, values, _, _)| format!("{color} {}", values[position]))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    for (color, [first, _, bits], data, segmented) in descriptors {
        let descriptor = format!("{color} Palette Color Lookup Table Descriptor");
        if !matches!(bits, 8 | 16) {
            violations.push(format!(
                "{label}: {descriptor} third Value is {bits}; it shall be 8 or 16 ({CITATION})"
            ));
            continue;
        }
        if let Some(required) = entry_bits.filter(|required| *required != bits) {
            violations.push(format!(
                "{label}: {descriptor} third Value is {bits}; a Palette Color Lookup Table Module in an Image IOD has {required}-bit entries ({CITATION})"
            ));
        }
        let entries = if first == 0 { 65_536 } else { u64::from(first) };
        if let Some(bytes) = binary(item, data) {
            let width = u64::from(bits / 8);
            let expected = entries * width;
            if bytes.len() as u64 == 65_536 * width && first != 0 {
                violations.push(format!(
                    "{label}: {color} Palette Color Lookup Table Data holds 65536 entries but the {descriptor} first Value is {first}; 2^16 entries are encoded as 0 ({CITATION})"
                ));
            } else if !is_padded_length(bytes.len() as u64, expected) {
                violations.push(format!(
                    "{label}: {color} Palette Color Lookup Table Data holds {} bytes; {entries} entries stored as {bits} bits allocated occupy {expected} bytes, padded only to the next even length ({CITATION})",
                    bytes.len()
                ));
            }
            continue;
        }
        let Some(bytes) = binary(item, segmented) else {
            continue;
        };
        match expanded_entries(&bytes) {
            Ok(65_536) if first != 0 => violations.push(format!(
                "{label}: Segmented {color} Palette Color Lookup Table Data expands to 65536 entries but the {descriptor} first Value is {first}; 2^16 entries are encoded as 0 ({CITATION})"
            )),
            Ok(expanded) if expanded != entries => violations.push(format!(
                "{label}: Segmented {color} Palette Color Lookup Table Data expands to {expanded} entries; the {descriptor} declares {entries} (PS3.3 {EDITION} C.7.9.2)"
            )),
            Ok(_) => {}
            Err(reason) => violations.push(format!(
                "{label}: Segmented {color} Palette Color Lookup Table Data {reason}"
            )),
        }
    }
}

/// One segment of Segmented Palette Color Lookup Table Data.
#[derive(Clone, Copy)]
struct Segment {
    /// Byte offset of the opcode from the start of the data.
    offset: u64,
    /// Entries a discrete or linear segment contributes; 0 for indirect.
    entries: u64,
    indirect: Option<Indirect>,
}

#[derive(Clone, Copy)]
struct Indirect {
    segments: u64,
    target: u64,
}

/// The number of entries the segments of PS3.3 C.7.9.2 expand to, or why
/// they do not parse. Bounded by the data: one pass records the segments,
/// a second resolves indirect segments through prefix sums.
fn expanded_entries(bytes: &[u8]) -> Result<u64, String> {
    if bytes.len() % 2 == 1 {
        return Err(format!(
            "has odd length {}; segments are 16-bit words (PS3.3 {EDITION} C.7.9.2)",
            bytes.len()
        ));
    }
    let words = bytes.len() / 2;
    let word = |index: usize| u16::from_le_bytes([bytes[2 * index], bytes[2 * index + 1]]);
    let mut segments: Vec<Segment> = Vec::new();
    let mut position = 0usize;
    while position < words {
        let offset = position as u64 * 2;
        let (size, entries, indirect) = match word(position) {
            0 => match (position + 1 < words).then(|| usize::from(word(position + 1))) {
                Some(entries) if position + 2 + entries <= words => {
                    (2 + entries, entries as u64, None)
                }
                _ => {
                    return Err(format!(
                        "has a discrete segment at byte {offset} whose Segment Length exceeds the remaining data; its format is opcode 0, Segment Length, then that many entries (PS3.3 {EDITION} C.7.9.2.1 Table C.7-25)"
                    ))
                }
            },
            1 => {
                if position + 3 > words {
                    return Err(format!(
                        "has a truncated linear segment at byte {offset}; its format is opcode 1, Segment Length, Y1 (PS3.3 {EDITION} C.7.9.2.2 Table C.7-26)"
                    ));
                }
                if segments.is_empty() {
                    return Err(
                        concat!("begins with a linear segment, which has no previous segment end point to continue (PS3.3 ", dicom_edition!(), " C.7.9.2.2)")
                            .to_string(),
                    );
                }
                (3, u64::from(word(position + 1)), None)
            }
            2 => {
                if position + 4 > words {
                    return Err(format!(
                        "has a truncated indirect segment at byte {offset}; its format is opcode 2, number of segments to copy, then the byte offset as least and most significant 16 bits (PS3.3 {EDITION} C.7.9.2.3 Table C.7-27)"
                    ));
                }
                let indirect = Indirect {
                    segments: u64::from(word(position + 1)),
                    target: u64::from(word(position + 2)) | u64::from(word(position + 3)) << 16,
                };
                (4, 0, Some(indirect))
            }
            reserved => {
                return Err(format!(
                    "has segment opcode {reserved} at byte {offset}; opcodes 3 and above are reserved (PS3.3 {EDITION} C.7.9.2 Table C.7-24)"
                ))
            }
        };
        segments.push(Segment {
            offset,
            entries,
            indirect,
        });
        position += size;
    }
    // Prefix sums of entries and of indirect segments let each copy be
    // resolved in constant time, without recursion.
    let mut entries_before = Vec::with_capacity(segments.len() + 1);
    let mut indirect_before = Vec::with_capacity(segments.len() + 1);
    entries_before.push(0u64);
    indirect_before.push(0usize);
    for segment in &segments {
        entries_before.push(entries_before.last().copied().unwrap_or(0) + segment.entries);
        indirect_before.push(
            indirect_before.last().copied().unwrap_or(0) + usize::from(segment.indirect.is_some()),
        );
    }
    let mut total = entries_before.last().copied().unwrap_or(0);
    for segment in &segments {
        let Some(indirect) = segment.indirect else {
            continue;
        };
        let Ok(first) = segments.binary_search_by_key(&indirect.target, |target| target.offset)
        else {
            return Err(format!(
                "has an indirect segment at byte {} pointing to byte offset {}, which does not begin a segment (PS3.3 {EDITION} C.7.9.2.3)",
                segment.offset, indirect.target
            ));
        };
        let end = usize::try_from(indirect.segments)
            .ok()
            .and_then(|count| first.checked_add(count))
            .filter(|end| *end <= segments.len());
        let Some(end) = end else {
            return Err(format!(
                "has an indirect segment at byte {} copying {} segments from byte offset {}, beyond the last segment (PS3.3 {EDITION} C.7.9.2.3)",
                segment.offset, indirect.segments, indirect.target
            ));
        };
        if indirect_before[end] != indirect_before[first] {
            return Err(format!(
                "has an indirect segment at byte {} that copies another indirect segment; an indirect segment shall not point to or copy another indirect segment (PS3.3 {EDITION} C.7.9.2.3)",
                segment.offset
            ));
        }
        total = total.saturating_add(entries_before[end] - entries_before[first]);
    }
    Ok(total)
}

/// A descriptor's three Values as unsigned 16-bit words: the first and
/// third are unsigned whether the VR is US or SS (PS3.5 A.4), and equality
/// of the second is the same on the encoded words.
fn descriptor_values(item: &InMemDicomObject, tag: Tag) -> Option<[u16; 3]> {
    let words: Vec<u16> = match item.get(tag)?.value() {
        Value::Primitive(PrimitiveValue::U16(values)) => values.to_vec(),
        Value::Primitive(PrimitiveValue::I16(values)) => {
            values.iter().map(|value| *value as u16).collect()
        }
        Value::Primitive(other) => other
            .to_multi_int::<i64>()
            .ok()?
            .into_iter()
            .map(|value| value as u16)
            .collect(),
        _ => return None,
    };
    words.try_into().ok()
}

fn binary(item: &InMemDicomObject, tag: Tag) -> Option<Vec<u8>> {
    item.get(tag)?
        .to_bytes()
        .ok()
        .map(std::borrow::Cow::into_owned)
}

fn integer(item: &InMemDicomObject, tag: Tag) -> Option<u64> {
    item.get(tag)?.to_int::<u64>().ok()
}

fn text(item: &InMemDicomObject, tag: Tag) -> Option<String> {
    let value = item.get(tag)?.to_str().ok()?;
    let value = value.trim_matches([' ', '\0']);
    (!value.is_empty()).then(|| value.to_string())
}
