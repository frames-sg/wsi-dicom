//! Files of one SOP Instance in the validated set. A SOP Instance UID names
//! one object (PS3.5 9.1), so two of its files are copies: byte-identical
//! copies agree, a receiver may coerce Attributes and keep the UID (PS3.4
//! B.4.1.3), and a different SOP Class or different Pixel Data under one UID
//! are different objects.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use std::cmp::Ordering;

use dicom_core::value::{PixelFragmentSequence, Value};
use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, InMemDicomObject};
use sha2::{Digest, Sha256};

use crate::validation::iod::element_label;
use crate::validation::pixel_structure::{
    dicom_frame_count, optional_u64_values, visit_encapsulated_frames,
};

/// Where an observed object came from, for comparing copies of one SOP
/// Instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::validation) enum Origin {
    /// A PS3.10 file at the observed path, read again, up to
    /// `max_input_bytes`, only to compare it with another copy.
    File { max_input_bytes: u64 },
    /// A Data Set without a file, such as one of a DICOM JSON document, at
    /// the observed label, which is never read: its digests are taken as it
    /// is observed.
    Label,
    /// A Data Set without a file whose SOP Instance UID no other Data Set
    /// of the set holds, so it is never compared and nothing is digested.
    UniqueLabel,
}

/// What one object contributes to the comparison of copies.
#[derive(Debug)]
pub(super) struct CopyFacts {
    path: PathBuf,
    origin: Origin,
    /// A Data Set without a file is compared by these digests, taken as it
    /// is observed; a file is read again only when another copy appears.
    content: Option<Content>,
}

impl CopyFacts {
    pub(super) fn of(path: &Path, object: &DefaultDicomObject, origin: Origin) -> Self {
        Self {
            path: path.to_path_buf(),
            origin,
            content: (origin == Origin::Label).then(|| Content::of(object, None)),
        }
    }
}

/// The first copy of one SOP Instance, compared with each later copy: a
/// file is read again and digested at most once.
pub(super) struct FirstCopy<'a> {
    facts: &'a CopyFacts,
    loaded: Option<Result<Content, String>>,
}

impl<'a> FirstCopy<'a> {
    pub(super) fn new(facts: &'a CopyFacts) -> Self {
        Self {
            facts,
            loaded: None,
        }
    }

    /// How `second`, a later copy, relates to this one. An error when a file
    /// can no longer be read.
    pub(super) fn compare(&mut self, second: &CopyFacts) -> Result<Copies, String> {
        if let (Origin::File { max_input_bytes }, Origin::File { .. }) =
            (self.facts.origin, second.origin)
        {
            if same_bytes(&self.facts.path, &second.path, max_input_bytes)? {
                return Ok(Copies::Identical);
            }
        }
        let origins = (self.facts.origin, second.origin);
        let right = load(second)?;
        let left = self.content()?;
        Ok(relate(left, right.as_ref(), origins))
    }

    fn content(&mut self) -> Result<&Content, String> {
        let facts = self.facts;
        if let Some(content) = &facts.content {
            return Ok(content);
        }
        self.loaded
            .get_or_insert_with(|| load(facts).map(|content| content.into_owned()))
            .as_ref()
            .map_err(Clone::clone)
    }
}

/// How two files of one SOP Instance UID relate.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Copies {
    /// The same bytes or, for Data Sets without files, the same Attributes.
    Identical,
    /// Files of the same Attributes and Pixel Data Frames that encode them
    /// differently: in their preamble, lengths, padding, offset tables or
    /// Fragments.
    Reencoded,
    /// Two objects under one UID, and why.
    Different(String),
    /// One object whose copies differ in these Attributes only.
    Coerced(Vec<String>),
}

