//! The set-level requirements of the document IODs: what a KOS or SR
//! document references, compared with the referenced Instances the validated
//! set holds. Referenced SOP Class UID is the referenced Instance's SOP Class
//! (Table 10-11); Referenced Frame Numbers are Frames of it (Table C.18.4-1);
//! SCOORD Graphic Data lies within its Columns and Rows, or its Total Pixel
//! Matrix, with Pixel Origin Interpretation where it is tiled (Table
//! C.18.6-1); and the duplicates an Identical Documents Sequence lists are
//! the entire document, in another Study, listing all the others (C.17.2.2).
//!
//! Each rule's set check reports once per validated set: failed when a
//! compared pair fails, passed when every compared pair passes, skipped when
//! nothing a document references is in the set, and not at all without a
//! document of its IODs. Only what `observe` recorded is compared: a label
//! is never opened.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use dicom_core::value::Value;
use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::tree::{Tree, MAX_ITEMS};
use super::{
    constraints, CITATION, KEY_OBJECT_SELECTION, KEY_OBJECT_SELECTION_RULE, MAX_NOTES,
    STRUCTURED_REPORT_RULE,
};
use crate::validation::disclosure::{shown, shown_all};
use crate::validation::image_semantics::{integer, integers, numbers};
use crate::validation::iod::{dicom_edition, items, string_value, Family, MAX_NESTING};
use crate::validation::{intrinsic_check, ValidationCheck, ValidationStatus};

/// The Hierarchical SOP Instance Reference Macro Sequences whose Instances
/// are compared by SOP Class (Tables C.17-2 and C.17.6-2).
const EVIDENCE: [(dicom_core::Tag, &str); 4] = [
    (
        tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
        "Current Requested Procedure Evidence Sequence (0040,A375)",
    ),
    (
        tags::PERTINENT_OTHER_EVIDENCE_SEQUENCE,
        "Pertinent Other Evidence Sequence (0040,A385)",
    ),
    (
        tags::IDENTICAL_DOCUMENTS_SEQUENCE,
        "Identical Documents Sequence (0040,A525)",
    ),
    (
        tags::PREDECESSOR_DOCUMENTS_SEQUENCE,
        "Predecessor Documents Sequence (0040,A360)",
    ),
];

/// What the set checks compare, gathered from each validated object.
#[derive(Debug, Default)]
pub(in crate::validation) struct StructuredReportCorpus {
    instances: BTreeMap<String, Instance>,
    documents: Vec<Document>,
}

/// Any validated Instance, as a referenced one.
#[derive(Debug)]
struct Instance {
    path: PathBuf,
    class: Option<String>,
    frames: Option<i64>,
    /// Columns and Rows.
    matrix: Option<(i64, i64)>,
    /// Total Pixel Matrix Columns and Rows, of a tiled image.
    total: Option<(i64, i64)>,
}

/// Where a document references an Instance.
#[derive(Debug)]
enum Locator {
    /// A Content Item, by its Referenced Content Item Identifier.
    ContentItem(String),
    Sequence(&'static str),
}

impl std::fmt::Display for Locator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContentItem(identifier) => write!(formatter, "Content Item {identifier}"),
            Self::Sequence(name) => formatter.write_str(name),
        }
    }
}

#[derive(Debug)]
struct Reference {
    instance: String,
    class: Option<String>,
    /// Referenced Frame Numbers of an IMAGE Content Item.
    frames: Vec<i64>,
    locator: Locator,
}

/// An SCOORD Content Item and an image it is selected from.
#[derive(Debug)]
struct Region {
    instance: String,
    origin: Option<String>,
    /// The largest column and row of its Graphic Data.
    extent: (f64, f64),
    locator: Locator,
}

#[derive(Debug)]
struct Document {
    path: PathBuf,
    rule: &'static str,
    instance: Option<String>,
    study: Option<String>,
    references: Vec<Reference>,
    regions: Vec<Region>,
    identical: Vec<String>,
    /// The Document Title and Content Sequence, hashed.
    content: u64,
}

