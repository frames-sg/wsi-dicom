//! Segmentation constraints that the generated IOD tables cannot express
//! (PS3.3 A.51, C.8.20.2, C.8.20.3 and Table C.8.20-4): Image Type, Segment
//! Numbers and the Segments they identify, the coordinate system of the
//! Segments (A.51.5.1), the packing and values of the Pixel Data, the Palette
//! Color Lookup Table, and the patient image plane. Whether a Segmentation
//! matches the images it references is checked with the set.

use std::collections::BTreeSet;
use std::path::Path;

use dicom_core::value::Value;
use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};
use flate2::{Decompress, FlushDecompress, Status};

use super::image_semantics::{
    color_space, integer, multiplicity, palette, patient_planes, string, strings, Findings,
    FunctionalGroups, Iod, CITATION, PS3_5, SEGMENTATION_RULE,
};
use super::iod::{dicom_edition, items};
use super::pixel_structure::{dicom_frame_count, optional_u64_values, visit_encapsulated_frames};
use super::ValidationCheck;

/// The Segmentation check of one object; `held` is the Frame size limit
/// when the source holds the Pixel Data Values, `None` when it holds
/// stand-ins.
pub(super) fn run_segmentation_check(
    path: &Path,
    object: &DefaultDicomObject,
    held: Option<usize>,
) -> Option<ValidationCheck> {
    if Iod::of(object) != Some(Iod::Segmentation) {
        return None;
    }
    let root: &InMemDicomObject = object;
    let mut findings = Findings::default();
    let groups = FunctionalGroups::of(root);
    let kind = string(root, tags::SEGMENTATION_TYPE);
    image_type(root, &mut findings);
    let numbers = segment_numbers(root, kind.as_deref(), &mut findings);
    if kind.as_deref() == Some("LABELMAP") {
        if let Some(overlap) = string(root, tags::SEGMENTS_OVERLAP).filter(|value| value != "NO") {
            findings.push(|| format!(
                "Segments Overlap (0062,0013) is {overlap}; if present, it shall be NO when Segmentation Type is LABELMAP ({CITATION} Table C.8.20-2)"
            ));
        }
    }
    referenced_segments(&groups, &numbers, &mut findings);
    coordinate_system(root, &groups, &mut findings);
    match held {
        Some(max_frame_bytes) => {
            if kind.as_deref() == Some("BINARY") {
                packing(object, &mut findings);
            }
            pixel_values(object, kind.as_deref(), &numbers, max_frame_bytes, &mut findings);
        }
        None => findings.unexamined(format!(
            "the packing and values of Pixel Data (7FE0,0010), which the source does not hold ({CITATION} C.8.20.2.1, C.8.20.2.3.3 and Table C.8.20-2)"
        )),
    }
    palette(root, true, &mut findings);
    color_space(root, "Table C.11.15-1", &mut findings);
    patient_planes(root, &groups, &mut findings);
    Some(findings.check(
        path,
        SEGMENTATION_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " A.51 and C.8.20"),
    ))
}

/// Table C.8.20-2: Image Type is DERIVED\PRIMARY and no other Values.
fn image_type(root: &InMemDicomObject, findings: &mut Findings) {
    if let Some(values) = strings(root, tags::IMAGE_TYPE).filter(|values| values.len() > 2) {
        findings.push(|| format!(
            "Image Type (0008,0008) has {} Values {values:?}; Value 1 is DERIVED, Value 2 PRIMARY, and no other Values shall be present ({CITATION} Table C.8.20-2)",
            values.len()
        ));
    }
}

/// C.8.20.2.4 and Table C.8.20-4: Segment Numbers are unique, and for
/// BINARY and FRACTIONAL Segmentations they are 1 to the number of Segments
/// (in any Item order).
fn segment_numbers(
    root: &InMemDicomObject,
    kind: Option<&str>,
    findings: &mut Findings,
) -> BTreeSet<i64> {
    let mut numbers = BTreeSet::new();
    for segment in items(root, tags::SEGMENT_SEQUENCE) {
        let Some(number) = integer(segment, tags::SEGMENT_NUMBER) else {
            continue;
        };
        if !numbers.insert(number) {
            findings.push(|| format!(
                "Segment Number (0062,0004) {number} identifies more than one Item of Segment Sequence (0062,0002); it shall be unique within the Segmentation Instance ({CITATION} C.8.20.2.4 and Table C.8.20-4)"
            ));
        }
    }
    if matches!(kind, Some("BINARY" | "FRACTIONAL")) {
        let count = i64::try_from(numbers.len()).unwrap_or(i64::MAX);
        if numbers.iter().copied().ne(1..=count) {
            findings.push(|| format!(
                "Segment Numbers (0062,0004) are {numbers:?}; for Segmentation Type {} they shall start at 1 and increase monotonically by 1 ({CITATION} C.8.20.2.4)",
                kind.unwrap_or_default()
            ));
        }
    }
    numbers
}

