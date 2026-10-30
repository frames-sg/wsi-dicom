//! The PS3.15 Table E.1-1 and Table E.3.4-1 actions, as far as a
//! de-identified object can show their breach: an Attribute or Content Item
//! present where every applicable action is X, or without a Value where every
//! applicable action requires one (D, U, X/D). Z, K, C and the other
//! compound actions admit any Value the object could hold, since a dummy or
//! cleaned Value cannot be told from an original without the original.

use std::fmt;
use std::path::Path;

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;

use super::{is_empty, Declaration, Findings};
use crate::validation::disclosure::shown;
use crate::validation::iod::deidentification::{
    confidentiality_attribute, structured_content_concept, ConfidentialityOption,
    DeidentificationAction,
};
use crate::validation::iod::{
    element_label, integer_value, is_private, items, string_value, Family, MAX_NESTING,
};

use DeidentificationAction as A;

/// The actions a Table E.1-1 or E.3.4-1 row may prescribe under
/// `declaration`: each declared Option's override, else the Basic Profile's.
/// When no CID 7050 code says which Options apply, any of them may.
pub(super) fn candidates(
    basic: DeidentificationAction,
    options: &[(ConfidentialityOption, DeidentificationAction)],
    declaration: &Declaration,
) -> Vec<DeidentificationAction> {
    if !declaration.options_known() {
        let mut every: Vec<_> = std::iter::once(basic)
            .chain(options.iter().map(|(_, action)| *action))
            .collect();
        every.dedup();
        return every;
    }
    let overriding: Vec<_> = options
        .iter()
        .filter(|(option, _)| declaration.has(*option))
        .map(|(_, action)| *action)
        .collect();
    if overriding.is_empty() {
        vec![basic]
    } else {
        overriding
    }
}

/// Whether an Attribute present with or without a Value breaks every one
/// of `candidates`.
fn breaks(candidates: &[DeidentificationAction], empty: bool) -> bool {
    !candidates.is_empty()
        && candidates.iter().all(|action| match action {
            A::X => true,
            A::D | A::U | A::XD => empty,
            _ => false,
        })
}

/// How an Attribute that [`breaks`] every one of `candidates` breaks them.
fn breach(candidates: &[DeidentificationAction], table: &str) -> String {
    let codes: Vec<&str> = candidates.iter().map(|action| action.code()).collect();
    let codes = codes.join(" or ");
    if candidates.iter().all(|action| *action == A::X) {
        format!("is present; {table} action X requires its removal")
    } else if candidates
        .iter()
        .any(|action| matches!(action, A::X | A::XD))
    {
        format!(
            "is present without a Value; {table} action {codes} requires its removal or a non-zero length Value"
        )
    } else {
        format!("has no Value; {table} action {codes} requires a non-zero length Value")
    }
}

/// Holds every Data Element of `object`, at any depth, to PS3.15 E.1.1
/// (group 0004, Table E.1-1), E.3.10 (Private Attributes) and, under the
/// Clean Structured Content Option, Table E.3.4-1.
pub(super) fn check_data_set(
    object: &InMemDicomObject,
    declaration: &Declaration,
    findings: &mut Findings,
) {
    let structured = declaration.has(ConfidentialityOption::CleanStructuredContent);
    let mut walk = Walk {
        declaration,
        report: Report {
            violations: &mut findings.violations,
            trail: Vec::new(),
            record: None,
        },
        structured,
        blocks: private_blocks(object),
    };
    walk.data_set(object);
    if structured {
        specimen_identifier_conflict(object, findings);
    }
}

/// A Directory Record of a DICOMDIR, as messages name it.
#[derive(Clone, Copy, Debug)]
pub(super) struct RecordAt<'a> {
    pub(super) dicomdir: &'a Path,
    pub(super) record_type: &'a str,
    /// The byte offset of its Item.
    pub(super) offset: u64,
}

impl fmt::Display for RecordAt<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {} record at byte {}",
            self.dicomdir.display(),
            self.record_type.escape_default(),
            self.offset
        )
    }
}

/// Holds the Directory Record `record` at `at`, which describes
/// de-identified Files, to Table E.1-1, E.3.10 and, under the Clean
/// Structured Content Option, Table E.3.4-1, at every depth (E.1.1: the
/// DICOMDIR is created from the de-identified Files or itself
/// de-identified). Only a Value the Profile removes is reported: an empty
/// Record Key, which PS3.3 F.5 may require as Type 2, holds nothing to
/// remove, and group 0004 is the record's own structure.
pub(super) fn check_record(
    record: &InMemDicomObject,
    at: RecordAt<'_>,
    declaration: &Declaration,
    violations: &mut Family,
) {
    let mut walk = Walk {
        declaration,
        report: Report {
            violations,
            trail: Vec::new(),
            record: Some(at),
        },
        structured: declaration.has(ConfidentialityOption::CleanStructuredContent),
        blocks: private_blocks(record),
    };
    walk.data_set(record);
}