impl StructuredReportCorpus {
    /// Records the facts of one validated object.
    pub(in crate::validation) fn observe(&mut self, path: &Path, object: &DefaultDicomObject) {
        let root: &InMemDicomObject = object;
        let instance = string_value(root, tags::SOP_INSTANCE_UID, 1);
        if let Some(uid) = &instance {
            self.instances
                .entry(uid.clone())
                .or_insert_with(|| Instance {
                    path: path.to_path_buf(),
                    class: string_value(root, tags::SOP_CLASS_UID, 1),
                    frames: integer(root, tags::NUMBER_OF_FRAMES),
                    matrix: pair(root, tags::COLUMNS, tags::ROWS),
                    total: pair(
                        root,
                        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
                        tags::TOTAL_PIXEL_MATRIX_ROWS,
                    ),
                });
        }
        let Some(constraints) = constraints(object) else {
            return;
        };
        let tree = Tree::of(root);
        let rule = if constraints.iod == KEY_OBJECT_SELECTION {
            KEY_OBJECT_SELECTION_RULE
        } else {
            STRUCTURED_REPORT_RULE
        };
        let mut references = content_references(&tree);
        references.extend(evidence_references(root));
        self.documents.push(Document {
            path: path.to_path_buf(),
            rule,
            instance,
            study: string_value(root, tags::STUDY_INSTANCE_UID, 1),
            references,
            regions: regions(&tree),
            identical: items(root, tags::IDENTICAL_DOCUMENTS_SEQUENCE)
                .iter()
                .flat_map(|study| items(study, tags::REFERENCED_SERIES_SEQUENCE))
                .flat_map(|series| items(series, tags::REFERENCED_SOP_SEQUENCE))
                .filter_map(|sop| string_value(sop, tags::REFERENCED_SOP_INSTANCE_UID, 1))
                .collect(),
            content: content_digest(root),
        });
    }
}

fn pair(
    object: &InMemDicomObject,
    first: dicom_core::Tag,
    second: dicom_core::Tag,
) -> Option<(i64, i64)> {
    Some((integer(object, first)?, integer(object, second)?))
}

/// The Instances the IMAGE, WAVEFORM and COMPOSITE Content Items reference,
/// with the presentation states and Real World Value Mappings an IMAGE
/// references with its image.
fn content_references(tree: &Tree<'_>) -> Vec<Reference> {
    let mut found = Vec::new();
    for (index, node) in tree.nodes.iter().enumerate() {
        if node.is_by_reference()
            || !matches!(node.value_type(), Some("IMAGE" | "WAVEFORM" | "COMPOSITE"))
        {
            continue;
        }
        let image = node.value_type() == Some("IMAGE");
        for reference in items(node.object, tags::REFERENCED_SOP_SEQUENCE) {
            let accompanying = [
                tags::REFERENCED_SOP_SEQUENCE,
                tags::REFERENCED_REAL_WORLD_VALUE_MAPPING_INSTANCE_SEQUENCE,
            ]
            .into_iter()
            .flat_map(|tag| items(reference, tag));
            for (position, instance) in std::iter::once(reference).chain(accompanying).enumerate() {
                let Some(uid) = string_value(instance, tags::REFERENCED_SOP_INSTANCE_UID, 1) else {
                    continue;
                };
                found.push(Reference {
                    instance: uid,
                    class: string_value(instance, tags::REFERENCED_SOP_CLASS_UID, 1),
                    frames: if image && position == 0 {
                        integers(instance, tags::REFERENCED_FRAME_NUMBER).unwrap_or_default()
                    } else {
                        Vec::new()
                    },
                    locator: Locator::ContentItem(tree.identifier(index)),
                });
            }
        }
    }
    found
}