/// Table C.8.20-3: each Frame's Referenced Segment Number is one Value that
/// identifies a Segment of Segment Sequence.
fn referenced_segments(
    groups: &FunctionalGroups<'_>,
    numbers: &BTreeSet<i64>,
    findings: &mut Findings,
) {
    for (path, identification) in groups.macro_items(tags::SEGMENT_IDENTIFICATION_SEQUENCE) {
        let count = multiplicity(identification, tags::REFERENCED_SEGMENT_NUMBER).unwrap_or(0);
        if count > 1 {
            findings.push(|| format!(
                "{path}Referenced Segment Number (0062,000B) has {count} Values; it shall not be multi-valued ({CITATION} Table C.8.20-3)"
            ));
            continue;
        }
        if let Some(number) = integer(identification, tags::REFERENCED_SEGMENT_NUMBER) {
            if !numbers.contains(&number) {
                findings.push(|| format!(
                    "{path}Referenced Segment Number (0062,000B) {number} identifies no Segment Number (0062,0004) of Segment Sequence (0062,0002), by reference to which it identifies the Segment ({CITATION} Table C.8.20-3)"
                ));
            }
        }
    }
}

/// A.51.5.1: with a Frame of Reference UID the Segments are located in it by
/// Pixel Measures and either the patient plane or the slide position of each
/// Frame (positions of TILED_FULL Frames follow from the Total Pixel Matrix);
/// without one, each Frame is derived from a referenced image by the
/// Derivation Image Functional Group.
fn coordinate_system(
    root: &InMemDicomObject,
    groups: &FunctionalGroups<'_>,
    findings: &mut Findings,
) {
    let tiled_full =
        string(root, tags::DIMENSION_ORGANIZATION_TYPE).as_deref() == Some("TILED_FULL");
    let mut unlocated = Lacking::default();
    let mut underived = Lacking::default();
    let referenced = string(root, tags::FRAME_OF_REFERENCE_UID).is_some();
    for frame in 0..groups.frames() {
        let has = |tag| groups.macro_item(frame, tag).is_some();
        if referenced {
            let patient =
                has(tags::PLANE_POSITION_SEQUENCE) && has(tags::PLANE_ORIENTATION_SEQUENCE);
            let slide = tiled_full || has(tags::PLANE_POSITION_SLIDE_SEQUENCE);
            if !has(tags::PIXEL_MEASURES_SEQUENCE) || !(patient || slide) {
                unlocated.add(frame + 1);
            }
        } else {
            let derived = groups
                .macro_item(frame, tags::DERIVATION_IMAGE_SEQUENCE)
                .is_some_and(|derivation| {
                    !items(derivation, tags::SOURCE_IMAGE_SEQUENCE).is_empty()
                });
            if !derived {
                underived.add(frame + 1);
            }
        }
    }
    if let Some(first) = unlocated.first {
        findings.push(|| format!(
            "{} Frame(s), Frame {first} the first, lack Pixel Measures with either Plane Position (Patient) and Plane Orientation (Patient) or Plane Position (Slide); with a Frame of Reference UID the Segment shall be specified within that coordinate system using them ({CITATION} A.51.5.1)",
            unlocated.count
        ));
    }
    if let Some(first) = underived.first {
        findings.push(|| format!(
            "{} Frame(s), Frame {first} the first, have no Derivation Image Functional Group with a Source Image; without a Frame of Reference UID each pixel shall correspond to a pixel of a referenced image using it ({CITATION} A.51.5.1)",
            underived.count
        ));
    }
}

/// The Frames that lack something, counted, with the first (1-based).
#[derive(Default)]
struct Lacking {
    count: usize,
    first: Option<usize>,
}

impl Lacking {
    fn add(&mut self, frame: usize) {
        self.count += 1;
        self.first.get_or_insert(frame);
    }
}

