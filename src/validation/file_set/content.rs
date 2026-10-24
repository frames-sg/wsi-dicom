//! The Content Sequence (0040,A730) key of SR DOCUMENT and KEY OBJECT DOC
//! records (PS3.3 Tables F.5-25 and F.5-26): all, and only, the Content
//! Items with a HAS CONCEPT MOD relationship from the referenced document's
//! root Content Item; and the Content Items of that Sequence, held to the
//! PS3.3 C.17 and C.18 rules that bind a Content Item whatever IOD holds it.
//!
//! Each record is read here as dicom-object reads a Data Set, once the
//! DICOMDIR's framing has passed the bounded parse of `records`, and under
//! the character set that parse decodes the record with, so the comparison
//! sees the same Data Sets either side.

use std::borrow::Cow;
use std::io::Cursor;
use std::path::Path;

use dicom_core::value::Value as ElementValue;
use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_encoding::decode::basic::LittleEndianBasicDecoder;
use dicom_encoding::decode::explicit_le::ExplicitVRLittleEndianDecoder;
use dicom_object::{DefaultDicomObject, InMemDicomObject};
use dicom_parser::stateful::decode::StatefulDecoder;

use super::records::Record;
use crate::validation::iod::{Family, MAX_NESTING};
use crate::validation::structured_report::content_item_violations;

const HAS_CONCEPT_MOD: &str = "HAS CONCEPT MOD";

/// Each of `records`, the Directory Records the bounded parse found in
/// `bytes`, as dicom-object reads its Item's Data Set in Explicit VR Little
/// Endian (PS3.10 8.6) under the record's character set. A record's own
/// Specific Character Set (0008,0005) is read before its text, so it scopes
/// to that record alone.
pub(super) fn parse_records(
    bytes: &[u8],
    records: &[Record],
) -> Result<Vec<InMemDicomObject>, String> {
    records
        .iter()
        .map(|record| {
            let content = bytes.get(record.content.clone()).ok_or_else(|| {
                format!(
                    "the record at byte {} lies outside the DICOMDIR's bytes",
                    record.offset
                )
            })?;
            let decoder = StatefulDecoder::new(
                Cursor::new(content),
                ExplicitVRLittleEndianDecoder::default(),
                LittleEndianBasicDecoder,
                record.charset.clone(),
            );
            InMemDicomObject::read_dataset(decoder).map_err(|err| {
                format!(
                    "dicom-object cannot read the record at byte {}: {err}",
                    record.offset
                )
            })
        })
        .collect()
}

/// Whether records of `record_type` carry the F.5-25/F.5-26 Content
/// Sequence key.
pub(super) fn has_content_key(record_type: &str) -> bool {
    matches!(record_type, "SR DOCUMENT" | "KEY OBJECT DOC")
}

/// PS3.3 Tables F.5-25 and F.5-26: compares the record's Content Sequence
/// with the HAS CONCEPT MOD Content Items of the root of `document`, read
/// from `path`.
pub(super) fn check_content_sequence(
    record: &Record,
    parsed: &InMemDicomObject,
    path: &Path,
    document: &DefaultDicomObject,
    family: &mut Family,
) {
    let record_type = record.record_type();
    let table = if record_type == "SR DOCUMENT" {
        "F.5-25"
    } else {
        "F.5-26"
    };
    let keyed: &[InMemDicomObject] = parsed
        .get(tags::CONTENT_SEQUENCE)
        .and_then(|element| element.items())
        .unwrap_or_default();
    let modifiers: Vec<&InMemDicomObject> = document
        .get(tags::CONTENT_SEQUENCE)
        .and_then(|element| element.items())
        .unwrap_or_default()
        .iter()
        .filter(|item| relationship(item).as_deref() == Some(HAS_CONCEPT_MOD))
        .collect();
    let mut unmatched: Vec<&InMemDicomObject> = modifiers;
    let mut extra = 0usize;
    for item in keyed {
        match unmatched
            .iter()
            .position(|candidate| same_data_set(item, candidate, 0))
        {
            Some(position) => {
                unmatched.swap_remove(position);
            }
            None => extra += 1,
        }
    }
    let place = format!(
        "Content Sequence (0040,A730) of the {record_type} record at byte {}",
        record.offset
    );
    if !unmatched.is_empty() {
        family.push(|| {
            format!(
                "{place} lacks {} of the HAS CONCEPT MOD Content Items of the root of {}, but \
                 shall include all of them (PS3.3 Table {table})",
                unmatched.len(),
                path.display()
            )
        });
    }
    if extra > 0 {
        family.push(|| {
            format!(
                "{place} holds {extra} Content Item{} that {} not a HAS CONCEPT MOD Content Item \
                 of the root of {}, but shall include only those (PS3.3 Table {table})",
                if extra == 1 { "" } else { "s" },
                if extra == 1 { "is" } else { "are" },
                path.display()
            )
        });
    }
}

fn relationship(item: &InMemDicomObject) -> Option<String> {
    let value = item.get(tags::RELATIONSHIP_TYPE)?.to_str().ok()?;
    Some(value.trim_matches([' ', '\0']).to_string())
}

/// Whether two Data Sets hold the same Data Elements with the same Values,
/// Sequence Items in order; Group Length elements and padding aside, and
/// whatever VR each was read with (an Implicit VR document against the
/// Explicit VR DICOMDIR).
fn same_data_set(left: &InMemDicomObject, right: &InMemDicomObject, depth: usize) -> bool {
    if depth >= MAX_NESTING {
        return false;
    }
    let significant = |object: &InMemDicomObject| -> Vec<Tag> {
        object
            .iter()
            .map(|element| element.header().tag)
            .filter(|tag| tag.element() != 0x0000)
            .collect()
    };
    let tags = significant(left);
    if tags != significant(right) {
        return false;
    }
    tags.into_iter()
        .all(|tag| match (left.get(tag), right.get(tag)) {
            (Some(left), Some(right)) => same_value(left.value(), right.value(), depth),
            _ => false,
        })
}

fn same_value(
    left: &ElementValue<InMemDicomObject>,
    right: &ElementValue<InMemDicomObject>,
    depth: usize,
) -> bool {
    match (left, right) {
        (ElementValue::Primitive(left), ElementValue::Primitive(right)) => {
            unpadded(left.to_str()) == unpadded(right.to_str())
        }
        (ElementValue::Sequence(left), ElementValue::Sequence(right)) => {
            left.items().len() == right.items().len()
                && left
                    .items()
                    .iter()
                    .zip(right.items())
                    .all(|(left, right)| same_data_set(left, right, depth + 1))
        }
        _ => false,
    }
}

fn unpadded(text: Cow<'_, str>) -> String {
    text.trim_end_matches([' ', '\0']).to_string()
}

/// Holds the Content Sequence of every SR DOCUMENT and KEY OBJECT DOC
/// record to the structured-report Content Item rules, `parsed` aligned
/// with `records`.
pub(super) fn check_content_items(
    records: &[Record],
    parsed: &[InMemDicomObject],
    family: &mut Family,
) {
    for (record, parsed) in records.iter().zip(parsed) {
        if !has_content_key(&record.record_type()) {
            continue;
        }
        let Some(items) = parsed
            .get(tags::CONTENT_SEQUENCE)
            .and_then(|element| element.items())
        else {
            continue;
        };
        for violation in content_item_violations(items) {
            family.push(|| format!("the record at byte {}: {violation}", record.offset));
        }
    }
}
