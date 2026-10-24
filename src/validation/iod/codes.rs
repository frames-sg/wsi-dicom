//! PS3.16 Context Group membership of coded entries (PS3.3 8.8, PS3.16 7.2).

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::model::{Code, ContextGroup, Strength};
use super::report::{tag_text, Findings};
use super::scope::string_value;
use super::value_sets::CONTEXT_GROUPS;

/// A coded entry as encoded: the one Code Value Attribute present (Code, Long
/// Code or URN Code Value) and its Coding Scheme Designator when present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::validation) struct Coded {
    pub(super) scheme: Option<String>,
    pub(super) value: String,
    pub(super) meaning: String,
}

impl Coded {
    pub(in crate::validation) fn of(item: &InMemDicomObject) -> Option<Self> {
        let conventional = [tags::CODE_VALUE, tags::LONG_CODE_VALUE]
            .into_iter()
            .find_map(|tag| string_value(item, tag, 1));
        let (scheme, value) = match conventional {
            Some(value) => (
                Some(string_value(item, tags::CODING_SCHEME_DESIGNATOR, 1)?),
                value,
            ),
            None => (
                string_value(item, tags::CODING_SCHEME_DESIGNATOR, 1),
                string_value(item, tags::URN_CODE_VALUE, 1)?,
            ),
        };
        let meaning = string_value(item, tags::CODE_MEANING, 1).unwrap_or_default();
        Some(Self {
            scheme,
            value,
            meaning,
        })
    }

    pub(in crate::validation) fn is(&self, scheme: &str, value: &str) -> bool {
        self.scheme.as_deref() == Some(scheme) && self.value == value
    }

    fn describe(&self) -> String {
        match &self.scheme {
            Some(scheme) => format!("({}, {}, \"{}\")", self.value, scheme, self.meaning),
            None => format!(
                "({}, no Coding Scheme Designator, \"{}\")",
                self.value, self.meaning
            ),
        }
    }
}

pub(super) fn group(id: u32) -> Option<&'static ContextGroup> {
    CONTEXT_GROUPS
        .binary_search_by_key(&id, |group| group.id)
        .ok()
        .map(|index| &CONTEXT_GROUPS[index])
}

/// Whether the group holds the code, by its SNOMED CT identifier or by the
/// SNOMED-RT identifier that the retired "SRT" designator used.
pub(super) fn contains(group: &ContextGroup, coded: &Coded) -> bool {
    let Some(scheme) = coded.scheme.as_deref() else {
        return false;
    };
    if group.schemes.contains(&scheme) {
        return true;
    }
    let found = group
        .codes
        .binary_search_by(|code: &Code| {
            (code.scheme, code.value).cmp(&(scheme, coded.value.as_str()))
        })
        .is_ok();
    found
        || (scheme == "SRT"
            && group
                .codes
                .iter()
                .any(|code| code.scheme == "SCT" && code.legacy == coded.value))
}

/// Checks that each Code Sequence Item holds `code`, which the IOD Content
/// Constraint of PS3.3 `section` requires.
pub(super) fn check_code(
    findings: &mut Findings,
    label: &str,
    tag: u32,
    code: Code,
    section: &str,
    items: &[InMemDicomObject],
) {
    for (index, item) in items.iter().enumerate() {
        let Some(coded) = Coded::of(item) else {
            continue; // the Code Sequence Macro presence checks report it
        };
        if !coded.is(code.scheme, code.value) {
            findings.value_sets.push(|| {
                format!(
                    "{label} {}[{}]: {} is not ({}, {}, \"{}\"), which PS3.3 {section} requires",
                    tag_text(tag),
                    index + 1,
                    coded.describe(),
                    code.value,
                    code.scheme,
                    code.meaning
                )
            });
        }
    }
}

/// How a coded entry departs from the Context Groups it is held to.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::validation) enum CodeFinding {
    /// Outside a Defined non-extensible group.
    Violation(String),
    /// Outside a Defined extensible group without a declared extension, or
    /// outside a Baseline group.
    Advisory(String),
}

/// The departure of the coded entry of the Code Sequence Item `item` from
/// `groups`, described as "<code> is not in Defined CID n \"name\" (PS3.16
/// <edition>)"; `None` for a member, an Item without a coded entry (the Code
/// Sequence Macro presence checks report it), or a declared extension of an
/// extensible group (Context Group Extension Flag "Y", PS3.3 8.7).
pub(in crate::validation) fn code_finding(
    strength: Strength,
    groups: &[u32],
    item: &InMemDicomObject,
) -> Option<CodeFinding> {
    let groups: Vec<&ContextGroup> = groups.iter().filter_map(|id| group(*id)).collect();
    let coded = Coded::of(item)?;
    if groups.iter().any(|group| contains(group, &coded)) {
        return None;
    }
    let names = groups
        .iter()
        .map(|group| format!("CID {} \"{}\"", group.id, group.name))
        .collect::<Vec<_>>()
        .join(" or ");
    let message = format!(
        "{} is not in {} {names} (PS3.16 {})",
        coded.describe(),
        match strength {
            Strength::Baseline => "Baseline",
            Strength::Defined => "Defined",
        },
        super::tables::EDITION
    );
    let extensible = groups.iter().any(|group| group.extensible);
    let declared =
        string_value(item, tags::CONTEXT_GROUP_EXTENSION_FLAG, 1).as_deref() == Some("Y");
    match strength {
        Strength::Defined if !extensible => Some(CodeFinding::Violation(message)),
        Strength::Defined if declared => None,
        Strength::Defined => Some(CodeFinding::Advisory(format!(
            "{message}; an extension of an extensible group is declared with Context Group Extension Flag (0008,010B) \"Y\""
        ))),
        Strength::Baseline => Some(CodeFinding::Advisory(message)),
    }
}

/// Checks the coded entry of each Code Sequence Item against `groups`
/// (`code_finding`).
pub(super) fn check_items(
    findings: &mut Findings,
    label: &str,
    tag: u32,
    strength: Strength,
    groups: &[u32],
    items: &[InMemDicomObject],
) {
    for (index, item) in items.iter().enumerate() {
        let at = || format!("{label} {}[{}]", tag_text(tag), index + 1);
        match code_finding(strength, groups, item) {
            Some(CodeFinding::Violation(message)) => {
                findings.value_sets.push(|| format!("{}: {message}", at()));
            }
            Some(CodeFinding::Advisory(message)) => {
                findings.advisories.add(format!("{}: {message}", at()));
            }
            None => {}
        }
    }
}

/// The name of Context Group `id` among those the IODs reach.
pub(in crate::validation) fn context_group_name(id: u32) -> Option<&'static str> {
    group(id).map(|group| group.name)
}