/// C.8.20.2.1: BINARY pixels are packed 8 to a byte, the Frames concatenated
/// without padding between them (PS3.5 8.1.1), so a Native Pixel Data Value
/// holds the bits of every Frame padded once to an even length.
fn packing(object: &DefaultDicomObject, findings: &mut Findings) {
    if super::image_semantics::encapsulated(object) {
        return;
    }
    let Some(element) = object.get(tags::PIXEL_DATA) else {
        return;
    };
    let Value::Primitive(primitive) = element.value() else {
        return;
    };
    let length = u64::try_from(primitive.calculate_byte_len()).unwrap_or(u64::MAX);
    let (Some(rows), Some(columns), Ok(frames)) = (
        integer(object, tags::ROWS).and_then(|value| u64::try_from(value).ok()),
        integer(object, tags::COLUMNS).and_then(|value| u64::try_from(value).ok()),
        dicom_frame_count(object),
    ) else {
        return;
    };
    let pixels = rows.saturating_mul(columns);
    let bytes = pixels.saturating_mul(frames as u64).div_ceil(8);
    let padded = bytes + bytes % 2;
    if length != padded {
        let per_frame = pixels.div_ceil(8).saturating_mul(frames as u64);
        let reason = if frames > 1 && length == per_frame + per_frame % 2 {
            ", as if each Frame were padded to a whole byte"
        } else {
            ""
        };
        findings.push(|| format!(
            "Pixel Data (7FE0,0010) has {length} bytes for {frames} Frame(s) of {rows} x {columns} single-bit pixels{reason}; packed 8 to a byte they occupy {bytes} bytes, padded to {padded} ({CITATION} C.8.20.2.1 and {PS3_5} 8.1.1)"
        ));
    }
}

/// Table C.8.20-2: no FRACTIONAL value exceeds Maximum Fractional Value;
/// C.8.20.2.3.3: every LABELMAP value is a Segment Number of an Item of
/// Segment Sequence.
fn pixel_values(
    object: &DefaultDicomObject,
    kind: Option<&str>,
    numbers: &BTreeSet<i64>,
    max_frame_bytes: usize,
    findings: &mut Findings,
) {
    let bits = integer(object, tags::BITS_STORED).unwrap_or(0);
    let (limit, described) = match kind {
        Some("FRACTIONAL") => {
            let Some(maximum) = integer(object, tags::MAXIMUM_FRACTIONAL_VALUE) else {
                return;
            };
            // Stored values cannot exceed the largest that Bits Stored holds.
            if !(1..=62).contains(&bits) || maximum >= (1_i64 << bits) - 1 {
                return;
            }
            (Some(maximum), None)
        }
        Some("LABELMAP") => (None, Some(numbers)),
        _ => return,
    };
    let mut over = Tally::default();
    let mut undescribed = BTreeSet::new();
    let scanned = scan_samples(object, max_frame_bytes, |value| {
        let value = i64::from(value);
        if limit.is_some_and(|limit| value > limit) {
            over.add(value);
        }
        if described.is_some_and(|described| !described.contains(&value)) && undescribed.len() < 16
        {
            undescribed.insert(value);
        }
    });
    if let Err(reason) = scanned {
        let what = if limit.is_some() {
            "Pixel Data values against Maximum Fractional Value (0062,000E) (Table C.8.20-2)"
        } else {
            "LABELMAP Pixel Data values against the Segment Numbers (C.8.20.2.3.3)"
        };
        findings.unexamined(format!("{what}: {reason}"));
        return;
    }
    if let (Some(limit), Some(largest)) = (limit, over.largest) {
        findings.push(|| format!(
            "{} Pixel Data (7FE0,0010) values exceed Maximum Fractional Value (0062,000E) {limit}, the largest {largest}; there shall be no values in Pixel Data greater than it ({CITATION} Table C.8.20-2)",
            over.count
        ));
    }
    if !undescribed.is_empty() {
        findings.push(|| format!(
            "Pixel Data (7FE0,0010) holds the values {undescribed:?}, which no Item of Segment Sequence (0062,0002) describes; every pixel value of a LABELMAP Segmentation is described by the Item with that Segment Number ({CITATION} C.8.20.2.3.3)"
        ));
    }
}

#[derive(Default)]
struct Tally {
    count: u64,
    largest: Option<i64>,
}

