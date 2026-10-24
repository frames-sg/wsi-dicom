//! Attribute presence, Enumerated Values and Sequence Item checks for one Data Set.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use dicom_core::VR;
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;

use super::conditions::evaluate;
use super::model::{Attribute, AttributeType, ItemCount, Node, Selection, Table, ValueSet};
use super::report::{condition_suffix, tag_text, Findings};
use super::scope::{
    element_has_value, integer_value, raw_tag, string_value, tag_of, Requirement, Scope,
};
use super::tables::ConditionId;
use super::MAX_NESTING;
use super::{codes, templates};

/// Bound on "Include Table" indirection at one nesting level.
const MAX_INCLUDE_DEPTH: usize = 16;
/// Bound on the inclusions resolved for one Data Set.
const MAX_DYNAMIC_INCLUSIONS: usize = 64;

/// One table row that applies to a Data Set, with the table that defines it and
/// its Value Set.
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

/// A Macro inclusion that depends on the Data Set it applies to.
#[derive(Clone, Copy, Debug)]
pub(super) enum Dynamic {
    /// Included when the condition is Required for the Data Set.
    Conditional(&'static Table, ConditionId),
    /// The table the Data Set's Value of the selection key names.
    Select(&'static Selection),
}

/// Expands Macro inclusions into the rows that apply at this nesting level,
/// leaving out the inclusions that depend on the Data Set.
pub(super) fn flatten(nodes: &'static [Node], table: &'static Table, out: &mut Vec<Definition>) {
    flatten_level(nodes, table, out, &mut Vec::new());
}

/// `flatten`, collecting the inclusions that depend on the Data Set.
pub(super) fn flatten_level(
    nodes: &'static [Node],
    table: &'static Table,
    out: &mut Vec<Definition>,
    dynamic: &mut Vec<Dynamic>,
) {
    flatten_bounded(nodes, table, out, dynamic, 0);
}

fn flatten_bounded(
    nodes: &'static [Node],
    table: &'static Table,
    out: &mut Vec<Definition>,
    dynamic: &mut Vec<Dynamic>,
    depth: usize,
) {
    for node in nodes {
        match node {
            Node::Attribute(attribute) => out.push(Definition {
                attribute,
                table,
                value_set: attribute.value_set,
            }),
            Node::Include(included) if depth < MAX_INCLUDE_DEPTH => {
                flatten_bounded(included.nodes, included, out, dynamic, depth + 1);
            }
            Node::Include(_) => {}
            Node::IncludeIf(included, condition) => {
                dynamic.push(Dynamic::Conditional(included, *condition));
            }
            Node::Select(selection) => dynamic.push(Dynamic::Select(selection)),
        }
    }
}

/// The tables an inclusion that depends on the Data Set may bring in, for
/// analyses over every Data Set the tables admit.
pub(super) fn dynamic_tables(dynamic: Dynamic) -> Vec<&'static Table> {
    match dynamic {
        Dynamic::Conditional(table, _) => vec![table],
        Dynamic::Select(selection) => selection.tables.iter().map(|(_, table)| *table).collect(),
    }
}

/// The rows that apply to a Data Set, grouped by tag.
pub(super) type ByTag = BTreeMap<u32, Vec<Definition>>;

/// The rows of one nesting level, and the inclusions each Data Set resolves.
#[derive(Default)]
pub(super) struct Level {
    pub(super) rows: ByTag,
    pub(super) dynamic: Vec<Dynamic>,
}

/// The (attribute, table) addresses of the rows that define a Sequence.
type SequenceRows = Vec<(usize, usize)>;

thread_local! {
    /// Item rows by the Sequence rows that hold them. Rows are static, and a
    /// Per-Frame Functional Groups Sequence repeats the same Sequences in every
    /// Item, so the grouping is built once per thread.
    static CHILDREN: RefCell<HashMap<SequenceRows, Rc<Level>>> = RefCell::default();
}

/// The rows of the Items of the Sequences that `group` defines.
fn child_definitions(group: &[Definition]) -> Rc<Level> {
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
            let mut dynamic = Vec::new();
            for definition in group {
                flatten_level(
                    definition.attribute.children,
                    definition.table,
                    &mut children,
                    &mut dynamic,
                );
            }
            Rc::new(Level {
                rows: by_tag(&children),
                dynamic,
            })
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
    /// A key differs from its rows' tag in a repeating group other than the
    /// first (60xx Overlay Planes).
    pub(super) fn data_set(
        &mut self,
        scope: &Scope<'_>,
        path: &str,
        level: &Level,
        presence: Presence,
        depth: usize,
    ) {
        let resolved;
        let definitions = if level.dynamic.is_empty() {
            &level.rows
        } else {
            resolved = self.resolve(scope, level);
            &resolved
        };
        for (tag, group) in definitions {
            let element = scope.item.get(tag_of(*tag));
            if presence == Presence::Evaluate || element.is_some() {
                self.presence(scope, path, *tag, group, element, presence);
            }
            let Some(element) = element else {
                continue;
            };
            self.enumerations(scope, path, *tag, group, element);
            if let Some(items) = element.items() {
                self.value_sets(scope, path, group, items);
                self.sequence(scope, path, group, items, depth);
            }
        }
    }

    /// The level's rows with those of the inclusions that apply to the Data
    /// Set in `scope`: Macros whose condition is Required, and the table the
    /// selection key's Value names. An undeterminable condition includes
    /// nothing and is reported.
    fn resolve(&mut self, scope: &Scope<'_>, level: &Level) -> ByTag {
        let mut rows = Vec::new();
        let mut pending = level.dynamic.clone();
        let mut resolved = 0;
        while let Some(next) = pending.pop() {
            resolved += 1;
            if resolved > MAX_DYNAMIC_INCLUSIONS {
                break;
            }
            let table = match next {
                Dynamic::Conditional(table, condition) => match evaluate(condition, scope) {
                    Requirement::Required => table,
                    Requirement::Undeterminable => {
                        self.findings.presence.undetermined(condition);
                        continue;
                    }
                    Requirement::Optional | Requirement::Forbidden => continue,
                },
                Dynamic::Select(selection) => {
                    let value = string_value(scope.item, tag_of(selection.key), 1);
                    match selection
                        .tables
                        .iter()
                        .find(|(selector, _)| value.as_deref() == Some(*selector))
                    {
                        Some((_, table)) => table,
                        None => continue,
                    }
                }
            };
            flatten_level(table.nodes, table, &mut rows, &mut pending);
        }
        let mut definitions = level.rows.clone();
        for definition in rows {
            definitions
                .entry(definition.attribute.tag)
                .or_default()
                .push(definition);
        }
        definitions
    }

    fn presence(
        &mut self,
        scope: &Scope<'_>,
        path: &str,
        tag: u32,
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
        let label = || format!("{path}{} {}", attribute.name, tag_text(tag));
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
        tag: u32,
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
                            tag_text(tag),
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
        if children.rows.is_empty() && children.dynamic.is_empty() {
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
    fn value_sets(
        &mut self,
        scope: &Scope<'_>,
        path: &str,
        group: &[Definition],
        items: &[InMemDicomObject],
    ) {
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
                    templates::check_items(
                        self.findings,
                        &label,
                        strength,
                        template,
                        items,
                        scope.root,
                    );
                }
                Some(ValueSet::Code(code, section)) => {
                    codes::check_code(self.findings, &label, attribute.tag, code, section, items);
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
