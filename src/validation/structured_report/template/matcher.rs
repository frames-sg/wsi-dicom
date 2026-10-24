//! The walk of a Content Tree against a Template: each Content Sequence's
//! Items matched to the rows nested under their parent's row, INCLUDE rows
//! expanded, the alternatives an Item matches compared, and each level's
//! rows held to the Items they matched.

use dicom_dictionary_std::tags;

use super::conditions::{Context, Need};
use super::{
    code_is, code_text, describe, prefix, template_of, Count, Report,
    KEY_OBJECT_SELECTION_TEMPLATE, MAX_INCLUDE_DEPTH, MAX_WORK, UNSEALED,
};
use crate::validation::disclosure::withholds_item_content;
use crate::validation::iod::structured_report::{
    code_finding, code_finding_in, CodeFinding, Concept, RowRequirement, Strength, Template,
    TemplateRow, ValueSet,
};
use crate::validation::iod::{items, string_value};
use crate::validation::structured_report::tree::Tree;
use crate::validation::structured_report::{Findings, CITATION, PS3_16};

/// The generic Concept Name Table C.17-5 allows for a reference without a
/// Purpose of Reference.
const GENERIC_PURPOSE: (&str, &str) = ("SCT", "260753009");

/// A row of an expanded level, as the Content Items of one Content Sequence
/// match it.
#[derive(Clone, Copy)]
pub(super) struct Slot {
    pub(super) template: &'static Template,
    pub(super) index: usize,
    /// The row's Relationship Type, or the INCLUDE row's for a top row of an
    /// included Template that gives none.
    pub(super) relationship: &'static str,
    /// The innermost INCLUDE that brought the row in, in `Level::includes`.
    pub(super) include: Option<usize>,
    /// Whether an INCLUDE above the row may be invoked more than once.
    pub(super) repeatable: bool,
    /// Whether no Content Item may be added (TID 2010, recursively).
    pub(super) sealed: bool,
    /// Whether a Baseline Template inclusion binds the row.
    pub(super) baseline: bool,
}

impl Slot {
    pub(super) fn row(&self) -> &'static TemplateRow {
        &self.template.rows[self.index]
    }
}

/// An INCLUDE row of a level and the INCLUDE it is nested in.
#[derive(Clone, Copy)]
pub(super) struct Include {
    pub(super) template: &'static Template,
    pub(super) index: usize,
    pub(super) parent: Option<usize>,
}

/// What the INCLUDE rows above an expanded row pass down to it.
#[derive(Clone, Copy)]
struct Inclusion {
    /// The INCLUDE row's Relationship Type, for rows that give none.
    relationship: &'static str,
    include: Option<usize>,
    repeatable: bool,
    sealed: bool,
    baseline: bool,
    depth: usize,
}

/// The rows the Content Items of one Content Sequence match.
#[derive(Default)]
pub(super) struct Level {
    pub(super) slots: Vec<Slot>,
    pub(super) includes: Vec<Include>,
}

/// The rows directly under row `index`; the top rows for `None`.
fn child_rows(template: &'static Template, index: Option<usize>) -> Vec<usize> {
    let Some(index) = index else {
        return (0..template.rows.len())
            .filter(|row| template.rows[*row].level == 0)
            .collect();
    };
    let level = template.rows[index].level;
    template.rows[index + 1..]
        .iter()
        .take_while(|row| row.level > level)
        .enumerate()
        .filter(|(_, row)| row.level == level + 1)
        .map(|(offset, _)| index + 1 + offset)
        .collect()
}

/// Whether Concept Name constrains the row to particular codes.
fn specific(row: &TemplateRow) -> bool {
    !matches!(row.concept, Concept::Any | Concept::Parameter(_))
}

/// One walk of a document's tree against its Templates.
pub(super) struct Matcher<'t, 'a> {
    tree: &'t Tree<'a>,
    /// The Template named in messages: the root of the walk.
    root: Option<&'static Template>,
    /// The Content Items a walk matched to a row.
    pub(super) matched: Vec<bool>,
    work: usize,
    exhausted: bool,
    parameters: bool,
}

/// What a level's matching found of its rows.
pub(super) struct Matching {
    /// The slot each Content Item of the level matched.
    pub(super) assigned: Vec<Option<usize>>,
    /// The Content Items matched to each slot.
    pub(super) counts: Vec<u32>,
}