struct Walk<'a> {
    declaration: &'a Declaration,
    report: Report<'a>,
    structured: bool,
    /// What Private Data Element Characteristics Sequence (0008,0300) says
    /// of each Private block.
    blocks: Vec<PrivateBlock>,
}

/// The violations the walk counts, and where it is: each Sequence above the
/// Data Set walked, with the 1-based number of the Item it descends into.
/// A path is formatted only for a violation that will be shown.
struct Report<'a> {
    violations: &'a mut Family,
    trail: Vec<(Tag, usize)>,
    /// The Directory Record walked, if it is one.
    record: Option<RecordAt<'a>>,
}

impl Report<'_> {
    /// Counts a violation by the Data Element `tag` of the Data Set walked;
    /// `message` receives the element's label.
    fn element(&mut self, tag: Tag, message: impl FnOnce(&str) -> String) {
        let (record, trail) = (self.record, &self.trail);
        self.violations
            .push(|| message(&element_label(&item_path(record, trail), tag)));
    }

    /// Counts a violation by the Data Set walked; `message` receives the
    /// path that prefixes its Attributes.
    fn item(&mut self, message: impl FnOnce(&str) -> String) {
        let (record, trail) = (self.record, &self.trail);
        self.violations.push(|| message(&item_path(record, trail)));
    }
}

/// The Directory Record, if any, then `<Keyword> (gggg,eeee)[n] > ` for
/// each Sequence and Item of `trail`, as the value checks prefix a nested
/// element.
fn item_path(record: Option<RecordAt<'_>>, trail: &[(Tag, usize)]) -> String {
    let start = record.map_or_else(String::new, |record| format!("{record} > "));
    trail.iter().fold(start, |path, (tag, number)| {
        format!("{}[{number}] > ", element_label(&path, *tag))
    })
}

/// One Item of Private Data Element Characteristics Sequence (0008,0300)
/// (PS3.3 C.12.1.1.7).
#[derive(Debug)]
struct PrivateBlock {
    group: u16,
    creator: String,
    /// Block Identifying Information Status (0008,0303).
    status: String,
    /// Nonidentifying Private Elements (0008,0304): the low 8 bits of tags.
    safe: Vec<u16>,
    /// Each Deidentification Action Sequence (0008,0305) Item: its
    /// Identifying Private Elements (0008,0306) and Deidentification Action
    /// (0008,0307).
    actions: Vec<(Vec<u16>, String)>,
}

fn private_blocks(object: &InMemDicomObject) -> Vec<PrivateBlock> {
    items(object, tags::PRIVATE_DATA_ELEMENT_CHARACTERISTICS_SEQUENCE)
        .iter()
        .filter_map(|block| {
            Some(PrivateBlock {
                group: u16::try_from(integer_value(block, tags::PRIVATE_GROUP_REFERENCE)?).ok()?,
                creator: string_value(block, tags::PRIVATE_CREATOR_REFERENCE, 1)?,
                status: string_value(block, tags::BLOCK_IDENTIFYING_INFORMATION_STATUS, 1)
                    .unwrap_or_default(),
                safe: offsets(block, tags::NONIDENTIFYING_PRIVATE_ELEMENTS),
                actions: items(block, tags::DEIDENTIFICATION_ACTION_SEQUENCE)
                    .iter()
                    .map(|action| {
                        (
                            offsets(action, tags::IDENTIFYING_PRIVATE_ELEMENTS),
                            string_value(action, tags::DEIDENTIFICATION_ACTION, 1)
                                .unwrap_or_default(),
                        )
                    })
                    .collect(),
            })
        })
        .collect()
}

fn offsets(item: &InMemDicomObject, tag: Tag) -> Vec<u16> {
    item.get(tag)
        .and_then(|element| element.to_multi_int::<u16>().ok())
        .unwrap_or_default()
}