/// The digests of an object's top-level Attributes, its Pixel Data apart.
#[derive(Clone, Debug)]
struct Content {
    sop_class: Option<String>,
    transfer_syntax: Option<String>,
    /// Each top-level Attribute but the pixel ones, and the File Meta
    /// Information of a file, in Tag order.
    attributes: Vec<(Tag, [u8; 32])>,
    /// The Frames of the Pixel Data, Float Pixel Data or Double Float Pixel
    /// Data Value of a file; a Data Set without one holds only a stand-in.
    pixels: Option<[u8; 32]>,
}

const PIXEL_TAGS: [Tag; 3] = [
    tags::PIXEL_DATA,
    tags::FLOAT_PIXEL_DATA,
    tags::DOUBLE_FLOAT_PIXEL_DATA,
];

impl Content {
    /// The digests of `object`; a file's, read with `max_input_bytes`,
    /// include its File Meta Information and Pixel Data.
    fn of(object: &DefaultDicomObject, max_input_bytes: Option<u64>) -> Self {
        let file = max_input_bytes.is_some();
        let root: &InMemDicomObject = object;
        let mut attributes: Vec<(Tag, [u8; 32])> = root
            .iter()
            .filter(|element| !PIXEL_TAGS.contains(&element.header().tag))
            .map(|element| (element.header().tag, element_digest(element)))
            .collect();
        let meta = object.meta();
        let transfer_syntax = file.then(|| trimmed(&meta.transfer_syntax));
        if file {
            for (tag, value) in [
                (tags::TRANSFER_SYNTAX_UID, meta.transfer_syntax.as_str()),
                (
                    tags::IMPLEMENTATION_CLASS_UID,
                    meta.implementation_class_uid.as_str(),
                ),
                (
                    tags::IMPLEMENTATION_VERSION_NAME,
                    meta.implementation_version_name.as_deref().unwrap_or(""),
                ),
                (
                    tags::SOURCE_APPLICATION_ENTITY_TITLE,
                    meta.source_application_entity_title
                        .as_deref()
                        .unwrap_or(""),
                ),
            ] {
                attributes.push((tag, Sha256::digest(trimmed(value).as_bytes()).into()));
            }
        }
        attributes.sort_by_key(|(tag, _)| *tag);
        let pixels = max_input_bytes.and_then(|max_input_bytes| {
            // No Frame exceeds the file it was read from.
            let max_frame_bytes = usize::try_from(max_input_bytes).unwrap_or(usize::MAX);
            PIXEL_TAGS
                .iter()
                .find_map(|tag| root.get(*tag))
                .map(|element| pixel_digest(object, element, max_frame_bytes))
        });
        Self {
            sop_class: root
                .get(tags::SOP_CLASS_UID)
                .and_then(|element| element.to_str().ok())
                .map(|value| trimmed(&value)),
            transfer_syntax,
            attributes,
            pixels,
        }
    }
}

fn trimmed(value: &str) -> String {
    value.trim_end_matches(['\0', ' ']).to_string()
}

/// A digest of an element's VR and Value, Items included.
fn element_digest(element: &InMemElement) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(format!("{:?}", element.header().vr).as_bytes());
    match element.value() {
        Value::Primitive(primitive) => hash.update(primitive.to_bytes()),
        Value::Sequence(sequence) => {
            for item in sequence.items() {
                hash.update(b"item");
                for nested in item.iter() {
                    hash.update(nested.header().tag.group().to_le_bytes());
                    hash.update(nested.header().tag.element().to_le_bytes());
                    hash.update(element_digest(nested));
                }
            }
        }
        // A nested pixel element, such as an Icon Image's, holds one Frame.
        Value::PixelSequence(sequence) => hash.update(fragments_digest(sequence)),
    }
    hash.finalize().into()
}

