//! PS3.3 A.35 Content Constraints of the Structured Reporting IODs (the Key
//! Object Selection Document and the Basic Text, Enhanced, Comprehensive and
//! Comprehensive 3D SR documents) and the PS3.16 Templates their content is
//! built from, for the rule that walks an SR content tree.
//!
//! The IOD checks hold a KOS or SR object to its Module tables, each Content
//! Item to the Document Content and Document Relationship Macros (Tables
//! C.17-5 and C.17-6) by its Value Type and by-value or by-reference
//! inclusion; the Relationship Content Constraints, the Value Types an IOD
//! permits, and the Templates are the content rule's to apply.

use dicom_object::InMemDicomObject;

use super::tables::{DOCUMENT_CONSTRAINTS, IODS};
use super::templates;
use super::value_sets::TEMPLATES;

pub(in crate::validation) use super::codes::{code_finding, context_group_name, CodeFinding};

pub(in crate::validation) use super::model::{
    Concept, RowRequirement, Strength, Template, TemplateCondition, TemplateRow, ValueSet,
};

/// One row of a Relationship Content Constraints table (e.g. Table
/// A.35.3-2): a Source Content Item of one of `sources` may have Target
/// Content Items of one of `targets` by `relationship`.
#[derive(Debug)]
pub(in crate::validation) struct RelationshipConstraint {
    /// Source Value Types; empty for "any type".
    pub(in crate::validation) sources: &'static [&'static str],
    /// Relationship Type (0040,A010).
    pub(in crate::validation) relationship: &'static str,
    pub(in crate::validation) targets: &'static [&'static str],
}

impl RelationshipConstraint {
    /// Whether a Source of `source` may relate to a Target of `target` by
    /// this row.
    pub(in crate::validation) fn admits(
        &self,
        source: &str,
        relationship: &str,
        target: &str,
    ) -> bool {
        self.relationship == relationship
            && (self.sources.is_empty() || self.sources.contains(&source))
            && self.targets.contains(&target)
    }
}

/// The A.35.x.3.1 Content Constraints of one Structured Reporting IOD.
#[derive(Debug)]
pub(in crate::validation) struct DocumentConstraints {
    /// The IOD's PS3.3 section, e.g. "A.35.4".
    pub(in crate::validation) iod: &'static str,
    /// A.35.x.3.1.1: the Value Types (0040,A040) Content Sequence (0040,A730)
    /// Items may have.
    pub(in crate::validation) value_types: &'static [&'static str],
    /// A.35.x.3.1.2: the Relationship Content Constraints table, e.g.
    /// "A.35.4-2", and its rows.
    pub(in crate::validation) table: &'static str,
    pub(in crate::validation) relationships: &'static [RelationshipConstraint],
    /// A.35.x.3.1.3: the Template the document is constructed from at its
    /// root, where the IOD names one (TID 2010 for the KOS).
    pub(in crate::validation) root_template: Option<u32>,
}

impl DocumentConstraints {
    /// Whether the table permits a Source of `source` to relate to a Target
    /// of `target` by `relationship`.
    pub(in crate::validation) fn admits(
        &self,
        source: &str,
        relationship: &str,
        target: &str,
    ) -> bool {
        self.relationships
            .iter()
            .any(|row| row.admits(source, relationship, target))
    }
}

/// The Content Constraints of the Structured Reporting IOD at `section`.
pub(in crate::validation) fn document_constraints(
    section: &str,
) -> Option<&'static DocumentConstraints> {
    DOCUMENT_CONSTRAINTS
        .iter()
        .find(|constraints| constraints.iod == section)
}

/// The Content Constraints of the Structured Reporting IOD whose Storage SOP
/// Class is `sop_class`.
pub(in crate::validation) fn document_constraints_of_sop_class(
    sop_class: &str,
) -> Option<&'static DocumentConstraints> {
    IODS.iter()
        .find(|iod| iod.sop_classes.iter().any(|class| class.uid == sop_class))
        .and_then(|iod| document_constraints(iod.section))
}

/// The PS3.16 Template `id` among those the IODs reach: the Templates their
/// Value Sets bind, TID 2010 and TID 1500, and every Template these include.
pub(in crate::validation) fn template(id: u32) -> Option<&'static Template> {
    TEMPLATES
        .binary_search_by_key(&id, |template| template.id)
        .ok()
        .map(|index| &TEMPLATES[index])
}

/// Every Template the IODs reach, sorted by Template ID.
pub(in crate::validation) fn templates() -> &'static [Template] {
    TEMPLATES
}

impl Template {
    /// The rows nested directly under row `index`: the Target Content Items
    /// of the Content Item that row describes.
    pub(in crate::validation) fn children(
        &self,
        index: usize,
    ) -> impl Iterator<Item = &'static TemplateRow> + '_ {
        let rows: &'static [TemplateRow] = self.rows;
        let level = rows.get(index).map_or(u8::MAX, |row| row.level);
        rows.iter()
            .skip(index + 1)
            .take_while(move |row| row.level > level)
            .filter(move |row| row.level == level + 1)
    }
}

impl TemplateRow {
    /// Whether the Content Item `item` has the Value Type and Concept Name
    /// this row constrains; for a by-reference row, `item` is the Target.
    pub(in crate::validation) fn admits(&self, item: &InMemDicomObject) -> bool {
        templates::row_admits(self, item)
    }

    /// The row's Concept Name constraint as messages name it.
    pub(in crate::validation) fn concept_text(&self) -> String {
        templates::row_concept_text(self)
    }
}