impl Walk<'_> {
    fn data_set(&mut self, data_set: &InMemDicomObject) {
        if self.report.trail.len() > MAX_NESTING {
            self.report.item(|path| {
                format!(
                    "{path}Items nested deeper than {MAX_NESTING} levels cannot be held to Table E.1-1"
                )
            });
            return;
        }
        if self.structured {
            self.content_item(data_set);
        }
        for element in data_set.iter() {
            let tag = element.header().tag;
            if self.reported(data_set, element, tag) {
                continue;
            }
            let Some(nested) = element.items() else {
                continue;
            };
            for (index, item) in nested.iter().enumerate() {
                self.report.trail.push((tag, index + 1));
                self.data_set(item);
                self.report.trail.pop();
            }
        }
    }

    /// Reports `element` when it breaks Annex E; a reported Sequence's Items
    /// are not walked.
    fn reported(&mut self, data_set: &InMemDicomObject, element: &InMemElement, tag: Tag) -> bool {
        if tag.group() == 0x0004 && self.report.record.is_some() {
            return true;
        }
        if tag.group() == 0x0004 {
            self.report.element(tag, |label| {
                format!(
                    "{label} is present; E.1.1 requires every Data Element of group 0004 removed outside a DICOMDIR"
                )
            });
            return true;
        }
        if is_private(tag) {
            return self.private(data_set, element);
        }
        let Some(row) = confidentiality_attribute(tag) else {
            return false;
        };
        let mut candidates = candidates(row.basic, row.options, self.declaration);
        // Table E.1-1 Note 11: the Clean Pixel Data Option may clean, or
        // recreate, the Pixel Data of an Icon Image Sequence, which is
        // removed only when it cannot be cleaned.
        if tag == tags::ICON_IMAGE_SEQUENCE
            && (!self.declaration.options_known()
                || self.declaration.has(ConfidentialityOption::CleanPixelData))
        {
            candidates.push(A::C);
        }
        if !self.breaks(&candidates, is_empty(element)) {
            return false;
        }
        self.report.element(tag, |label| {
            format!("{label} {}", breach(&candidates, "Table E.1-1"))
        });
        true
    }

    /// Whether an Attribute present with or without a Value breaks every
    /// one of `candidates`; in a Directory Record, whether it holds a Value
    /// every one of them removes.
    fn breaks(&self, candidates: &[DeidentificationAction], empty: bool) -> bool {
        if self.report.record.is_none() {
            return breaks(candidates, empty);
        }
        !empty && !candidates.is_empty() && candidates.iter().all(|action| *action == A::X)
    }

    /// E.3.10: without the Retain Safe Private Option every Private
    /// Attribute is removed; with it, each retained one keeps the Private
    /// Creator that reserves its block (PS3.5 7.8.1).
    fn private(&mut self, data_set: &InMemDicomObject, element: &InMemElement) -> bool {
        let tag = element.header().tag;
        if !self.declaration.options_known() {
            // Retain Safe Private may apply; the text does not say.
            return false;
        }
        if self.report.record.is_some() && is_empty(element) {
            return false;
        }
        if !self
            .declaration
            .has(ConfidentialityOption::RetainSafePrivate)
        {
            self.report.element(tag, |label| {
                format!(
                    "{label} is present; without the Retain Safe Private Option E.3.10 and Table E.1-1 require every Private Attribute removed"
                )
            });
            return true;
        }
        let block = tag.element() >> 8;
        if block < 0x10 {
            return false;
        }
        let creator = Tag(tag.group(), block);
        let Some(creator) = string_value(data_set, creator, 1) else {
            self.report.element(tag, |label| {
                format!(
                    "{label} is retained without its Private Creator ({:04X},{:04X}); E.3.10 retains safe Private Attributes together with the Private Creator IDs that define them",
                    creator.group(),
                    creator.element()
                )
            });
            return false;
        };
        let Some(breach) = characterized_breach(&self.blocks, tag, &creator, element) else {
            return false;
        };
        self.report
            .element(tag, |label| format!("{label} {}", breach.describe()));
        true
    }

    /// Table E.3.4-1: a Content Item whose Concept Name and Value Type the
    /// table lists, held to the action the declared Options give it.
    fn content_item(&mut self, item: &InMemDicomObject) {
        let Some(value_type) = string_value(item, tags::VALUE_TYPE, 1) else {
            return;
        };
        let Some(concept) = items(item, tags::CONCEPT_NAME_CODE_SEQUENCE).first() else {
            return;
        };
        let (Some(scheme), Some(value)) = (
            string_value(concept, tags::CODING_SCHEME_DESIGNATOR, 1),
            string_value(concept, tags::CODE_VALUE, 1),
        ) else {
            return;
        };
        let Some(row) = structured_content_concept(&scheme, &value, &value_type) else {
            return;
        };
        let candidates = candidates(row.basic, row.options, self.declaration);
        let empty = content_value_tag(&value_type)
            .and_then(|tag| item.get(tag))
            .is_none_or(is_empty);
        if !self.breaks(&candidates, empty) {
            return;
        }
        self.report.item(|path| {
            format!(
                "{path}Content Item ({value}, {scheme}, \"{}\") {value_type} {}",
                row.code.meaning,
                breach(&candidates, "Table E.3.4-1")
            )
        });
    }
}