impl<'t, 'a> Matcher<'t, 'a> {
    pub(super) fn new(tree: &'t Tree<'a>) -> Self {
        Self {
            tree,
            root: None,
            matched: vec![false; tree.nodes.len()],
            work: 0,
            exhausted: false,
            parameters: false,
        }
    }

    pub(super) fn finish(&self, findings: &mut Findings) {
        if self.exhausted {
            findings.unexamined(format!(
                "Template conformance beyond {MAX_WORK} row comparisons"
            ));
        }
        if self.parameters {
            findings.unexamined(
                "the Value Sets INCLUDE rows bind to Template parameters (e.g. $Measurement); rows a parameter constrains admit any Concept Name".into(),
            );
        }
    }

    /// Holds Content Item `node` and its subtree to `template`, which
    /// consists of a single CONTAINER with nested content.
    pub(super) fn root(
        &mut self,
        node: usize,
        template: &'static Template,
        findings: &mut Findings,
    ) {
        let top = child_rows(template, None);
        let [index] = top[..] else {
            findings.unexamined(format!(
                "Template conformance to TID {} \"{}\", which is not a single CONTAINER with nested content",
                template.id, template.name
            ));
            return;
        };
        self.root = Some(template);
        let slot = Slot {
            template,
            index,
            relationship: "",
            include: None,
            repeatable: false,
            sealed: template.id == KEY_OBJECT_SELECTION_TEMPLATE,
            baseline: false,
        };
        self.root_item(node, slot, findings);
        self.matched[node] = true;
        self.children(node, slot, findings);
    }

    /// The Value Type and Concept Name of the root row.
    fn root_item(&mut self, node: usize, slot: Slot, findings: &mut Findings) {
        let row = slot.row();
        let object = self.tree.nodes[node].object;
        let at = self.tree.at(node);
        let template = slot.template;
        if self.tree.nodes[node].value_type() != Some(row.value_type) {
            findings.push(|| {
                format!(
                    "{at}: TID {} \"{}\" Row {} is a {}, not {} ({PS3_16})",
                    template.id,
                    template.name,
                    row.row,
                    row.value_type,
                    describe(object, self.tree.nodes[node].value_type())
                )
            });
            return;
        }
        let concept = items(object, tags::CONCEPT_NAME_CODE_SEQUENCE).first();
        match (row.concept, concept) {
            (Concept::Set(ValueSet::ContextGroups(strength, groups)), Some(concept)) => {
                match code_finding(strength, groups, concept) {
                    Some(CodeFinding::Violation(message)) => findings.push(|| {
                        format!(
                            "{at} > Concept Name Code Sequence (0040,A043): {message}, which TID {} \"{}\" Row {} requires of the Document Title ({PS3_16})",
                            template.id, template.name, row.row
                        )
                    }),
                    Some(CodeFinding::Advisory(message)) => findings.advise(format!(
                        "{at} > Concept Name Code Sequence (0040,A043): {message} (TID {} Row {})",
                        template.id, row.row
                    )),
                    None => {}
                }
            }
            (_, None) => {}
            _ if row.admits(object) => {}
            _ => findings.push(|| {
                format!(
                    "{at}: TID {} \"{}\" Row {} is {} {}, not {} ({PS3_16})",
                    template.id,
                    template.name,
                    row.row,
                    row.value_type,
                    row.concept_text(),
                    describe(object, Some(row.value_type))
                )
            }),
        }
    }

    /// The rows of one Content Sequence: the rows nested under `slot`'s, or
    /// a level's worth of included Template rows.
    fn expand(slot: Slot, report: &mut dyn Report) -> Level {
        let mut level = Level::default();
        let rows = child_rows(slot.template, Some(slot.index));
        let inclusion = Inclusion {
            relationship: "",
            include: None,
            repeatable: false,
            sealed: slot.sealed,
            baseline: slot.baseline,
            depth: 0,
        };
        Self::expand_rows(slot.template, &rows, inclusion, &mut level, report);
        level
    }

