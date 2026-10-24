//! The content tree of the Key Object Selection Document and SR document IODs
//! (PS3.3 A.35.4, A.35.1, A.35.2, A.35.3 and A.35.13): in pathology, the
//! measurement reports (TID 1500) and the "acquisition complete" and
//! quality-rejection Key Object Selection Documents of a slide's Series.
//!
//! The IOD checks hold each Content Item to the Document Content and Document
//! Relationship Macros by its Value Type (Tables C.17-5 and C.17-6). These
//! rules walk the tree those Items form, once and bounded (`tree`), and hold it
//! to what the tables cannot express:
//! - the IOD's Value Types and Relationship Content Constraints, by-reference
//!   relationships and SELECTED FROM targets (`relations`);
//! - the Values of the C.18 Content Item Macros (`items`);
//! - the document-level prose of C.17: completion and verification, observers,
//!   evidence against the tree, Identical and Predecessor Documents
//!   (`document`);
//! - the PS3.16 Template at a KOS root (TID 2010) and the Template a document
//!   declares (`template`).
//!
//! `corpus` compares what a document references with the validated set.
//! `content_item_violations` applies the IOD-independent Content Item rules to
//! the Content Sequence of a DICOMDIR record.

mod corpus;
mod document;
mod items;
mod relations;
mod template;
mod tree;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::Path;

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, VR};
use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::iod::structured_report::{document_constraints_of_sop_class, DocumentConstraints};
use super::iod::{dicom_edition, string_value, Family, EDITION};
use super::ValidationCheck;
use tree::Tree;

pub(in crate::validation) use corpus::{run_structured_report_set_checks, StructuredReportCorpus};

pub(super) const STRUCTURED_REPORT_RULE: &str = "intrinsic-wsi-dicom-structured-report";
pub(super) const KEY_OBJECT_SELECTION_RULE: &str = "intrinsic-wsi-dicom-key-object-selection";

/// The Key Object Selection Document IOD.
const KEY_OBJECT_SELECTION: &str = "A.35.4";

const CITATION: &str = concat!("PS3.3 ", dicom_edition!());
const PS3_16: &str = concat!("PS3.16 ", dicom_edition!());

/// Advisories and not-examined notes a check names; the rest are counted.
const MAX_NOTES: usize = 20;
/// Violations `content_item_violations` returns; the rest are counted in one
/// last message.
const MAX_RETURNED: usize = 50;

/// The SR document check of an object of A.35.1, A.35.2, A.35.3 or A.35.13.
pub(in crate::validation) fn run_structured_report_check(
    label: &Path,
    object: &DefaultDicomObject,
) -> Option<ValidationCheck> {
    let constraints = constraints(object).filter(|found| found.iod != KEY_OBJECT_SELECTION)?;
    Some(check_document(
        label,
        object,
        constraints,
        STRUCTURED_REPORT_RULE,
    ))
}

/// The Key Object Selection Document check of an object of A.35.4.
pub(in crate::validation) fn run_key_object_selection_check(
    label: &Path,
    object: &DefaultDicomObject,
) -> Option<ValidationCheck> {
    let constraints = constraints(object).filter(|found| found.iod == KEY_OBJECT_SELECTION)?;
    Some(check_document(
        label,
        object,
        constraints,
        KEY_OBJECT_SELECTION_RULE,
    ))
}

/// The PS3.3 C.17 and C.18 Content Item rules that bind Content Items
/// whatever IOD holds them, for `items`, the Content Sequence of a DICOMDIR
/// SR DOCUMENT or KEY OBJECT DOC record (Tables F.5-25 and F.5-26): the
/// Values of each Value Type's Macro and the SELECTED FROM targets of SCOORD
/// and TCOORD Items. A by-reference Item there names a Content Item of the
/// referenced document, which the record does not hold, so its Target is not
/// examined. The record parse bounds the nesting this walk reads.
pub(in crate::validation) fn content_item_violations(items: &[InMemDicomObject]) -> Vec<String> {
    let root = InMemDicomObject::from_element_iter([DataElement::new(
        tags::CONTENT_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(items.to_vec()),
    )]);
    let tree = Tree::of(&root);
    let mut findings = Findings::returning();
    items::check(&tree, &mut findings);
    relations::check_selected_from(&tree, &mut findings);
    findings.into_messages()
}