fn evidence_references(root: &InMemDicomObject) -> Vec<Reference> {
    let mut found = Vec::new();
    for (tag, name) in EVIDENCE {
        for sop in items(root, tag)
            .iter()
            .flat_map(|study| items(study, tags::REFERENCED_SERIES_SEQUENCE))
            .flat_map(|series| items(series, tags::REFERENCED_SOP_SEQUENCE))
        {
            if let Some(uid) = string_value(sop, tags::REFERENCED_SOP_INSTANCE_UID, 1) {
                found.push(Reference {
                    instance: uid,
                    class: string_value(sop, tags::REFERENCED_SOP_CLASS_UID, 1),
                    frames: Vec::new(),
                    locator: Locator::Sequence(name),
                });
            }
        }
    }
    found
}

/// Each SCOORD Content Item with each IMAGE it is selected from.
fn regions(tree: &Tree<'_>) -> Vec<Region> {
    let mut found = Vec::new();
    for (index, node) in tree.nodes.iter().enumerate() {
        if node.is_by_reference() || node.value_type() != Some("SCOORD") {
            continue;
        }
        let Some(values) =
            numbers(node.object, tags::GRAPHIC_DATA).filter(|values| values.len() >= 2)
        else {
            continue;
        };
        let largest = |offset: usize| {
            values
                .iter()
                .skip(offset)
                .step_by(2)
                .copied()
                .fold(f64::MIN, f64::max)
        };
        let extent = (largest(0), largest(1));
        let origin = string_value(node.object, tags::PIXEL_ORIGIN_INTERPRETATION, 1);
        for child in tree.children(index) {
            if tree.nodes[child].relationship() != Some("SELECTED FROM") {
                continue;
            }
            let Some(target) = tree
                .target(child)
                .filter(|target| tree.nodes[*target].value_type() == Some("IMAGE"))
            else {
                continue;
            };
            let Some(uid) = items(tree.nodes[target].object, tags::REFERENCED_SOP_SEQUENCE)
                .first()
                .and_then(|sop| string_value(sop, tags::REFERENCED_SOP_INSTANCE_UID, 1))
            else {
                continue;
            };
            found.push(Region {
                instance: uid,
                origin: origin.clone(),
                extent,
                locator: Locator::ContentItem(tree.identifier(index)),
            });
        }
    }
    found
}

/// A hash of the Document Title and the Content Sequence: the content an
/// identical copy duplicates, apart from the identifying Attributes it
/// changes.
fn content_digest(root: &InMemDicomObject) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut budget = MAX_ITEMS;
    for tag in [tags::CONCEPT_NAME_CODE_SEQUENCE, tags::CONTENT_SEQUENCE] {
        for item in items(root, tag) {
            hash_data_set(item, 0, &mut budget, &mut hasher);
        }
    }
    hasher.finish()
}

fn hash_data_set(
    object: &InMemDicomObject,
    depth: usize,
    budget: &mut usize,
    hasher: &mut DefaultHasher,
) {
    if depth >= MAX_NESTING || *budget == 0 {
        return;
    }
    *budget -= 1;
    for element in object.iter() {
        let tag = element.header().tag;
        if tag.element() == 0 {
            continue;
        }
        (tag.group(), tag.element()).hash(hasher);
        match element.value() {
            Value::Sequence(sequence) => {
                for item in sequence.items() {
                    hash_data_set(item, depth + 1, budget, hasher);
                }
            }
            Value::Primitive(primitive) => {
                primitive
                    .to_str()
                    .trim_end_matches([' ', '\0'])
                    .hash(hasher);
            }
            Value::PixelSequence(_) => {}
        }
    }
}

/// What one rule's set check compared.
#[derive(Default)]
struct Outcome {
    compared: usize,
    violations: Family,
    unexamined: BTreeSet<String>,
}

