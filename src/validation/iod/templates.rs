//! PS3.16 Template conformance of Content Item Sequences (PS3.16 6.1 and 6.2).
//!
//! Each Content Item is matched to the first Template row with its Value Type
//! and Concept Name; rows of an INCLUDE row's Template are gated by that row.

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::codes::{self, Coded};
use super::deidentification::{
    structured_content_concept, ConfidentialityOption, DeidentificationAction,
};
use super::model::{
    Concept, RowRequirement, Strength, Template, TemplateCondition, TemplateRow, ValueSet,
};
use super::report::Findings;
use super::scope::{items as sequence_items, raw_tag, string_value};
use super::value_sets::TEMPLATES;
use crate::validation::disclosure::Nesting;

/// Bound on INCLUDE nesting; the Templates the IOD reaches nest twice.
const MAX_INCLUDE_DEPTH: usize = 8;

fn find(id: u32) -> Option<&'static Template> {
    TEMPLATES
        .binary_search_by_key(&id, |template| template.id)
        .ok()
        .map(|index| &TEMPLATES[index])
}

/// A row of the expanded Template, with the INCLUDE row that brought it in.
#[derive(Clone, Copy)]
struct Expanded {
    template: &'static Template,
    row: &'static TemplateRow,
    include: Option<(&'static Template, &'static TemplateRow)>,
}

fn expand(
    template: &'static Template,
    include: Option<(&'static Template, &'static TemplateRow)>,
    depth: usize,
    out: &mut Vec<Expanded>,
) {
    // A nested row describes a Target Content Item of the row above it, in
    // that Item's own Content Sequence, which this flat matching of one
    // Content Item Sequence does not walk.
    for row in template.rows.iter().filter(|row| row.level == 0) {
        match row.concept {
            Concept::Set(ValueSet::Template(_, id)) if row.value_type == "INCLUDE" => {
                if let Some(included) = find(id).filter(|_| depth < MAX_INCLUDE_DEPTH) {
                    expand(included, Some((template, row)), depth + 1, out);
                }
            }
            _ => out.push(Expanded {
                template,
                row,
                include,
            }),
        }
    }
}

/// Whether the Content Item `object` has the Value Type and Concept Name
/// `row` constrains.
pub(super) fn row_admits(row: &TemplateRow, object: &InMemDicomObject) -> bool {
    Item::of(object).matches(row)
}

/// The Concept Name constraint of `row` as messages name it, e.g. "(121041,
/// DCM, \"Specimen Identifier\")" or "CID 7010".
pub(super) fn row_concept_text(row: &TemplateRow) -> String {
    concept_text(row.concept)
}

/// The Value Type and Concept Name of one Content Item.
struct Item<'a> {
    object: &'a InMemDicomObject,
    value_type: Option<String>,
    concept: Option<Coded>,
}

impl<'a> Item<'a> {
    fn of(object: &'a InMemDicomObject) -> Self {
        Self {
            object,
            value_type: string_value(object, tags::VALUE_TYPE, 1),
            concept: sequence_items(object, tags::CONCEPT_NAME_CODE_SEQUENCE)
                .first()
                .and_then(Coded::of),
        }
    }

    /// The code a CODE Content Item conveys.
    fn value(&self) -> Option<Coded> {
        sequence_items(self.object, tags::CONCEPT_CODE_SEQUENCE)
            .first()
            .and_then(Coded::of)
    }

    fn matches(&self, row: &TemplateRow) -> bool {
        if self.value_type.as_deref() != Some(row.value_type) {
            return false;
        }
        // A row without a Concept Name constraint, or whose Concept Name the
        // including row passes as a parameter, admits any.
        if matches!(row.concept, Concept::Any | Concept::Parameter(_)) {
            return true;
        }
        let Some(concept) = &self.concept else {
            return false;
        };
        match row.concept {
            Concept::Enumerated(code) | Concept::Defined(code) => {
                concept.is(code.scheme, code.value)
            }
            Concept::Set(ValueSet::ContextGroups(_, groups)) => groups
                .iter()
                .filter_map(|id| codes::group(*id))
                .any(|group| codes::contains(group, concept)),
            Concept::Set(ValueSet::Code(code, _)) => concept.is(code.scheme, code.value),
            Concept::Set(ValueSet::Template(..)) => false,
            Concept::Any | Concept::Parameter(_) => true,
        }
    }
}

