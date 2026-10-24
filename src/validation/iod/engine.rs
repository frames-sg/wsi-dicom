//! Attribute presence, Enumerated Values and Sequence Item checks for one Data Set.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use dicom_core::VR;
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;

use super::conditions::evaluate;
use super::model::{Attribute, AttributeType, ItemCount, Node, Table, ValueSet};
use super::report::{condition_suffix, tag_text, Findings};
use super::scope::{element_has_value, integer_value, raw_tag, tag_of, Requirement, Scope};
use super::MAX_NESTING;
use super::{codes, templates};

/// Bound on "Include Table" indirection at one nesting level.
const MAX_INCLUDE_DEPTH: usize = 16;

/// One table row that applies to a Data Set, with the table that defines it and
/// the Value Set that applies where the table is included.
#[derive(Clone, Copy, Debug)]
pub(super) struct Definition {
    pub(super) attribute: &'static Attribute,
    pub(super) table: &'static Table,
    pub(super) value_set: Option<ValueSet>,
}

impl Definition {
    pub(super) fn citation(&self) -> String {
        format!(
            "{} {} Table {}",
            self.table.part, self.table.section, self.table.label
        )
    }
}

/// Expands Macro inclusions into the rows that apply at this nesting level.
pub(super) fn flatten(nodes: &'static [Node], table: &'static Table, out: &mut Vec<Definition>) {
    flatten_bounded(nodes, table, &[], out, 0);
}

fn flatten_bounded(
    nodes: &'static [Node],
    table: &'static Table,
    bound: &'static [(u32, ValueSet)],
    out: &mut Vec<Definition>,
    depth: usize,
) {
    for node in nodes {
        match node {
            Node::Attribute(attribute) => out.push(Definition {
                attribute,
                table,
                value_set: bound
                    .iter()
                    .find(|(tag, _)| *tag == attribute.tag)
                    .map(|(_, value_set)| *value_set)
                    .or(attribute.value_set),
            }),
            Node::Include(included, bound) if depth < MAX_INCLUDE_DEPTH => {
                flatten_bounded(included.nodes, included, bound, out, depth + 1);
            }
            Node::Include(..) => {}
        }
    }
}

/// The rows that apply to a Data Set, grouped by tag.
pub(super) type ByTag = BTreeMap<u32, Vec<Definition>>;

/// The (attribute, table) addresses of the rows that define a Sequence.
type SequenceRows = Vec<(usize, usize)>;

thread_local! {
    /// Item rows by the Sequence rows that hold them. Rows are static, and a
    /// Per-Frame Functional Groups Sequence repeats the same Sequences in every
    /// Item, so the grouping is built once per thread.
    static CHILDREN: RefCell<HashMap<SequenceRows, Rc<ByTag>>> = RefCell::default();
}

/// The rows of the Items of the Sequences that `group` defines.
fn child_definitions(group: &[Definition]) -> Rc<ByTag> {
    let key: SequenceRows = group
        .iter()
        .map(|definition| {
            (
                std::ptr::from_ref(definition.attribute) as usize,
                std::ptr::from_ref(definition.table) as usize,
            )
        })
        .collect();
    CHILDREN.with(|cache| {
        Rc::clone(cache.borrow_mut().entry(key).or_insert_with(|| {
            let mut children = Vec::new();
            for definition in group {
                flatten(
                    definition.attribute.children,
                    definition.table,
                    &mut children,
                );
            }
            Rc::new(by_tag(&children))
        }))
    })
}

pub(super) fn by_tag(definitions: &[Definition]) -> ByTag {
    let mut grouped = ByTag::new();
    for definition in definitions {
        grouped
            .entry(definition.attribute.tag)
            .or_default()
            .push(*definition);
    }
    grouped
}

/// Whether absence of the top-level rows is evaluated, or only the Value of
/// those present (Functional Group Sequences, whose presence the Macro usage
/// governs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Presence {
    Evaluate,
    WhenPresent,
}

pub(super) struct Walk<'f> {
    pub(super) findings: &'f mut Findings,
}