impl Tally {
    fn add(&mut self, value: i64) {
        self.count += 1;
        self.largest = Some(self.largest.map_or(value, |largest| largest.max(value)));
    }
}

/// Calls `visit` with every stored sample of a single-sample 8 or 16 bit
/// image: Native Pixel Data, or Frames that are uncompressed, Deflated, RLE
/// or JPEG 2000 coded. Any other encoding, or a Frame beyond
/// `max_frame_bytes`, is the reason it cannot.
fn scan_samples(
    object: &DefaultDicomObject,
    max_frame_bytes: usize,
    mut visit: impl FnMut(u16),
) -> Result<(), String> {
    let geometry = Geometry::of(object, max_frame_bytes)?;
    let element = object
        .get(tags::PIXEL_DATA)
        .ok_or("Pixel Data (7FE0,0010) is absent")?;
    match element.value() {
        Value::Primitive(_) => {
            let bytes = element
                .to_bytes()
                .map_err(|err| format!("Pixel Data is not readable as bytes: {err}"))?;
            let needed = geometry.frame_bytes.saturating_mul(geometry.frames);
            let native = bytes.get(..needed).ok_or_else(|| {
                format!(
                    "Pixel Data holds {} bytes, fewer than its {} Frame(s) need",
                    bytes.len(),
                    geometry.frames
                )
            })?;
            samples(native, geometry.bytes_per_sample, &mut visit);
            Ok(())
        }
        Value::PixelSequence(sequence) => {
            let syntax = object
                .meta()
                .transfer_syntax()
                .trim_end_matches(['\0', ' ']);
            let offsets = optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE)?;
            let lengths = optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE_LENGTHS)?;
            let mut failure = None;
            visit_encapsulated_frames(
                sequence,
                geometry.frames,
                offsets.as_deref(),
                lengths.as_deref(),
                max_frame_bytes,
                |frame| {
                    if failure.is_some() {
                        return;
                    }
                    let decoded = frame
                        .bytes()
                        .and_then(|bytes| decode_frame(syntax, &bytes, &geometry));
                    match decoded {
                        Ok(native) => samples(&native, geometry.bytes_per_sample, &mut visit),
                        Err(reason) => {
                            failure = Some(format!("Frame {}: {reason}", frame.index + 1))
                        }
                    }
                },
            )?;
            failure.map_or(Ok(()), Err)
        }
        Value::Sequence(_) => Err("Pixel Data (7FE0,0010) is a Sequence".to_string()),
    }
}

/// The Native layout of one single-sample Frame.
struct Geometry {
    rows: usize,
    columns: usize,
    frames: usize,
    bytes_per_sample: usize,
    frame_bytes: usize,
}

impl Geometry {
    fn of(object: &DefaultDicomObject, max_frame_bytes: usize) -> Result<Self, String> {
        let dimension = |tag, name: &str| {
            integer(object, tag)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| format!("{name} is absent or invalid"))
        };
        let rows = dimension(tags::ROWS, "Rows")?;
        let columns = dimension(tags::COLUMNS, "Columns")?;
        let bytes_per_sample = match integer(object, tags::BITS_ALLOCATED) {
            Some(8) => 1,
            Some(16) => 2,
            other => return Err(format!("Bits Allocated {other:?} is neither 8 nor 16")),
        };
        if integer(object, tags::SAMPLES_PER_PIXEL).is_some_and(|samples| samples != 1) {
            return Err("Samples per Pixel is not 1".to_string());
        }
        let frame_bytes = rows
            .checked_mul(columns)
            .and_then(|pixels| pixels.checked_mul(bytes_per_sample))
            .filter(|bytes| *bytes <= max_frame_bytes)
            .ok_or_else(|| {
                format!("a {rows} x {columns} Frame exceeds the {max_frame_bytes}-byte Frame limit")
            })?;
        Ok(Self {
            rows,
            columns,
            frames: dicom_frame_count(object)?,
            bytes_per_sample,
            frame_bytes,
        })
    }
}

fn samples(bytes: &[u8], bytes_per_sample: usize, visit: &mut impl FnMut(u16)) {
    if bytes_per_sample == 1 {
        bytes.iter().for_each(|byte| visit(u16::from(*byte)));
    } else {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .for_each(|pair| visit(u16::from_le_bytes(*pair)));
    }
}