/// The PS3.15 Annex E Options the De-identification Method Code Sequence
/// (0012,0064) of `object` declares, when Clean Structured Content is among
/// them: its Content Items are then cleaned by Table E.3.4-1 (PS3.15 E.3.4).
fn cleaned_options(object: &InMemDicomObject) -> Option<Vec<ConfidentialityOption>> {
    let options: Vec<ConfidentialityOption> =
        sequence_items(object, tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE)
            .iter()
            .filter_map(Coded::of)
            .filter_map(|code| {
                ConfidentialityOption::from_code(code.scheme.as_deref()?, &code.value)
            })
            .collect();
    options
        .contains(&ConfidentialityOption::CleanStructuredContent)
        .then_some(options)
}

/// Whether Table E.3.4-1 removes the Content Item `row` describes from an
/// object cleaned under `options`: X under the Basic Profile, and no
/// declared Option naming another action for it.
fn removed(row: &TemplateRow, options: &[ConfidentialityOption]) -> bool {
    let (Concept::Enumerated(code) | Concept::Defined(code)) = row.concept else {
        return false;
    };
    structured_content_concept(code.scheme, code.value, row.value_type).is_some_and(|concept| {
        concept.basic == DeidentificationAction::X
            && options
                .iter()
                .filter_map(|option| concept.action(*option))
                .all(|action| action == DeidentificationAction::X)
    })
}

/// A departure of a Content Item Sequence from its Template.
enum Departure {
    /// From the Template's rows, counts or order.
    Template(String),
    /// A required Content Item missing that Table E.3.4-1 removes under the
    /// Clean Structured Content Option the object declares, with the ID of
    /// the Template whose row requires it.
    Cleaned(String, u32),
}

/// Whether a row is required, permitted or forbidden.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Need {
    Required,
    Optional,
    Forbidden,
}

/// Content Items assigned to the rows of an expanded Template.
struct Matching<'a> {
    rows: Vec<Expanded>,
    items: Vec<Item<'a>>,
    assigned: Vec<Option<usize>>,
}

impl<'a> Matching<'a> {
    fn new(root: &'static Template, objects: &'a [InMemDicomObject]) -> Self {
        let mut rows = Vec::new();
        expand(root, None, 0, &mut rows);
        let items: Vec<Item<'a>> = objects.iter().map(Item::of).collect();
        let assigned = items
            .iter()
            .map(|item| rows.iter().position(|expanded| item.matches(expanded.row)))
            .collect();
        Self {
            rows,
            items,
            assigned,
        }
    }