impl Walk<'_> {
    /// Checks one Data Set against `definitions`, recursing into Sequence Items.
    pub(super) fn data_set(
        &mut self,
        scope: &Scope<'_>,
        path: &str,
        definitions: &ByTag,
        presence: Presence,
        depth: usize,
    ) {
        for (tag, group) in definitions {
            let element = scope.item.get(tag_of(*tag));
            if presence == Presence::Evaluate || element.is_some() {
                self.presence(scope, path, group, element, presence);
            }
            let Some(element) = element else {
                continue;
            };
            self.enumerations(scope, path, group, element);
            if let Some(items) = element.items() {
                self.value_sets(path, group, items);
                self.sequence(scope, path, group, items, depth);
            }
        }
    }

    fn presence(
        &mut self,
        scope: &Scope<'_>,
        path: &str,
        group: &[Definition],
        element: Option<&InMemElement>,
        presence: Presence,
    ) {
        // PS3.3 C.1.2.3: when Modules overlap, the lowest Type applies.
        let Some(lowest) = group.iter().map(|d| d.attribute.kind).min() else {
            return;
        };
        let governing = || group.iter().filter(move |d| d.attribute.kind == lowest);
        let Some(first) = governing().next() else {
            return;
        };
        let (mut required, mut optional, mut forbidden) = (None, false, true);
        for definition in governing() {
            let requirement = requirement(definition, scope);
            match requirement {
                Requirement::Required => required = required.or(Some(definition)),
                Requirement::Optional => optional = true,
                Requirement::Undeterminable if presence == Presence::Evaluate => {
                    if let Some(condition) = definition.attribute.condition {
                        self.findings.presence.undetermined(condition);
                    }
                }
                _ => {}
            }
            forbidden &= requirement == Requirement::Forbidden;
        }
        let attribute = first.attribute;
        let label = || format!("{path}{} {}", attribute.name, tag_text(attribute.tag));
        match (element, required) {
            (None, Some(definition)) => self.findings.presence.push(|| {
                format!(
                    "{}: Type {} missing ({})",
                    label(),
                    definition.attribute.kind.label(),
                    definition.citation()
                )
            }),
            (Some(element), Some(definition))
                if definition.attribute.kind.requires_value()
                    && !element_has_value(element.value()) =>
            {
                self.findings.presence.push(|| {
                    format!(
                        "{}: Type {} present without a Value ({})",
                        label(),
                        definition.attribute.kind.label(),
                        definition.citation()
                    )
                });
            }
            (None, None) if optional && presence == Presence::Evaluate => {
                self.findings.optional.row(*first, false);
            }
            (Some(_), None) if optional => self.findings.optional.row(*first, true),
            (Some(_), None) if forbidden && presence == Presence::Evaluate => {
                self.findings.presence.push(|| {
                    format!(
                        "{}: Type {} present although its condition is not met ({}{})",
                        label(),
                        attribute.kind.label(),
                        first.citation(),
                        condition_suffix(attribute.condition)
                    )
                });
            }
            _ => {}
        }
    }

    fn enumerations(
        &mut self,
        scope: &Scope<'_>,
        path: &str,
        group: &[Definition],
        element: &InMemElement,
    ) {
        // Most rows have none; skipping them also keeps binary Values such as
        // native Pixel Data from being converted to one string per sample.
        if group
            .iter()
            .all(|definition| definition.attribute.enumerations.is_empty())
        {
            return;
        }
        let Ok(values) = element.to_multi_str() else {
            return;
        };
        let numeric = is_numeric(element.vr());
        for definition in group {
            for enumeration in definition.attribute.enumerations {
                if let Some(condition) = enumeration.condition {
                    match evaluate(condition, scope) {
                        Requirement::Required => {}
                        Requirement::Undeterminable => {
                            self.findings.enumerated.undetermined(condition);
                            continue;
                        }
                        Requirement::Optional | Requirement::Forbidden => continue,
                    }
                }
                let selected: Vec<(usize, &String)> = match enumeration.value {
                    Some(index) => values
                        .get(usize::from(index).saturating_sub(1))
                        .map(|value| (usize::from(index), value))
                        .into_iter()
                        .collect(),
                    None => values.iter().enumerate().map(|(i, v)| (i + 1, v)).collect(),
                };
                for (index, raw) in selected {
                    let value = raw.trim_matches(|c| c == ' ' || c == '\0');
                    // A wholly empty element is the presence check's to judge; an
                    // empty Value among others is not a valid Value (PS3.5 6.3).
                    if (value.is_empty() && values.len() == 1)
                        || enumerated(value, enumeration.values, numeric)
                    {
                        continue;
                    }
                    let which = if values.len() > 1 || enumeration.value.is_some() {
                        format!(" Value {index}")
                    } else {
                        String::new()
                    };
                    self.findings.enumerated.push(|| {
                        format!(
                            "{path}{} {}{which} \"{value}\" is not an Enumerated Value [{}] ({})",
                            definition.attribute.name,
                            tag_text(definition.attribute.tag),
                            enumeration.values.join(", "),
                            definition.citation()
                        )
                    });
                }
            }
        }
    }

    fn sequence(
        &mut self,
        scope: &Scope<'_>,
        path: &str,
        group: &[Definition],
        items: &[InMemDicomObject],
        depth: usize,
    ) {
        let name = group[0].attribute.name;
        for definition in group {
            self.item_count(scope, path, definition, items.len());
        }
        if depth >= MAX_NESTING {
            return; // the value-format check reports the nesting bound
        }
        let children = child_definitions(group);
        if children.is_empty() {
            return;
        }
        for (index, item) in items.iter().enumerate() {
            let child = scope.child(item);
            let item_path = format!("{path}{name}[{}] > ", index + 1);
            self.data_set(&child, &item_path, &children, Presence::Evaluate, depth + 1);
        }
    }

    /// Context Group and Template constraints on the Items of a Code or Content
    /// Item Sequence.
    fn value_sets(&mut self, path: &str, group: &[Definition], items: &[InMemDicomObject]) {
        for definition in group {
            let attribute = definition.attribute;
            let label = format!("{path}{}", attribute.name);
            match definition.value_set {
                Some(ValueSet::ContextGroups(strength, groups)) => {
                    codes::check_items(
                        self.findings,
                        &label,
                        attribute.tag,
                        strength,
                        groups,
                        items,
                    );
                }
                Some(ValueSet::Template(strength, template)) => {
                    templates::check_items(self.findings, &label, strength, template, items);
                }
                None => {}
            }
        }
    }

    fn item_count(&mut self, scope: &Scope<'_>, path: &str, definition: &Definition, count: usize) {
        let attribute = definition.attribute;
        // An empty Type 1 Sequence is a presence violation, an empty Type 2
        // Sequence is permitted, and a Type 3 Sequence shall not be present
        // with zero Items (PS3.5 7.4.3, 7.4.5).
        if count == 0 {
            if attribute.kind == AttributeType::Three {
                self.findings.sequence_items.push(|| {
                    format!(
                        "{path}{} {}: Type 3 Sequence present with zero Items (PS3.5 7.4.5; {})",
                        attribute.name,
                        tag_text(attribute.tag),
                        definition.citation()
                    )
                });
            }
            return;
        }
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        let of_value = |tag: u32, n: i64| {
            (i64::from(count) != n)
                .then(|| (format!("{n} (the Value of {})", tag_text(tag)), false))
        };
        let expected = match attribute.items {
            ItemCount::Unconstrained => None,
            ItemCount::Exactly(n) => (count != n).then(|| (format!("exactly {n}"), n == 1)),
            ItemCount::AtLeast(n) => (count < n).then(|| (format!("at least {n}"), false)),
            ItemCount::AtMost(n) => (count > n).then(|| (format!("at most {n}"), n == 1)),
            ItemCount::NumberOfFrames => {
                let tag = raw_tag(tags::NUMBER_OF_FRAMES);
                integer_value(scope.root, tags::NUMBER_OF_FRAMES).and_then(|n| of_value(tag, n))
            }
            ItemCount::ValueOf(primary, fallback) => [primary, fallback]
                .into_iter()
                .find_map(|tag| integer_value(scope.root, tag_of(tag)).map(|n| (tag, n)))
                .and_then(|(tag, n)| of_value(tag, n)),
        };
        if let Some((expected, singular)) = expected {
            self.findings.sequence_items.push(|| {
                format!(
                    "{path}{} {}: {count} Item{} where {expected} {} required ({})",
                    attribute.name,
                    tag_text(attribute.tag),
                    if count == 1 { "" } else { "s" },
                    if singular { "is" } else { "are" },
                    definition.citation()
                )
            });
        }
    }
}

/// Requirement of one row in `scope`.
pub(super) fn requirement(definition: &Definition, scope: &Scope<'_>) -> Requirement {
    let attribute = definition.attribute;
    match (attribute.kind, attribute.condition) {
        (AttributeType::One | AttributeType::Two, _) => Requirement::Required,
        (AttributeType::Three, None) => Requirement::Optional,
        (_, Some(condition)) => evaluate(condition, scope),
        // The generator rejects 1C/2C rows without a condition.
        (AttributeType::OneC | AttributeType::TwoC, None) => Requirement::Undeterminable,
    }
}

fn is_numeric(vr: VR) -> bool {
    matches!(
        vr,
        VR::IS | VR::DS | VR::US | VR::SS | VR::UL | VR::SL | VR::UV | VR::SV | VR::FL | VR::FD
    )
}

fn enumerated(value: &str, allowed: &[&str], numeric: bool) -> bool {
    if allowed.contains(&value) {
        return true;
    }
    numeric
        && value.parse::<f64>().is_ok_and(|number| {
            allowed
                .iter()
                .filter_map(|candidate| candidate.parse::<f64>().ok())
                .any(|candidate| candidate == number)
        })
}