/// The Content Constraints of the document IOD whose SOP Class the Data Set's
/// SOP Class UID names, or else the File Meta Information's, as the IOD
/// checks select it.
fn constraints(object: &DefaultDicomObject) -> Option<&'static DocumentConstraints> {
    let declared = string_value(object, tags::SOP_CLASS_UID, 1);
    let meta = Some(
        object
            .meta()
            .media_storage_sop_class_uid()
            .trim_end_matches(['\0', ' '])
            .to_string(),
    );
    [declared, meta]
        .into_iter()
        .flatten()
        .find_map(|uid| document_constraints_of_sop_class(&uid))
}

fn check_document(
    label: &Path,
    object: &DefaultDicomObject,
    constraints: &'static DocumentConstraints,
    rule: &str,
) -> ValidationCheck {
    let root: &InMemDicomObject = object;
    let tree = Tree::of(root);
    let mut findings = Findings::default();
    tree.note_limits(&mut findings);
    relations::check(&tree, constraints, &mut findings);
    items::check(&tree, &mut findings);
    document::check(root, &tree, constraints, &mut findings);
    template::check(root, &tree, constraints, &mut findings);
    findings.check(
        label,
        rule,
        &format!(
            "DICOM PS3.3 {EDITION} {}, C.17 and C.18; PS3.16 {EDITION}",
            constraints.iod
        ),
    )
}

/// What a document check found: its violations, bounded and formatted only
/// while shown; its advisories (departures from Baseline Value Sets and
/// Templates); what it could not examine, with why; and how many Template
/// row conditions it could not evaluate.
#[derive(Debug, Default)]
pub(super) struct Findings {
    violations: Family,
    /// For `content_item_violations`: the violations as messages.
    returned: Option<Returned>,
    advisories: Notes,
    unexamined: Notes,
    undetermined: usize,
}

#[derive(Debug, Default)]
struct Returned {
    shown: Vec<String>,
    total: usize,
}

/// Distinct notes, the first `MAX_NOTES` kept.
#[derive(Debug, Default)]
struct Notes {
    kept: BTreeSet<String>,
    omitted: usize,
}

impl Notes {
    fn add(&mut self, note: String) {
        if self.kept.len() < MAX_NOTES || self.kept.contains(&note) {
            self.kept.insert(note);
        } else {
            self.omitted += 1;
        }
    }

    fn describe(&self) -> Option<String> {
        if self.kept.is_empty() {
            return None;
        }
        let mut text = self.kept.iter().cloned().collect::<Vec<_>>().join("; ");
        if self.omitted > 0 {
            text.push_str(&format!("; and {} more", self.omitted));
        }
        Some(text)
    }
}

impl Findings {
    fn returning() -> Self {
        Self {
            returned: Some(Returned::default()),
            ..Self::default()
        }
    }

    /// Counts a violation; `violation` is formatted only while it is shown.
    pub(super) fn push(&mut self, violation: impl FnOnce() -> String) {
        match &mut self.returned {
            Some(returned) => {
                returned.total += 1;
                if returned.shown.len() < MAX_RETURNED {
                    returned.shown.push(violation());
                }
            }
            None => self.violations.push(violation),
        }
    }

    pub(super) fn advise(&mut self, advisory: String) {
        self.advisories.add(advisory);
    }

    pub(super) fn unexamined(&mut self, reason: String) {
        self.unexamined.add(reason);
    }

    /// Counts a Template row condition the object gives no means to evaluate.
    pub(super) fn undetermined(&mut self) {
        self.undetermined += 1;
    }

    fn check(self, path: &Path, rule: &str, citation: &str) -> ValidationCheck {
        let mut check = self.violations.check(path, rule, citation);
        if self.undetermined > 0 {
            check.message.push_str(&format!(
                "; {} Template row condition evaluation{} undeterminable from the object (not failed)",
                self.undetermined,
                if self.undetermined == 1 { "" } else { "s" }
            ));
        }
        if let Some(advisories) = self.advisories.describe() {
            check.message.push_str("; advisory: ");
            check.message.push_str(&advisories);
        }
        if let Some(unexamined) = self.unexamined.describe() {
            check.message.push_str("; not examined: ");
            check.message.push_str(&unexamined);
        }
        check
    }

    fn into_messages(self) -> Vec<String> {
        let Some(mut returned) = self.returned else {
            return Vec::new();
        };
        let hidden = returned.total - returned.shown.len();
        if hidden > 0 {
            returned
                .shown
                .push(format!("and {hidden} more Content Item violations"));
        }
        returned.shown
    }
}