    /// Adds `rows` of `template` to `level`, each INCLUDE row as the rows of
    /// the Template it includes.
    fn expand_rows(
        template: &'static Template,
        rows: &[usize],
        inclusion: Inclusion,
        level: &mut Level,
        report: &mut dyn Report,
    ) {
        for index in rows.iter().copied() {
            let row = &template.rows[index];
            let relationship = if row.relationship.is_empty() {
                inclusion.relationship
            } else {
                row.relationship
            };
            let Concept::Set(ValueSet::Template(strength, id)) = row.concept else {
                level.slots.push(Slot {
                    template,
                    index,
                    relationship,
                    include: inclusion.include,
                    repeatable: inclusion.repeatable,
                    sealed: inclusion.sealed,
                    baseline: inclusion.baseline,
                });
                continue;
            };
            let included = template_of(id).filter(|_| inclusion.depth < MAX_INCLUDE_DEPTH);
            let Some(included) = included else {
                report.unexamined(format!(
                    "the rows of TID {id}, which TID {} Row {} includes and the Template tables do not hold",
                    template.id, row.row
                ));
                continue;
            };
            level.includes.push(Include {
                template,
                index,
                parent: inclusion.include,
            });
            let inner = Inclusion {
                relationship,
                include: Some(level.includes.len() - 1),
                repeatable: inclusion.repeatable || row.max != Some(1),
                sealed: inclusion.sealed && !UNSEALED.contains(&id),
                baseline: inclusion.baseline || strength == Strength::Baseline,
                depth: inclusion.depth + 1,
            };
            Self::expand_rows(included, &child_rows(included, None), inner, level, report);
        }
    }

    /// Matches the Content Items of `node`'s Content Sequence to the rows
    /// nested under `slot`'s row, holds the rows to them, and descends.
    fn children(&mut self, node: usize, slot: Slot, report: &mut dyn Report) {
        if self.exhausted {
            return;
        }
        let level = Self::expand(slot, report);
        let children: Vec<usize> = self.tree.children(node).collect();
        if level.slots.is_empty() && level.includes.is_empty() {
            self.unconstrained(&children, slot, report);
            return;
        }
        let matching = self.assign(&children, &level);
        self.hold(node, &children, &level, &matching, slot, report);
        for (child, assigned) in children.iter().zip(&matching.assigned) {
            let Some(found) = assigned else {
                continue;
            };
            let slot = level.slots[*found];
            if report.records() {
                self.matched[*child] = true;
            }
            if matches!(slot.row().concept, Concept::Parameter(_)) {
                self.parameters |= report.records();
            }
            self.value_set(*child, slot, report);
            self.purpose_of_reference(*child, slot, report);
            if !self.tree.nodes[*child].is_by_reference() {
                self.children(*child, slot, report);
            }
        }
    }

    /// Content Items under a row that defines none: additions, unless the
    /// Template may be extended.
    fn unconstrained(&self, children: &[usize], slot: Slot, report: &mut dyn Report) {
        if slot.template.extensible && !slot.sealed {
            return;
        }
        for child in children {
            self.addition(*child, slot.template, report);
        }
    }

    fn addition(&self, child: usize, owner: &'static Template, report: &mut dyn Report) {
        let at = self.tree.at(child);
        let target = self.tree.target(child).unwrap_or(child);
        let object = self.tree.nodes[target].object;
        let value_type = self.tree.nodes[target].value_type();
        let root = self.root;
        report.departure(false, &|| {
            format!(
                "{at}: {}Content Item {} matches no row of TID {} \"{}\", to which no Content Items may be added ({PS3_16})",
                prefix(root, owner),
                describe(object, value_type),
                owner.id,
                owner.name
            )
        });
    }

    /// Assigns each Content Item to a row it matches.
    fn assign(&mut self, children: &[usize], level: &Level) -> Matching {
        let mut assigned = Vec::with_capacity(children.len());
        for child in children.iter().copied() {
            let candidates = self.candidates(child, level);
            let chosen = match candidates[..] {
                [] => None,
                [only] => Some(only),
                _ => Some(self.best(child, &candidates, level)),
            };
            assigned.push(chosen);
        }
        let mut counts = vec![0u32; level.slots.len()];
        for found in assigned.iter().flatten() {
            counts[*found] = counts[*found].saturating_add(1);
        }
        Matching { assigned, counts }
    }