/// One Frame in its Native layout.
fn decode_frame(syntax: &str, bytes: &[u8], geometry: &Geometry) -> Result<Vec<u8>, String> {
    match syntax {
        uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN => bytes
            .get(..geometry.frame_bytes)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| "the Fragment is shorter than its Native Frame".to_string()),
        uids::DEFLATED_IMAGE_FRAME_COMPRESSION => inflate(bytes, geometry.frame_bytes),
        uids::RLE_LOSSLESS => rle(bytes, geometry),
        uids::JPEG2000_LOSSLESS
        | uids::JPEG2000
        | uids::HTJ2K_LOSSLESS
        | uids::HTJ2K_LOSSLESS_RPCL
        | uids::HTJ2K => jpeg_2000(bytes, geometry),
        other => Err(format!(
            "this validator decodes no Frames of Transfer Syntax {other}"
        )),
    }
}

fn inflate(bytes: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let mut inflater = Decompress::new(false);
    // One byte more than the Frame shows a stream that inflates too far.
    let mut output = vec![0u8; expected + 1];
    match inflater.decompress(bytes, &mut output, FlushDecompress::Finish) {
        Ok(Status::StreamEnd) if inflater.total_out() >= expected as u64 => {
            output.truncate(expected);
            Ok(output)
        }
        Ok(_) => Err("its Deflate bit stream does not inflate to its Native Frame".to_string()),
        Err(err) => Err(format!("its Deflate bit stream is malformed: {err}")),
    }
}

/// PS3.5 Annex G: an RLE Frame of one segment per byte of each sample, most
/// significant first.
fn rle(bytes: &[u8], geometry: &Geometry) -> Result<Vec<u8>, String> {
    let word = |index: usize| {
        bytes
            .get(index * 4..index * 4 + 4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]) as usize)
    };
    let count = word(0).ok_or("the RLE Header is truncated")?;
    if count != geometry.bytes_per_sample {
        return Err(format!(
            "the RLE Header declares {count} segments for {} byte(s) per sample",
            geometry.bytes_per_sample
        ));
    }
    let pixels = geometry.rows * geometry.columns;
    let mut native = vec![0u8; geometry.frame_bytes];
    for segment in 0..count {
        let start = word(segment + 1).ok_or("the RLE Header is truncated")?;
        let end = if segment + 1 < count {
            word(segment + 2).ok_or("the RLE Header is truncated")?
        } else {
            bytes.len()
        };
        let data = bytes
            .get(start..end)
            .ok_or("an RLE segment offset lies outside the Frame")?;
        let decoded = unpack_bits(data, pixels)?;
        // Segment 1 holds the most significant byte.
        let byte = count - 1 - segment;
        for (pixel, value) in decoded.into_iter().enumerate() {
            native[pixel * count + byte] = value;
        }
    }
    Ok(native)
}

fn unpack_bits(data: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let mut output = Vec::with_capacity(expected);
    let mut position = 0;
    while output.len() < expected {
        let header = *data
            .get(position)
            .ok_or("an RLE segment ends before its pixels")? as i8;
        position += 1;
        match header {
            0..=127 => {
                let count = header as usize + 1;
                let literal = data
                    .get(position..position + count)
                    .ok_or("an RLE literal run is truncated")?;
                output.extend_from_slice(literal);
                position += count;
            }
            -127..=-1 => {
                let value = *data
                    .get(position)
                    .ok_or("an RLE replicate run is truncated")?;
                output.extend(std::iter::repeat_n(
                    value,
                    1 + header.unsigned_abs() as usize,
                ));
                position += 1;
            }
            -128 => {}
        }
    }
    output.truncate(expected);
    Ok(output)
}

fn jpeg_2000(bytes: &[u8], geometry: &Geometry) -> Result<Vec<u8>, String> {
    let format = if geometry.bytes_per_sample == 1 {
        j2k::PixelFormat::Gray8
    } else {
        j2k::PixelFormat::Gray16
    };
    let mut decoder = j2k::J2kDecoder::new(bytes)
        .map_err(|err| format!("its JPEG 2000 codestream is not decodable: {err}"))?;
    let mut native = vec![0u8; geometry.frame_bytes];
    decoder
        .decode_into(
            &mut native,
            geometry.columns * geometry.bytes_per_sample,
            format,
        )
        .map_err(|err| format!("its JPEG 2000 codestream is not decodable: {err}"))?;
    Ok(native)
}