/// How a retained Private Attribute departs from what Private Data Element
/// Characteristics Sequence (0008,0300) says of its block (E.3.10).
enum PrivateBreach<'a> {
    /// Neither marked safe nor given a Deidentification Action (0008,0307).
    Uncharacterized { creator: &'a str, status: &'a str },
    /// Retained under Deidentification Action X.
    Retained,
    /// Without a Value under Deidentification Action D or U.
    Valueless(&'a str),
}

impl PrivateBreach<'_> {
    fn describe(&self) -> String {
        match self {
            Self::Uncharacterized { creator, status } => format!(
                "is retained although Private Data Element Characteristics Sequence (0008,0300) gives its block of {} Block Identifying Information Status {status} without marking it safe or giving a Deidentification Action (0008,0307); E.3.10 requires it removed",
                shown(tags::PRIVATE_CREATOR_REFERENCE, creator)
            ),
            Self::Retained => "is retained although its Deidentification Action (0008,0307) is X; E.3.10 requires it processed as recommended, here removed"
                .to_string(),
            Self::Valueless(action) => format!(
                "has no Value although its Deidentification Action (0008,0307) is {action}; E.3.10 requires it processed as recommended, with a non-zero length Value"
            ),
        }
    }
}

/// E.3.10: a Private Attribute that Private Data Element Characteristics
/// Sequence (0008,0300) does not mark safe in `blocks` is removed, or
/// processed as its Deidentification Action (0008,0307) recommends.
fn characterized_breach<'a>(
    blocks: &'a [PrivateBlock],
    tag: Tag,
    creator: &str,
    element: &InMemElement,
) -> Option<PrivateBreach<'a>> {
    let block = blocks
        .iter()
        .find(|block| block.group == tag.group() && block.creator == creator)?;
    let offset = tag.element() & 0x00FF;
    if block.status == "SAFE" || (block.status == "MIXED" && block.safe.contains(&offset)) {
        return None;
    }
    let action = block
        .actions
        .iter()
        .find(|(elements, _)| elements.contains(&offset))
        .map(|(_, action)| action.as_str());
    match action {
        None => Some(PrivateBreach::Uncharacterized {
            creator: &block.creator,
            status: &block.status,
        }),
        Some("X") => Some(PrivateBreach::Retained),
        Some(action @ ("D" | "U")) if is_empty(element) => Some(PrivateBreach::Valueless(action)),
        _ => None,
    }
}

/// The Attribute holding a Content Item's Value, by Value Type (PS3.3
/// Table C.17-5), for the Value Types whose emptiness a D or U action
/// forbids.
fn content_value_tag(value_type: &str) -> Option<Tag> {
    Some(match value_type {
        "TEXT" => tags::TEXT_VALUE,
        "UIDREF" => tags::UID,
        "DATETIME" => tags::DATE_TIME,
        "DATE" => tags::DATE,
        "TIME" => tags::TIME,
        "PNAME" => tags::PERSON_NAME,
        _ => return None,
    })
}

/// PS3.15 Table E.3.4-1 removes (121041, DCM, "Specimen Identifier") TEXT
/// Content Items under the Clean Structured Content Option, while PS3.16 TID
/// 8001 requires one in each Specimen Preparation step: a conflict within
/// the standard, reported as an advisory, never a failure.
fn specimen_identifier_conflict(object: &InMemDicomObject, findings: &mut Findings) {
    let steps = items(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE)
        .iter()
        .flat_map(|specimen| items(specimen, tags::SPECIMEN_PREPARATION_SEQUENCE))
        .filter(|step| {
            !items(step, tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE)
                .iter()
                .any(is_specimen_identifier)
        })
        .count();
    if steps == 0 {
        return;
    }
    findings.advisories.push(format!(
        "{steps} Specimen Preparation Sequence (0040,0610) Item(s) lack a (121041, DCM, \"Specimen Identifier\") Content Item: PS3.15 {edition} Table E.3.4-1 gives it action X under the Clean Structured Content Option, while PS3.16 {edition} TID 8001 requires it in each Specimen Preparation step; the standard conflicts, so its absence is not a de-identification failure",
        edition = crate::validation::iod::EDITION
    ));
}

fn is_specimen_identifier(item: &InMemDicomObject) -> bool {
    items(item, tags::CONCEPT_NAME_CODE_SEQUENCE)
        .first()
        .is_some_and(|concept| {
            string_value(concept, tags::CODE_VALUE, 1).as_deref() == Some("121041")
                && string_value(concept, tags::CODING_SCHEME_DESIGNATOR, 1).as_deref()
                    == Some("DCM")
        })
}