    /// The slots Content Item `child` matches, those with a particular
    /// Concept Name first, and those of the Template it declares if any.
    fn candidates(&mut self, child: usize, level: &Level) -> Vec<usize> {
        let node = &self.tree.nodes[child];
        let Some(target) = self.tree.target(child) else {
            return Vec::new();
        };
        let object = self.tree.nodes[target].object;
        let mut found: Vec<usize> = Vec::new();
        for (index, slot) in level.slots.iter().enumerate() {
            if self.work >= MAX_WORK {
                self.exhausted = true;
                break;
            }
            self.work += 1;
            let (relationship, by_reference) = match slot.relationship.strip_prefix("R-") {
                Some(relationship) => (relationship, true),
                None => (slot.relationship, false),
            };
            let related = slot.relationship.is_empty()
                || (node.relationship() == Some(relationship)
                    && node.is_by_reference() == by_reference);
            if related && slot.row().admits(object) {
                found.push(index);
            }
        }
        if found
            .iter()
            .any(|index| specific(level.slots[*index].row()))
        {
            found.retain(|index| specific(level.slots[*index].row()));
        }
        let declared = items(object, tags::CONTENT_TEMPLATE_SEQUENCE)
            .first()
            .and_then(|item| string_value(item, tags::TEMPLATE_IDENTIFIER, 1))
            .and_then(|identifier| identifier.parse::<u32>().ok());
        if let Some(id) = declared {
            if found
                .iter()
                .any(|index| level.slots[*index].template.id == id)
            {
                found.retain(|index| level.slots[*index].template.id == id);
            }
        }
        found
    }

    /// The candidate whose subtree Content Item `child` departs from least.
    fn best(&mut self, child: usize, candidates: &[usize], level: &Level) -> usize {
        let mut best = (usize::MAX, candidates[0]);
        for candidate in candidates.iter().copied() {
            let mut count = Count::default();
            if !self.tree.nodes[child].is_by_reference() {
                self.children(child, level.slots[candidate], &mut count);
            }
            if count.0 < best.0 {
                best = (count.0, candidate);
            }
            if best.0 == 0 {
                break;
            }
        }
        best.1
    }

    /// Holds the level's rows to the Content Items matched to them.
    fn hold(
        &self,
        parent: usize,
        children: &[usize],
        level: &Level,
        matching: &Matching,
        owner: Slot,
        report: &mut dyn Report,
    ) {
        let context = Context {
            tree: self.tree,
            parent,
            owner,
            level,
            matching,
            children,
        };
        let at = self.tree.at(parent);
        let root = self.root;
        let mut groups: Vec<usize> = Vec::new();
        for (index, slot) in level.slots.iter().enumerate() {
            let row = slot.row();
            let count = matching.counts[index];
            let need = context.gate(slot.include).and(context.need(
                slot.template,
                row,
                &mut groups,
                index,
            ));
            let what = || {
                format!(
                    "{}Row {} {} {}",
                    prefix(root, slot.template),
                    row.row,
                    row.value_type,
                    row.concept_text()
                )
            };
            match need {
                Need::Required if count < row.min.max(1) => {
                    report.departure(slot.baseline, &|| {
                        format!("{at}: {} missing ({PS3_16})", what())
                    });
                }
                Need::Forbidden if count > 0 => report.departure(slot.baseline, &|| {
                    format!(
                        "{at}: {} present although its condition is not met ({PS3_16})",
                        what()
                    )
                }),
                Need::Undetermined
                    if (count == 0 && row.requirement == RowRequirement::Mc)
                        || (count > 0 && row.condition_text.starts_with("IFF")) =>
                {
                    report.undetermined();
                }
                _ => {}
            }
            if !slot.repeatable && row.max.is_some_and(|max| count > max) {
                report.departure(slot.baseline, &|| {
                    format!("{at}: {} appears {count} times ({PS3_16})", what())
                });
            }
        }
        context.groups(&groups, report, root);
        self.additions(children, matching, owner, report);
        self.order(children, matching, level, report);
    }

    /// Content Items that match no row where the owner may not be extended.
    fn additions(
        &self,
        children: &[usize],
        matching: &Matching,
        owner: Slot,
        report: &mut dyn Report,
    ) {
        if owner.template.extensible && !owner.sealed {
            return;
        }
        for (child, assigned) in children.iter().zip(&matching.assigned) {
            if assigned.is_none() {
                self.addition(*child, owner.template, report);
            }
        }
    }