/// The set checks of the KOS and SR document rules.
pub(in crate::validation) fn run_structured_report_set_checks(
    corpus: &StructuredReportCorpus,
) -> Vec<ValidationCheck> {
    [STRUCTURED_REPORT_RULE, KEY_OBJECT_SELECTION_RULE]
        .into_iter()
        .filter_map(|rule| set_check(corpus, rule))
        .collect()
}

fn set_check(corpus: &StructuredReportCorpus, rule: &'static str) -> Option<ValidationCheck> {
    let documents: Vec<&Document> = corpus
        .documents
        .iter()
        .filter(|document| document.rule == rule)
        .collect();
    if documents.is_empty() {
        return None;
    }
    let citation = concat!(
        "DICOM PS3.3 ",
        dicom_edition!(),
        " C.17.2.2, C.18.4 and C.18.6"
    );
    let mut outcome = Outcome::default();
    for document in &documents {
        references(corpus, document, &mut outcome);
        regions_within(corpus, document, &mut outcome);
        identical(corpus, document, &mut outcome);
    }
    if outcome.violations.is_empty() && outcome.compared == 0 {
        return Some(intrinsic_check(
            rule,
            None,
            ValidationStatus::Skipped,
            format!("{citation}: no Instance a document references is in the validated set"),
        ));
    }
    let mut check = outcome.violations.check(Path::new(""), rule, citation);
    if !outcome.unexamined.is_empty() {
        let shown: Vec<String> = outcome.unexamined.iter().take(MAX_NOTES).cloned().collect();
        check.message.push_str("; not examined: ");
        check.message.push_str(&shown.join("; "));
        let hidden = outcome.unexamined.len() - shown.len();
        if hidden > 0 {
            check.message.push_str(&format!("; and {hidden} more"));
        }
    }
    Some(check)
}

/// Table 10-11: Referenced SOP Class UID identifies the referenced
/// Instance's SOP Class; Table C.18.4-1: Referenced Frame Numbers identify
/// Frames of it.
fn references(corpus: &StructuredReportCorpus, document: &Document, outcome: &mut Outcome) {
    for reference in &document.references {
        let Some(instance) = corpus.instances.get(&reference.instance) else {
            continue;
        };
        outcome.compared += 1;
        if let (Some(class), Some(actual)) = (&reference.class, &instance.class) {
            if class != actual {
                outcome.violations.push(|| format!(
                    "{} {} references SOP Instance {} with Referenced SOP Class UID {class}, but {} has SOP Class UID {actual}; Referenced SOP Class UID uniquely identifies the referenced SOP Class ({CITATION} Table 10-11)",
                    document.path.display(),
                    reference.locator,
                    shown(tags::REFERENCED_SOP_INSTANCE_UID, &reference.instance),
                    instance.path.display()
                ));
            }
        }
        let frames = instance.frames.unwrap_or(1);
        if let Some(frame) = reference.frames.iter().find(|frame| **frame > frames) {
            outcome.violations.push(|| format!(
                "{} {} references Frame {frame} of {}, which has {frames} Frame{}; Referenced Frame Number identifies the Frame numbers within the Referenced SOP Instance ({CITATION} Table C.18.4-1)",
                document.path.display(),
                reference.locator,
                instance.path.display(),
                if frames == 1 { "" } else { "s" }
            ));
        }
    }
}