    fn count(&self, index: usize) -> u32 {
        let count = self
            .assigned
            .iter()
            .filter(|row| **row == Some(index))
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    /// Items assigned to row `name` of `template`.
    fn matched<'s>(
        &'s self,
        template: &'s Template,
        name: &'s str,
    ) -> impl Iterator<Item = &'s Item<'a>> + 's {
        self.items
            .iter()
            .zip(&self.assigned)
            .filter_map(move |(item, row)| {
                let expanded = self.rows[(*row)?];
                (std::ptr::eq(expanded.template, template) && expanded.row.row == name)
                    .then_some(item)
            })
    }

    fn holds(&self, template: &Template, condition: TemplateCondition) -> bool {
        match condition {
            TemplateCondition::Absent { row, .. } => self.matched(template, row).next().is_none(),
            TemplateCondition::Present { row, .. } => self.matched(template, row).next().is_some(),
            TemplateCondition::HasCode {
                row, scheme, value, ..
            } => self
                .matched(template, row)
                .any(|item| item.value().is_some_and(|coded| coded.is(scheme, value))),
        }
    }

    fn need(&self, template: &Template, row: &TemplateRow) -> Need {
        let met = row
            .condition
            .map(|condition| self.holds(template, condition));
        let iff = matches!(
            row.condition,
            Some(
                TemplateCondition::Absent { iff: true, .. }
                    | TemplateCondition::Present { iff: true, .. }
                    | TemplateCondition::HasCode { iff: true, .. }
            )
        );
        // PS3.16 6.1.7: MC and UC rows are absent when an IFF condition fails.
        match (row.requirement, met) {
            (RowRequirement::M, _) | (RowRequirement::Mc, Some(true)) => Need::Required,
            (RowRequirement::Mc | RowRequirement::Uc, Some(false)) if iff => Need::Forbidden,
            _ => Need::Optional,
        }
    }

    /// Departures from the rows' requirements, counts, extensibility and
    /// order, in an object cleaned under `cleaned` Options when it declares
    /// Clean Structured Content.
    fn departures(
        &self,
        root: &Template,
        cleaned: Option<&[ConfidentialityOption]>,
    ) -> Vec<Departure> {
        let mut departures = Vec::new();
        for (index, expanded) in self.rows.iter().enumerate() {
            let gate = expanded
                .include
                .map_or(Need::Required, |(outer, include)| self.need(outer, include));
            let own = self.need(expanded.template, expanded.row);
            let effective = match (gate, own) {
                (Need::Forbidden, _) | (_, Need::Forbidden) => Need::Forbidden,
                (Need::Required, Need::Required) => Need::Required,
                _ => Need::Optional,
            };
            let row = expanded.row;
            let count = self.count(index);
            // The message names the root Template; only included rows name theirs.
            let what = || {
                let template = if std::ptr::eq(expanded.template, root) {
                    String::new()
                } else {
                    format!("TID {} ", expanded.template.id)
                };
                format!(
                    "{template}Row {} {} {}",
                    row.row,
                    row.value_type,
                    concept_text(row.concept)
                )
            };
            match effective {
                Need::Required if count < row.min.max(1) => {
                    let missing = format!("{} missing", what());
                    departures.push(match cleaned {
                        Some(options) if count == 0 && removed(row, options) => {
                            Departure::Cleaned(missing, expanded.template.id)
                        }
                        _ => Departure::Template(missing),
                    });
                }
                Need::Forbidden if count > 0 => {
                    departures.push(Departure::Template(format!(
                        "{} present although its condition is not met",
                        what()
                    )));
                }
                _ => {}
            }
            if row.max.is_some_and(|max| count > max) {
                departures.push(Departure::Template(format!(
                    "{} appears {count} times",
                    what()
                )));
            }
        }
        if !self
            .rows
            .iter()
            .all(|expanded| expanded.template.extensible)
        {
            for (position, _) in self
                .assigned
                .iter()
                .enumerate()
                .filter(|(_, row)| row.is_none())
            {
                departures.push(Departure::Template(format!(
                    "Content Item [{}] matches no row",
                    position + 1
                )));
            }
        }
        if root.ordered && !self.assigned.iter().flatten().is_sorted() {
            departures.push(Departure::Template(
                "Content Items are not in Template row order".into(),
            ));
        }
        departures
    }
}