    /// The Content Items of an ordered Template, per inclusion, in row order.
    fn order(
        &self,
        children: &[usize],
        matching: &Matching,
        level: &Level,
        report: &mut dyn Report,
    ) {
        let mut reported: Vec<(usize, Option<usize>)> = Vec::new();
        let mut last: Vec<((usize, Option<usize>), usize)> = Vec::new();
        for (child, assigned) in children.iter().zip(&matching.assigned) {
            let Some(found) = assigned else {
                continue;
            };
            let slot = level.slots[*found];
            if !slot.template.ordered {
                continue;
            }
            let key = (std::ptr::from_ref(slot.template) as usize, slot.include);
            match last.iter_mut().find(|(seen, _)| *seen == key) {
                Some((_, previous)) if *previous > slot.index => {
                    if !reported.contains(&key) {
                        reported.push(key);
                        let at = self.tree.at(*child);
                        let template = slot.template;
                        report.departure(slot.baseline, &|| {
                            format!(
                                "{at}: TID {} \"{}\" Content Items are not in Template row order ({PS3_16})",
                                template.id, template.name
                            )
                        });
                    }
                }
                Some((_, previous)) => *previous = slot.index,
                None => last.push((key, slot.index)),
            }
        }
    }

    /// TID 2010 Rows 8, 9, 10: "Purpose of reference shall not be present."
    /// Table C.17-5 allows a generic Concept Name, such as (260753009, SCT,
    /// "Source"), where a Template does not require the Purpose of
    /// Reference: that generic name is advised, any other a departure.
    fn purpose_of_reference(&self, child: usize, slot: Slot, report: &mut dyn Report) {
        let row = slot.row();
        if slot.template.id != KEY_OBJECT_SELECTION_TEMPLATE || !matches!(row.row, "8" | "9" | "10")
        {
            return;
        }
        let object = self.tree.nodes[child].object;
        let Some(code) = items(object, tags::CONCEPT_NAME_CODE_SEQUENCE).first() else {
            return;
        };
        let at = self.tree.at(child);
        let purpose = code_text(code);
        let message = || {
            format!(
                "{at}: TID 2010 \"Key Object Selection\" Row {} {} has Concept Name Code Sequence (0040,A043) {purpose}; for Rows 8, 9 and 10, purpose of reference shall not be present ({PS3_16})",
                row.row, row.value_type
            )
        };
        if code_is(code, Some(GENERIC_PURPOSE.0), GENERIC_PURPOSE.1) {
            report.advise(format!(
                "{}; but {CITATION} Table C.17-5 allows this generic Concept Name where a Template does not require the Purpose of Reference: the two texts are in tension, so it is advised, not failed",
                message()
            ));
        } else {
            report.departure(false, &message);
        }
    }

    /// The Value Set a CODE row binds to its Concept Code Sequence.
    fn value_set(&self, child: usize, slot: Slot, report: &mut dyn Report) {
        let row = slot.row();
        let (Some(ValueSet::ContextGroups(strength, groups)), "CODE") =
            (row.constraint, row.value_type)
        else {
            return;
        };
        let Some(target) = self.tree.target(child) else {
            return;
        };
        let item = self.tree.nodes[target].object;
        let Some(code) = items(item, tags::CONCEPT_CODE_SEQUENCE).first() else {
            return;
        };
        // The coded Value of a Content Item whose Concept Table E.3.4-1
        // removes, such as Subject Sex, is withheld.
        let withheld = withholds_item_content(item);
        let strength = if slot.baseline {
            Strength::Baseline
        } else {
            strength
        };
        let at = self.tree.at(child);
        let root = self.root;
        match code_finding_in(strength, groups, code, withheld) {
            Some(CodeFinding::Violation(message)) => report.departure(false, &|| {
                format!(
                    "{at} > Concept Code Sequence (0040,A168): {message}, which {}Row {} binds",
                    prefix(root, slot.template),
                    row.row
                )
            }),
            Some(CodeFinding::Advisory(message)) => report.advise(format!(
                "{at} > Concept Code Sequence (0040,A168): {message} ({}Row {})",
                prefix(root, slot.template),
                row.row
            )),
            None => {}
        }
    }
}
