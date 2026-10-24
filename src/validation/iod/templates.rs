//! PS3.16 Template conformance of Content Item Sequences (PS3.16 6.1 and 6.2).
//!
//! Each Content Item is matched to the first Template row with its Value Type
//! and Concept Name; rows of an INCLUDE row's Template are gated by that row.

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::codes::{self, Coded};
use super::model::{
    Concept, RowRequirement, Strength, Template, TemplateCondition, TemplateRow, ValueSet,
};
use super::report::Findings;
use super::scope::{items as sequence_items, raw_tag, string_value};
use super::value_sets::TEMPLATES;

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
    for row in template.rows {
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
            Concept::Set(ValueSet::Template(..)) => false,
        }
    }
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
            TemplateCondition::RowAbsent { row, .. } => {
                self.matched(template, row).next().is_none()
            }
            TemplateCondition::RowValue {
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
                TemplateCondition::RowAbsent { iff: true, .. }
                    | TemplateCondition::RowValue { iff: true, .. }
            )
        );
        match (row.requirement, met) {
            (RowRequirement::M, _) | (RowRequirement::Mc, Some(true)) => Need::Required,
            (RowRequirement::Mc, Some(false)) if iff => Need::Forbidden,
            _ => Need::Optional,
        }
    }

    /// Departures from the rows' requirements, counts, extensibility and order.
    fn departures(&self, root: &Template) -> Vec<String> {
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
                    departures.push(format!("{} missing", what()));
                }
                Need::Forbidden if count > 0 => {
                    departures.push(format!(
                        "{} present although its condition is not met",
                        what()
                    ));
                }
                _ => {}
            }
            if row.max.is_some_and(|max| count > max) {
                departures.push(format!("{} appears {count} times", what()));
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
                departures.push(format!("Content Item [{}] matches no row", position + 1));
            }
        }
        if root.ordered && !self.assigned.iter().flatten().is_sorted() {
            departures.push("Content Items are not in Template row order".into());
        }
        departures
    }
}

/// Checks a Content Item Sequence against Template `id`. Departures from a
/// Defined Template are violations; from a Baseline Template, advisories, and
/// a Baseline Template makes its rows' Value Sets suggestions too.
pub(super) fn check_items(
    findings: &mut Findings,
    label: &str,
    strength: Strength,
    id: u32,
    objects: &[InMemDicomObject],
) {
    let Some(root) = find(id) else {
        return;
    };
    let matching = Matching::new(root, objects);
    for departure in matching.departures(root) {
        let message = format!(
            "{label}: TID {id} \"{}\" {departure} (PS3.16 {})",
            root.name,
            super::tables::EDITION
        );
        match strength {
            Strength::Defined => findings.value_sets.push(|| message),
            Strength::Baseline => findings.advisories.add(message),
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
            codes::check_items(
                findings,
                &format!("{label}[{}] > Concept Code Sequence", position + 1),
                raw_tag(tags::CONCEPT_CODE_SEQUENCE),
                effective,
                groups,
                sequence_items(item.object, tags::CONCEPT_CODE_SEQUENCE),
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
    }
}