/// A digest of a pixel element's Value: each encoded Frame's payload, with
/// its length to keep the Frames apart, through the Fragment-to-Frame
/// mapping the pixel structure check validates, so neither the offset
/// tables nor how the Frames are split into Fragments count; or the Native
/// bytes. Fragments that map to no Frames are digested as one payload.
fn pixel_digest(
    object: &DefaultDicomObject,
    element: &InMemElement,
    max_frame_bytes: usize,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    match element.value() {
        Value::PixelSequence(sequence) => {
            if frames_digest(object, sequence, max_frame_bytes, &mut hash).is_err() {
                return fragments_digest(sequence);
            }
        }
        Value::Primitive(primitive) => {
            hash.update(b"native");
            hash.update(primitive.to_bytes());
        }
        Value::Sequence(_) => hash.update(b"sequence"),
    }
    hash.finalize().into()
}

fn frames_digest(
    object: &DefaultDicomObject,
    sequence: &PixelFragmentSequence<Vec<u8>>,
    max_frame_bytes: usize,
    hash: &mut Sha256,
) -> Result<(), String> {
    let frames = dicom_frame_count(object)?;
    let offsets = optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE)?;
    let lengths = optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE_LENGTHS)?;
    hash.update(b"frames");
    visit_encapsulated_frames(
        sequence,
        frames,
        offsets.as_deref(),
        lengths.as_deref(),
        max_frame_bytes,
        |frame| {
            let parts: Vec<&[u8]> = frame.parts().collect();
            let len: usize = parts.iter().map(|part| part.len()).sum();
            hash.update((len as u64).to_le_bytes());
            for part in parts {
                hash.update(part);
            }
        },
    )
}

/// A digest of the Fragments' payloads as one byte string.
fn fragments_digest(sequence: &PixelFragmentSequence<Vec<u8>>) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"fragments");
    for fragment in sequence.fragments() {
        hash.update(fragment);
    }
    hash.finalize().into()
}

/// How two copies of one SOP Instance relate, from their digests and where
/// each came from.
fn relate(left: &Content, right: &Content, origins: (Origin, Origin)) -> Copies {
    if left.sop_class != right.sop_class {
        return Copies::Different(format!(
            "they are of SOP Classes {} and {}",
            left.sop_class.as_deref().unwrap_or("absent"),
            right.sop_class.as_deref().unwrap_or("absent")
        ));
    }
    let same_syntax = left.transfer_syntax == right.transfer_syntax;
    if same_syntax && left.pixels.is_some() && left.pixels != right.pixels {
        return Copies::Different(
            "their Pixel Data differs in the same Transfer Syntax".to_string(),
        );
    }
    let differing = differing_attributes(left, right);
    if differing.is_empty() {
        return match origins {
            (Origin::Label, Origin::Label) => Copies::Identical,
            _ => Copies::Reencoded,
        };
    }
    let mut names: Vec<String> = differing
        .into_iter()
        .map(|tag| element_label("", tag))
        .collect();
    if !same_syntax && left.pixels.is_some() {
        names.push("Pixel Data, not compared across Transfer Syntaxes".to_string());
    }
    Copies::Coerced(names)
}

/// The Tags whose Attributes the two copies do not share, in Tag order: one
/// pass over both Tag-ordered digest lists.
fn differing_attributes(left: &Content, right: &Content) -> Vec<Tag> {
    let mut tags = Vec::new();
    let (mut left, mut right) = (
        left.attributes.iter().peekable(),
        right.attributes.iter().peekable(),
    );
    loop {
        let order = match (left.peek(), right.peek()) {
            (Some((left_tag, _)), Some((right_tag, _))) => left_tag.cmp(right_tag),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => return tags,
        };
        match order {
            Ordering::Less => tags.extend(left.next().map(|(tag, _)| *tag)),
            Ordering::Greater => tags.extend(right.next().map(|(tag, _)| *tag)),
            Ordering::Equal => {
                if let (Some((tag, left_digest)), Some((_, right_digest))) =
                    (left.next(), right.next())
                {
                    if left_digest != right_digest {
                        tags.push(*tag);
                    }
                }
            }
        }
    }
}

