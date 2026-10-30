//! The Requirement Types and conditions of Template rows (PS3.16 6.1.7 and
//! 6.1.8), evaluated over the Content Items one level matched.

use dicom_dictionary_std::tags;

use super::matcher::{Level, Matching, Slot};
use super::{code_is, prefix, Report};
use crate::validation::iod::items;
use crate::validation::iod::structured_report::{RowRequirement, Template, TemplateRow};
use crate::validation::structured_report::tree::Tree;
use crate::validation::structured_report::PS3_16;

/// How a row's Requirement Type and condition apply.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Need {
    Required,
    Optional,
    Forbidden,
    Undetermined,
}

/// A row condition of the forms PS3.16 Templates use.
enum Condition {
    /// "XOR Row(s) a, b": exactly one, for MC rows, or at most one of them.
    Xor(Vec<&'static str>),
    /// "At least one of Rows a, b and c shall be present".
    AtLeastOne(Vec<&'static str>),
    /// "IF[F] Row a [and Row b] [is|are] absent", "IF Row a not present".
    Absent(bool, Vec<&'static str>),
    /// "IFF Row a": present.
    Present(bool, Vec<&'static str>),
    /// "IF[F] Row a Concept Name = (code) [or (code)]".
    ConceptName(
        bool,
        &'static str,
        Vec<(Option<&'static str>, &'static str)>,
    ),
    /// "IFF Row a value = (code) [or Row a is absent]", "IFF Row a ...
    /// value is (code)", "IFF Row a is present with a value of \"PT\"".
    Value(
        bool,
        &'static str,
        Vec<(Option<&'static str>, &'static str)>,
        bool,
    ),
    Unknown,
}

fn row_names(text: &'static str) -> Vec<&'static str> {
    text.split([',', ' '])
        .map(|word| word.trim_matches(|c: char| c == '.' || c == '(' || c == ')'))
        .filter(|word| !word.is_empty() && word.starts_with(|c: char| c.is_ascii_digit()))
        .collect()
}

/// The codes "(value, scheme, \"meaning\")" a condition names, or quoted
/// values "\"PT\"" without a scheme.
fn codes(text: &'static str) -> Vec<(Option<&'static str>, &'static str)> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('(') {
        let Some(end) = rest[start..].find(')') else {
            break;
        };
        let mut fields = rest[start + 1..start + end].split(',').map(str::trim);
        if let (Some(value), Some(scheme)) = (fields.next(), fields.next()) {
            found.push((Some(scheme), value));
        }
        rest = &rest[start + end + 1..];
    }
    if found.is_empty() {
        found.extend(
            text.split('"')
                .skip(1)
                .step_by(2)
                .filter(|value| !value.is_empty())
                .map(|value| (None, value)),
        );
    }
    found
}

fn condition(text: &'static str) -> Option<Condition> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (iff, body) = if let Some(body) = text.strip_prefix("IFF ") {
        (true, body)
    } else if let Some(body) = text.strip_prefix("IF ") {
        (false, body)
    } else {
        (false, text)
    };
    Some(if let Some(rows) = text.strip_prefix("XOR ") {
        if rows.contains(" and ") {
            Condition::Unknown
        } else {
            Condition::Xor(row_names(rows))
        }
    } else if let Some(rows) = text.strip_prefix("At least one of ") {
        Condition::AtLeastOne(row_names(
            rows.trim_end_matches(" shall be present.")
                .trim_end_matches(" shall be present"),
        ))
    } else if !body.starts_with("Row ") {
        Condition::Unknown
    } else if body.contains("Concept Name =") {
        let row = row_names(&body[4..]).first().copied().unwrap_or_default();
        Condition::ConceptName(iff, row, codes(body))
    } else if body.contains(" value") {
        let row = row_names(&body[4..]).first().copied().unwrap_or_default();
        Condition::Value(iff, row, codes(body), body.ends_with("is absent"))
    } else if body.ends_with(" absent") || body.ends_with(" not present") {
        Condition::Absent(iff, row_names(body))
    } else if body.ends_with(" present")
        || body.split_whitespace().all(|word| {
            matches!(word, "Row" | "Rows" | "and") || word.starts_with(|c: char| c.is_ascii_digit())
        })
    {
        Condition::Present(iff, row_names(body))
    } else {
        Condition::Unknown
    })
}

impl Need {
    /// The requirement of a row under an INCLUDE gate.
    pub(super) fn and(self, own: Self) -> Self {
        match (self, own) {
            (Self::Forbidden, _) | (_, Self::Forbidden) => Self::Forbidden,
            (Self::Required, Self::Required) => Self::Required,
            (Self::Undetermined, _) | (_, Self::Undetermined) => Self::Undetermined,
            _ => Self::Optional,
        }
    }
}

/// One level's matching, for evaluating row conditions.
pub(super) struct Context<'c, 't, 'a> {
    pub(super) tree: &'t Tree<'a>,
    pub(super) parent: usize,
    pub(super) owner: Slot,
    pub(super) level: &'c Level,
    pub(super) matching: &'c Matching,
    pub(super) children: &'c [usize],
}

impl Context<'_, '_, '_> {
    /// The Content Items matched to row `name` of `template` at this level,
    /// an INCLUDE row counting once when its Template is invoked.
    fn count(&self, template: &'static Template, name: &str) -> u32 {
        let slots: u32 = self
            .level
            .slots
            .iter()
            .zip(&self.matching.counts)
            .filter(|(slot, _)| std::ptr::eq(slot.template, template) && slot.row().row == name)
            .map(|(_, count)| *count)
            .sum();
        let includes = self
            .level
            .includes
            .iter()
            .enumerate()
            .filter(|(index, include)| {
                std::ptr::eq(include.template, template)
                    && include.template.rows[include.index].row == name
                    && self.instantiated(*index)
            })
            .count();
        slots + u32::try_from(includes).unwrap_or(u32::MAX)
    }

    /// Whether row `name` of `template` is a row at this level.
    fn defines(&self, template: &'static Template, name: &str) -> bool {
        self.level
            .slots
            .iter()
            .any(|slot| std::ptr::eq(slot.template, template) && slot.row().row == name)
            || self.level.includes.iter().any(|include| {
                std::ptr::eq(include.template, template)
                    && include.template.rows[include.index].row == name
            })
    }

    /// Whether any row the INCLUDE `index` brought in matched a Content Item.
    fn instantiated(&self, index: usize) -> bool {
        self.level
            .slots
            .iter()
            .zip(&self.matching.counts)
            .any(|(slot, count)| *count > 0 && self.within(slot.include, index))
    }

    /// Whether `include` is INCLUDE `index` or nested in it.
    fn within(&self, include: Option<usize>, index: usize) -> bool {
        let mut current = include;
        while let Some(found) = current {
            if found == index {
                return true;
            }
            current = self.level.includes[found].parent;
        }
        false
    }

    /// The requirement the INCLUDE rows above a row put on it: required
    /// where each is required or invoked, forbidden where one is forbidden.
    pub(super) fn gate(&self, include: Option<usize>) -> Need {
        let Some(index) = include else {
            return Need::Required;
        };
        let found = self.level.includes[index];
        let row = &found.template.rows[found.index];
        let own = match self.own(found.template, row) {
            Need::Optional | Need::Undetermined if self.instantiated(index) => Need::Required,
            other => other,
        };
        self.gate(found.parent).and(own)
    }

    /// A row's own requirement; XOR and At least one conditions are held as
    /// groups, recorded in `groups`.
    pub(super) fn need(
        &self,
        template: &'static Template,
        row: &'static TemplateRow,
        groups: &mut Vec<usize>,
        slot: usize,
    ) -> Need {
        if let Some(Condition::Xor(_) | Condition::AtLeastOne(_)) = condition(row.condition_text) {
            if matches!(row.requirement, RowRequirement::Mc | RowRequirement::Uc) {
                groups.push(slot);
                return Need::Optional;
            }
        }
        self.own(template, row)
    }

    fn own(&self, template: &'static Template, row: &'static TemplateRow) -> Need {
        let conditional = matches!(row.requirement, RowRequirement::Mc | RowRequirement::Uc);
        let parsed = condition(row.condition_text);
        let (met, iff) = match (&parsed, conditional) {
            (_, false) | (None, _) => {
                return if row.requirement == RowRequirement::M {
                    Need::Required
                } else {
                    Need::Optional
                };
            }
            (Some(parsed), true) => self.met(template, parsed),
        };
        match (row.requirement, met) {
            (RowRequirement::Mc, Some(true)) => Need::Required,
            (_, Some(false)) if iff => Need::Forbidden,
            (_, None) if iff || row.requirement == RowRequirement::Mc => Need::Undetermined,
            _ => Need::Optional,
        }
    }

    /// Whether a condition holds (`None`: undeterminable), and whether it is
    /// an IFF condition.
    fn met(&self, template: &'static Template, parsed: &Condition) -> (Option<bool>, bool) {
        match parsed {
            Condition::Absent(iff, rows) => (self.all(template, rows, false), *iff),
            Condition::Present(iff, rows) => (self.all(template, rows, true), *iff),
            Condition::ConceptName(iff, row, codes) => {
                (self.concept_name(template, row, codes), *iff)
            }
            Condition::Value(iff, row, codes, or_absent) => {
                (self.value(template, row, codes, *or_absent), *iff)
            }
            Condition::Xor(_) | Condition::AtLeastOne(_) | Condition::Unknown => (None, false),
        }
    }

    /// Whether each named row is present (or absent), when all are rows at
    /// this level.
    fn all(&self, template: &'static Template, rows: &[&str], present: bool) -> Option<bool> {
        if rows.is_empty() || !rows.iter().all(|row| self.defines(template, row)) {
            return None;
        }
        Some(
            rows.iter()
                .all(|row| (self.count(template, row) > 0) == present),
        )
    }

    /// The Content Item row `name` describes: the parent, when it matched
    /// that row, or the first one at this level.
    fn item_of(&self, template: &'static Template, name: &str) -> Option<Option<usize>> {
        if std::ptr::eq(self.owner.template, template) && self.owner.row().row == name {
            return Some(self.tree.target(self.parent));
        }
        if !self.defines(template, name) {
            return None;
        }
        Some(
            self.children
                .iter()
                .zip(&self.matching.assigned)
                .find(|(_, assigned)| {
                    assigned.is_some_and(|found| {
                        let slot = self.level.slots[found];
                        std::ptr::eq(slot.template, template) && slot.row().row == name
                    })
                })
                .and_then(|(child, _)| self.tree.target(*child)),
        )
    }

    fn concept_name(
        &self,
        template: &'static Template,
        name: &str,
        codes: &[(Option<&str>, &str)],
    ) -> Option<bool> {
        let item = self.item_of(template, name)?;
        Some(item.is_some_and(|node| {
            items(
                self.tree.nodes[node].object,
                tags::CONCEPT_NAME_CODE_SEQUENCE,
            )
            .first()
            .is_some_and(|code| {
                codes
                    .iter()
                    .any(|(scheme, value)| code_is(code, *scheme, value))
            })
        }))
    }

    fn value(
        &self,
        template: &'static Template,
        name: &str,
        codes: &[(Option<&str>, &str)],
        or_absent: bool,
    ) -> Option<bool> {
        let item = self.item_of(template, name)?;
        Some(match item {
            None => or_absent,
            Some(node) => items(self.tree.nodes[node].object, tags::CONCEPT_CODE_SEQUENCE)
                .first()
                .is_some_and(|code| {
                    codes
                        .iter()
                        .any(|(scheme, value)| code_is(code, *scheme, value))
                }),
        })
    }

    /// The XOR and "At least one of" groups: exactly or at least one of the
    /// rows present where the group is required, at most one for XOR.
    pub(super) fn groups(
        &self,
        groups: &[usize],
        report: &mut dyn Report,
        root: Option<&'static Template>,
    ) {
        let at = self.tree.at(self.parent);
        let mut seen: Vec<(usize, Vec<&'static str>)> = Vec::new();
        for slot in groups {
            let slot = self.level.slots[*slot];
            let row = slot.row();
            let Some(parsed) = condition(row.condition_text) else {
                continue;
            };
            let (exclusive, others) = match parsed {
                Condition::Xor(others) => (true, others),
                Condition::AtLeastOne(rows) => (false, rows),
                _ => continue,
            };
            let mut members: Vec<&'static str> = others;
            if !members.contains(&row.row) {
                members.push(row.row);
            }
            let position = |name: &&str| {
                slot.template
                    .rows
                    .iter()
                    .position(|candidate| candidate.row == *name)
            };
            members.sort_by_key(position);
            members.dedup();
            let key = (std::ptr::from_ref(slot.template) as usize, members.clone());
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let present = members
                .iter()
                .filter(|member| self.count(slot.template, member) > 0)
                .count();
            let required =
                row.requirement == RowRequirement::Mc && self.gate(slot.include) == Need::Required;
            let rows = members.join(", ");
            let template = slot.template;
            if present == 0 && required {
                report.departure(slot.baseline, &|| {
                    format!(
                        "{at}: {}{} of Rows {rows} shall be present; none is ({PS3_16})",
                        prefix(root, template),
                        if exclusive {
                            "exactly one"
                        } else {
                            "at least one"
                        }
                    )
                });
            } else if exclusive && present > 1 {
                report.departure(slot.baseline, &|| {
                    format!(
                        "{at}: {}Rows {rows} are exclusive (XOR), but {present} are present ({PS3_16})",
                        prefix(root, template)
                    )
                });
            }
        }
    }
}
