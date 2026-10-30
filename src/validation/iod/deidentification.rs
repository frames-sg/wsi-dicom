//! PS3.15 Annex E Attribute Confidentiality Profiles as data, for the rule
//! that holds a de-identified object (Patient Identity Removed (0012,0062)
//! YES) to the Basic Profile and the Options its De-identification Method Code
//! Sequence (0012,0064) declares.
//!
//! `confidentiality.rs` is generated from PS3.15 Tables E.1-1a, E.1-1 and
//! E.3.4-1 and CID 7050; a changed action code or Option fails to compile
//! wherever it is matched.

use dicom_core::Tag;

use super::codes;
use super::confidentiality::{
    CONFIDENTIALITY_ATTRIBUTES, CONFIDENTIALITY_PATTERNS, STRUCTURED_CONTENT_CONCEPTS,
};
use super::scope::raw_tag;

pub(in crate::validation) use super::confidentiality::{
    ConfidentialityOption, DeidentificationAction, BASIC_PROFILE, BASIC_PROFILE_SECTION,
};
pub(in crate::validation) use super::model::Code;

/// CID 7050 De-identification Method.
const DEIDENTIFICATION_METHODS: u32 = 7050;

/// The PS3.15 Table E.1-1 row of `tag`: the row naming it, else the row of
/// its repeating group (Curve Data, Overlay Data, Overlay Comments), else for
/// an odd group the Private Attributes row. `None` when the table does not
/// list the Attribute, which the Profile then leaves as it is.
pub(in crate::validation) fn confidentiality_attribute(
    tag: Tag,
) -> Option<&'static ConfidentialityAttribute> {
    let raw = raw_tag(tag);
    CONFIDENTIALITY_ATTRIBUTES
        .binary_search_by_key(&raw, |row| match row.tag {
            TagPattern::Tag(own) => own,
            TagPattern::Repeating { tag, .. } => tag,
            TagPattern::Private => u32::MAX,
        })
        .ok()
        .map(|index| &CONFIDENTIALITY_ATTRIBUTES[index])
        .or_else(|| {
            CONFIDENTIALITY_PATTERNS
                .iter()
                .find(|row| row.tag.matches(raw))
        })
}

/// Every row of PS3.15 Table E.1-1: those naming one tag, sorted by tag,
/// then those naming a repeating group or every Private Attribute.
pub(in crate::validation) fn confidentiality_attributes(
) -> impl Iterator<Item = &'static ConfidentialityAttribute> {
    CONFIDENTIALITY_ATTRIBUTES
        .iter()
        .chain(CONFIDENTIALITY_PATTERNS)
}

/// The PS3.15 Table E.3.4-1 row for Content Items whose Concept Name is
/// (`value`, `scheme`) and whose Value Type is `value_type`.
pub(in crate::validation) fn structured_content_concept(
    scheme: &str,
    value: &str,
    value_type: &str,
) -> Option<&'static StructuredContentConcept> {
    STRUCTURED_CONTENT_CONCEPTS
        .binary_search_by(|row| {
            (row.code.scheme, row.code.value, row.value_type).cmp(&(scheme, value, value_type))
        })
        .ok()
        .map(|index| &STRUCTURED_CONTENT_CONCEPTS[index])
}

/// Every row of PS3.15 Table E.3.4-1, sorted by Coding Scheme Designator,
/// Code Value and Value Type.
pub(in crate::validation) fn structured_content_concepts() -> &'static [StructuredContentConcept] {
    STRUCTURED_CONTENT_CONCEPTS
}

/// The codes of CID 7050 De-identification Method: the Basic Profile's and
/// each Option's.
pub(in crate::validation) fn deidentification_methods() -> &'static [Code] {
    codes::group(DEIDENTIFICATION_METHODS).map_or(&[], |group| group.codes)
}

impl ConfidentialityOption {
    /// The Option a CID 7050 code names.
    pub(in crate::validation) fn from_code(scheme: &str, value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|option| option.code().scheme == scheme && option.code().value == value)
    }
}

/// The Data Elements one PS3.15 Table E.1-1 row names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::validation) enum TagPattern {
    /// One tag.
    Tag(u32),
    /// Every tag whose bits under `mask` equal `tag`, e.g. (60xx,4000).
    Repeating { tag: u32, mask: u32 },
    /// Every Private Data Element: "(gggg,eeee) where gggg is odd".
    Private,
}

impl TagPattern {
    pub(in crate::validation) fn matches(self, tag: u32) -> bool {
        match self {
            Self::Tag(own) => own == tag,
            Self::Repeating { tag: own, mask } => tag & mask == own,
            Self::Private => (tag >> 16) % 2 == 1,
        }
    }
}

/// One row of PS3.15 Table E.1-1 Application Level Confidentiality Profile
/// Attributes: what the Basic Profile does to the Attribute, and each Option
/// whose column names another action. An Option's action, when the Option is
/// implemented, overrides the Basic Profile's (PS3.15 E.1.1).
#[derive(Debug)]
pub(in crate::validation) struct ConfidentialityAttribute {
    pub(in crate::validation) name: &'static str,
    pub(in crate::validation) tag: TagPattern,
    /// Retired in PS3.6.
    pub(in crate::validation) retired: bool,
    /// In a Standard Composite IOD of PS3.3.
    pub(in crate::validation) in_standard_iod: bool,
    pub(in crate::validation) basic: DeidentificationAction,
    /// In the table's column order.
    pub(in crate::validation) options: &'static [(ConfidentialityOption, DeidentificationAction)],
}

impl ConfidentialityAttribute {
    /// The action `option` prescribes, if its column names one.
    pub(in crate::validation) fn action(
        &self,
        option: ConfidentialityOption,
    ) -> Option<DeidentificationAction> {
        option_action(self.options, option)
    }
}

/// One row of PS3.15 Table E.3.4-1: what the Clean Structured Content Option
/// (E.3.4) does to Content Items with this Concept Name and Value Type, under
/// the Basic Profile and each Option whose column names another action.
#[derive(Debug)]
pub(in crate::validation) struct StructuredContentConcept {
    pub(in crate::validation) code: Code,
    /// The Coding Scheme Version the table names, e.g. "2.0b"; empty if none.
    pub(in crate::validation) scheme_version: &'static str,
    /// The Content Item Value Type (0040,A040), e.g. "TEXT".
    pub(in crate::validation) value_type: &'static str,
    /// Retired in PS3.16.
    pub(in crate::validation) retired: bool,
    /// In a Standard Template of PS3.16.
    pub(in crate::validation) in_standard_template: bool,
    pub(in crate::validation) basic: DeidentificationAction,
    /// In the table's column order.
    pub(in crate::validation) options: &'static [(ConfidentialityOption, DeidentificationAction)],
}

impl StructuredContentConcept {
    /// The action `option` prescribes, if its column names one.
    pub(in crate::validation) fn action(
        &self,
        option: ConfidentialityOption,
    ) -> Option<DeidentificationAction> {
        option_action(self.options, option)
    }
}

fn option_action(
    options: &[(ConfidentialityOption, DeidentificationAction)],
    option: ConfidentialityOption,
) -> Option<DeidentificationAction> {
    options
        .iter()
        .find(|(own, _)| *own == option)
        .map(|(_, action)| *action)
}