/// Table C.18.6-1: Graphic Data within 0\0 to Columns\Rows, or to the Total
/// Pixel Matrix for Pixel Origin Interpretation VOLUME, which is "Required
/// if the instance referenced ... is tiled".
fn regions_within(corpus: &StructuredReportCorpus, document: &Document, outcome: &mut Outcome) {
    for region in &document.regions {
        let Some(image) = corpus.instances.get(&region.instance) else {
            continue;
        };
        outcome.compared += 1;
        let (place, path) = (&region.locator, &image.path);
        let bounds = match (region.origin.as_deref(), image.total) {
            (None, Some(_)) => {
                outcome.violations.push(|| format!(
                    "{} {place} is an SCOORD selected from {}, a tiled image (it has Total Pixel Matrix Columns and Rows), without Pixel Origin Interpretation (0048,0301); it is required if the referenced instance is tiled ({CITATION} Table C.18.6-1)",
                    document.path.display(),
                    path.display()
                ));
                continue;
            }
            (Some("VOLUME"), total) => {
                total.map(|bounds| (bounds, "Total Pixel Matrix Columns\\Rows"))
            }
            _ => image.matrix.map(|bounds| (bounds, "Columns\\Rows")),
        };
        let Some(((columns, rows), names)) = bounds else {
            outcome.unexamined.insert(format!(
                "the extent of the SCOORD {} {place} in {}, which has no {}",
                document.path.display(),
                path.display(),
                if region.origin.as_deref() == Some("VOLUME") {
                    "Total Pixel Matrix"
                } else {
                    "Columns and Rows"
                }
            ));
            continue;
        };
        let (column, row) = region.extent;
        if column > columns as f64 || row > rows as f64 {
            outcome.violations.push(|| format!(
                "{} {place} has Graphic Data reaching column {column} and row {row} of {}, whose {names} are {columns}\\{rows}; the values must be within the range 0\\0 to {names} ({CITATION} Table C.18.6-1)",
                document.path.display(),
                path.display()
            ));
        }
    }
}

/// C.17.2.2: a duplicate the Identical Documents Sequence lists is "the
/// entire document ... duplicated ... for inclusion into the different
/// Studies", and "The Identical Documents Sequence Attribute in each SOP
/// Instance shall contain references to all other duplicate SOP Instances."
fn identical(corpus: &StructuredReportCorpus, document: &Document, outcome: &mut Outcome) {
    let Some(own) = &document.instance else {
        return;
    };
    for listed in &document.identical {
        let Some(duplicate) = corpus
            .documents
            .iter()
            .find(|candidate| candidate.instance.as_ref() == Some(listed))
        else {
            continue;
        };
        outcome.compared += 1;
        let missing: Vec<&String> = std::iter::once(own)
            .chain(document.identical.iter().filter(|other| *other != listed))
            .filter(|uid| !duplicate.identical.contains(uid))
            .collect();
        if !missing.is_empty() {
            outcome.violations.push(|| {
                format!(
                    "{} lists {} in Identical Documents Sequence (0040,A525), but that duplicate's Identical Documents Sequence does not reference {}; the Identical Documents Sequence in each SOP Instance shall contain references to all other duplicate SOP Instances ({CITATION} C.17.2.2)",
                    document.path.display(),
                    duplicate.path.display(),
                    shown_all(
                        tags::REFERENCED_SOP_INSTANCE_UID,
                        missing.iter().map(|uid| uid.as_str()),
                        ", "
                    )
                )
            });
        }
        // The pair's content and Studies once, from its lesser UID.
        if duplicate.instance.as_ref().is_some_and(|uid| uid < own)
            && duplicate.identical.contains(own)
        {
            continue;
        }
        if document.study.is_some() && document.study == duplicate.study {
            outcome.violations.push(|| format!(
                "{} and its duplicate {} are both in Study {}; identical copies of a document are duplicated for inclusion into different Studies ({CITATION} C.17.2.2)",
                document.path.display(),
                duplicate.path.display(),
                shown(
                    tags::STUDY_INSTANCE_UID,
                    document.study.as_deref().unwrap_or_default()
                )
            ));
        }
        if document.content != duplicate.content {
            outcome.violations.push(|| format!(
                "{} lists {} in Identical Documents Sequence (0040,A525), but their Document Titles or Content Sequences differ; the entire document shall be duplicated, changing only what inclusion into the other Study requires ({CITATION} C.17.2.2)",
                document.path.display(),
                duplicate.path.display()
            ));
        }
    }
}