/// The digests of one copy: those taken as a Data Set was observed, or a
/// file's, read again.
fn load(facts: &CopyFacts) -> Result<std::borrow::Cow<'_, Content>, String> {
    match (&facts.content, facts.origin) {
        (Some(content), _) => Ok(std::borrow::Cow::Borrowed(content)),
        (None, Origin::File { max_input_bytes }) => {
            let object = read(&facts.path, max_input_bytes)?;
            Ok(std::borrow::Cow::Owned(Content::of(
                &object,
                Some(max_input_bytes),
            )))
        }
        (None, Origin::Label | Origin::UniqueLabel) => Err(format!(
            "{} has no recorded content to compare",
            facts.path.display()
        )),
    }
}

fn read(path: &Path, max_input_bytes: u64) -> Result<DefaultDicomObject, String> {
    let file =
        File::open(path).map_err(|err| format!("cannot reopen {}: {err}", path.display()))?;
    dicom_object::OpenFileOptions::new()
        .read_preamble(dicom_object::file::ReadPreamble::Auto)
        .from_reader(file.take(max_input_bytes))
        .map_err(|err| format!("cannot read {} again: {err}", path.display()))
}

/// Whether two files hold the same bytes, compared in chunks up to
/// `max_input_bytes` each.
fn same_bytes(left: &Path, right: &Path, max_input_bytes: u64) -> Result<bool, String> {
    let open = |path: &Path| {
        File::open(path)
            .map(|file| BufReader::new(file.take(max_input_bytes.saturating_add(1))))
            .map_err(|err| format!("cannot reopen {}: {err}", path.display()))
    };
    let (mut left_reader, mut right_reader) = (open(left)?, open(right)?);
    let mut left_chunk = vec![0u8; 64 * 1024];
    let mut right_chunk = vec![0u8; 64 * 1024];
    loop {
        let read = |reader: &mut BufReader<_>, chunk: &mut [u8], path: &Path| {
            fill(reader, chunk).map_err(|err| format!("cannot read {}: {err}", path.display()))
        };
        let left_read = read(&mut left_reader, &mut left_chunk, left)?;
        let right_read = read(&mut right_reader, &mut right_chunk, right)?;
        if left_read != right_read || left_chunk[..left_read] != right_chunk[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

/// Reads until `chunk` is full or the input ends; the bytes read.
fn fill(reader: &mut impl Read, chunk: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < chunk.len() {
        match reader.read(&mut chunk[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use dicom_core::value::PixelFragmentSequence;
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_object::meta::FileMetaTableBuilder;

    use super::*;

    const ORIGIN: Origin = Origin::File {
        max_input_bytes: 1 << 20,
    };

    /// A file of one SOP Instance whose RLE Pixel Data has `offsets` as its
    /// Basic Offset Table and `fragments` as its Fragments.
    fn encapsulated(
        dir: &Path,
        name: &str,
        frames: u32,
        offsets: Vec<u32>,
        fragments: Vec<Vec<u8>>,
    ) -> CopyFacts {
        let mut object = InMemDicomObject::new_empty();
        for (tag, vr, value) in [
            (tags::SOP_CLASS_UID, VR::UI, "1.2.840.10008.5.1.4.1.1.66.4"),
            (
                tags::SOP_INSTANCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.71",
            ),
            (tags::NUMBER_OF_FRAMES, VR::IS, &frames.to_string()),
        ] {
            object.put(DataElement::new(tag, vr, PrimitiveValue::from(value)));
        }
        object.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PixelFragmentSequence::new(offsets, fragments),
        ));
        let object = object
            .with_meta(
                FileMetaTableBuilder::new()
                    .media_storage_sop_class_uid("1.2.840.10008.5.1.4.1.1.66.4")
                    .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.71")
                    .transfer_syntax(dicom_dictionary_std::uids::RLE_LOSSLESS),
            )
            .unwrap();
        let path = dir.join(name);
        object.write_to_file(&path).unwrap();
        CopyFacts::of(&path, &object, ORIGIN)
    }

    fn relation(first: &CopyFacts, second: &CopyFacts) -> Copies {
        FirstCopy::new(first).compare(second).unwrap()
    }

    #[test]
    fn an_empty_basic_offset_table_and_one_of_offset_zero_hold_the_same_frame() {
        let tmp = tempfile::tempdir().unwrap();
        let frame = vec![1u8, 2, 3, 4];
        let empty = encapsulated(tmp.path(), "empty", 1, Vec::new(), vec![frame.clone()]);
        let zero = encapsulated(tmp.path(), "zero", 1, vec![0], vec![frame]);
        assert_eq!(relation(&empty, &zero), Copies::Reencoded);
    }

    #[test]
    fn frames_split_differently_into_fragments_are_the_same_pixel_data() {
        let tmp = tempfile::tempdir().unwrap();
        let (first, second) = (vec![1u8, 2, 3, 4], vec![5u8, 6]);
        // One Fragment per Frame, mapped without an offset table.
        let whole = encapsulated(
            tmp.path(),
            "whole",
            2,
            Vec::new(),
            vec![first.clone(), second.clone()],
        );
        // Frame 1 in two Fragments, located by the Basic Offset Table.
        let split = encapsulated(
            tmp.path(),
            "split",
            2,
            vec![0, 20],
            vec![first[..2].to_vec(), first[2..].to_vec(), second],
        );
        assert_eq!(relation(&whole, &split), Copies::Reencoded);
    }

    #[test]
    fn different_frames_or_frame_boundaries_are_different_pixel_data() {
        let tmp = tempfile::tempdir().unwrap();
        let original = encapsulated(
            tmp.path(),
            "original",
            2,
            Vec::new(),
            vec![vec![1, 2, 3, 4], vec![5, 6]],
        );
        let changed = encapsulated(
            tmp.path(),
            "changed",
            2,
            Vec::new(),
            vec![vec![1, 2, 3, 4], vec![5, 7]],
        );
        // The same bytes, with the boundary between the Frames moved.
        let moved = encapsulated(
            tmp.path(),
            "moved",
            2,
            Vec::new(),
            vec![vec![1, 2], vec![3, 4, 5, 6]],
        );
        let different =
            Copies::Different("their Pixel Data differs in the same Transfer Syntax".to_string());
        assert_eq!(relation(&original, &changed), different);
        assert_eq!(relation(&original, &moved), different);
    }

    #[test]
    fn the_first_copy_is_read_once_for_every_later_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let frame = vec![1u8, 2];
        let first = encapsulated(tmp.path(), "first", 1, Vec::new(), vec![frame.clone()]);
        let second = encapsulated(tmp.path(), "second", 1, vec![0], vec![frame.clone()]);
        let third = encapsulated(tmp.path(), "third", 1, vec![0], vec![frame]);
        let mut original = FirstCopy::new(&first);
        assert_eq!(original.compare(&second).unwrap(), Copies::Reencoded);
        // The first file no longer parses, but its digests are kept.
        std::fs::write(tmp.path().join("first"), b"not DICOM").unwrap();
        assert_eq!(original.compare(&third).unwrap(), Copies::Reencoded);
    }

    #[test]
    fn differing_attributes_are_merged_in_tag_order() {
        let content = |attributes: &[(u16, u8)]| Content {
            sop_class: None,
            transfer_syntax: None,
            attributes: attributes
                .iter()
                .map(|(element, digest)| (Tag(0x0010, *element), [*digest; 32]))
                .collect(),
            pixels: None,
        };
        let left = content(&[(0x10, 1), (0x20, 1), (0x30, 1), (0x40, 1)]);
        let right = content(&[(0x10, 1), (0x20, 2), (0x35, 1), (0x40, 1), (0x50, 1)]);
        assert_eq!(
            differing_attributes(&left, &right),
            [0x20, 0x30, 0x35, 0x50].map(|element| Tag(0x0010, element))
        );
    }
}