/// Checks a Content Item Sequence of `object` against Template `id`.
/// Departures from a Defined Template are violations; from a Baseline
/// Template, advisories, and a Baseline Template makes its rows' Value Sets
/// suggestions too. A required Content Item that PS3.15 Table E.3.4-1 removes
/// from an object declaring the Clean Structured Content Option is an
/// advisory citing both: the conflict is between the two Parts. `nesting`
/// is that of the Content Items (`disclosure::Nesting::below`).
pub(super) fn check_items(
    findings: &mut Findings,
    label: &str,
    strength: Strength,
    id: u32,
    objects: &[InMemDicomObject],
    object: &InMemDicomObject,
    nesting: Nesting,
) {
    let Some(root) = find(id) else {
        return;
    };
    let edition = super::tables::EDITION;
    let cleaned = cleaned_options(object);
    let matching = Matching::new(root, objects);
    for departure in matching.departures(root, cleaned.as_deref()) {
        let name = root.name;
        match departure {
            Departure::Template(departure) => {
                let message = format!("{label}: TID {id} \"{name}\" {departure} (PS3.16 {edition})");
                match strength {
                    Strength::Defined => findings.value_sets.push(|| message),
                    Strength::Baseline => findings.advisories.add(message),
                }
            }
            Departure::Cleaned(departure, template) => findings.advisories.add(format!(
                "{label}: TID {id} \"{name}\" {departure}, a Content Item PS3.15 {edition} Table E.3.4-1 removes under the Clean Structured Content Option the De-identification Method Code Sequence (0012,0064) declares (PS3.15 {edition} E.3.4; PS3.16 {edition} TID {template})"
            )),
        }
    }
    for (position, (item, row)) in matching.items.iter().zip(&matching.assigned).enumerate() {
        let Some(row) = row.map(|index| matching.rows[index].row) else {
            continue;
        };
        if let (Some(ValueSet::ContextGroups(row_strength, groups)), "CODE") =
            (row.constraint, row.value_type)
        {
            let effective = match strength {
                Strength::Baseline => Strength::Baseline,
                Strength::Defined => row_strength,
            };
            let codes = nesting
                .content_item(item.object)
                .below(tags::CONCEPT_CODE_SEQUENCE);
            codes::check_items(
                findings,
                (
                    &format!("{label}[{}] > Concept Code Sequence", position + 1),
                    raw_tag(tags::CONCEPT_CODE_SEQUENCE),
                ),
                (effective, groups),
                sequence_items(item.object, tags::CONCEPT_CODE_SEQUENCE),
                codes,
            );
        }
    }
}

fn concept_text(concept: Concept) -> String {
    match concept {
        Concept::Enumerated(code) | Concept::Defined(code) => {
            format!("({}, {}, \"{}\")", code.value, code.scheme, code.meaning)
        }
        Concept::Set(ValueSet::ContextGroups(_, groups)) => groups
            .iter()
            .map(|id| format!("CID {id}"))
            .collect::<Vec<_>>()
            .join(" or "),
        Concept::Set(ValueSet::Template(_, id)) => format!("TID {id}"),
        Concept::Set(ValueSet::Code(code, _)) => {
            format!("({}, {}, \"{}\")", code.value, code.scheme, code.meaning)
        }
        Concept::Any => "of any Concept Name".to_string(),
        Concept::Parameter(name) => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{removed, ConfidentialityOption as O};
    use crate::validation::iod::model::{Code, Concept, RowRequirement, TemplateRow};

    fn row(scheme: &'static str, value: &'static str, value_type: &'static str) -> TemplateRow {
        TemplateRow {
            row: "1",
            level: 0,
            relationship: "",
            value_type,
            concept: Concept::Enumerated(Code {
                scheme,
                value,
                meaning: "",
                legacy: "",
            }),
            min: 1,
            max: Some(1),
            requirement: RowRequirement::M,
            condition: None,
            condition_text: "",
            constraint: None,
            constraint_text: "",
        }
    }

    #[test]
    fn table_e_3_4_1_removes_what_no_declared_option_keeps() {
        let clean = [O::CleanStructuredContent];
        // Unique Device Identifier: X, kept under Retain Device Identity.
        let device = row("LN", "74711-3", "TEXT");
        assert!(removed(&device, &clean));
        assert!(!removed(
            &device,
            &[O::CleanStructuredContent, O::RetainDeviceIdentity]
        ));
        // Device Observer UID is X/D: a dummy keeps a required Item.
        assert!(!removed(&row("DCM", "121012", "UIDREF"), &clean));
        // Table E.3.4-1 names Specimen Identifier as TEXT alone.
        assert!(removed(&row("DCM", "121041", "TEXT"), &clean));
        assert!(!removed(&row("DCM", "121041", "CODE"), &clean));
        // Processing type is not in Table E.3.4-1.
        assert!(!removed(&row("DCM", "111701", "CODE"), &clean));
    }
}
