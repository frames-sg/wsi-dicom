//! Module usage and Functional Group usage of an IOD (its Module and
//! Functional Group Macros tables), and the Modules its IOD Content
//! Constraints forbid or permit only one of.

use std::collections::BTreeSet;

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::conditions::evaluate;
use super::engine::{by_tag, flatten, ByTag, Definition, Presence, Walk};
use super::model::{AttributeType, FunctionalGroupMacro, Iod, Module, Placement, Usage};
use super::report::{condition_suffix, tag_text, Findings};
use super::scope::{has, is_private, items, raw_tag, tag_of, Frame, Requirement, Scope};
use super::tables::ConditionId;
use super::values;

/// The groups of the repeating Overlay Plane Attributes (60xx,eeee), PS3.5 7.6.
const FIRST_OVERLAY_GROUP: u16 = 0x6000;
const LAST_OVERLAY_GROUP: u16 = 0x601E;
/// Tags named in one forbidden Module message.
const MAX_TAGS_SHOWN: usize = 4;

/// Evaluates the Modules of the IOD and the Attributes of those that apply.
pub(super) fn check_modules(findings: &mut Findings, scope: &Scope<'_>, iod: &Iod) {
    let overlays = overlay_groups(scope.root);
    let mut definitions = ByTag::new();
    for module in iod.modules {
        if module_applies(findings, scope, iod, module, &overlays) {
            add_rows(&mut definitions, module, &overlays);
        }
    }
    check_forbidden_modules(findings, scope.root, iod, &overlays);
    check_exclusive_modules(findings, scope.root, iod, &overlays);
    Walk { findings }.data_set(scope, "", &definitions, Presence::Evaluate, 0);
}

/// The Module's top-level rows by tag; an Overlay Plane Module applies once
/// in each overlay group the object encodes.
fn add_rows(definitions: &mut ByTag, module: &Module, overlays: &[u16]) {
    let offsets: Vec<u32> = if repeats(module) {
        overlays
            .iter()
            .map(|group| u32::from(group - FIRST_OVERLAY_GROUP) << 16)
            .collect()
    } else {
        vec![0]
    };
    let rows = top_level_rows(module);
    for offset in offsets {
        for definition in &rows {
            definitions
                .entry(definition.attribute.tag + offset)
                .or_default()
                .push(*definition);
        }
    }
}

/// Whether the Module's Attributes are the repeating 60xx group.
fn repeats(module: &Module) -> bool {
    module
        .signature
        .first()
        .copied()
        .or_else(|| {
            top_level_rows(module)
                .first()
                .map(|definition| definition.attribute.tag)
        })
        .is_some_and(is_overlay_tag)
}

fn is_overlay_tag(tag: u32) -> bool {
    tag >> 16 == u32::from(FIRST_OVERLAY_GROUP)
        && values::entry(tag_of(tag)).is_some_and(|entry| entry.mask != u32::MAX)
}

/// The overlay groups 6000 to 601E (even) holding any Data Element.
fn overlay_groups(root: &InMemDicomObject) -> Vec<u16> {
    (FIRST_OVERLAY_GROUP..=LAST_OVERLAY_GROUP)
        .step_by(2)
        .filter(|group| {
            root.iter()
                .any(|element| element.header().tag.group() == *group)
        })
        .collect()
}

/// The tags among `signature` the object encodes, in every overlay group for
/// an Overlay Plane tag.
fn present_tags(root: &InMemDicomObject, signature: &[u32], overlays: &[u16]) -> Vec<u32> {
    let mut present = Vec::new();
    for tag in signature {
        if is_overlay_tag(*tag) {
            present.extend(
                overlays
                    .iter()
                    .map(|group| tag + (u32::from(group - FIRST_OVERLAY_GROUP) << 16))
                    .filter(|candidate| has(root, tag_of(*candidate))),
            );
        } else if has(root, tag_of(*tag)) {
            present.push(*tag);
        }
    }
    present
}

fn usage_requirement(
    usage: Usage,
    condition: Option<ConditionId>,
    scope: &Scope<'_>,
) -> Requirement {
    match (usage, condition) {
        (Usage::Mandatory, _) => Requirement::Required,
        (_, Some(condition)) => evaluate(condition, scope),
        (_, None) => Requirement::Optional,
    }
}

fn module_applies(
    findings: &mut Findings,
    scope: &Scope<'_>,
    iod: &Iod,
    module: &Module,
    overlays: &[u16],
) -> bool {
    let present = module_present(scope.root, module, overlays);
    let requirement = usage_requirement(module.usage, module.condition, scope);
    let label = format!(
        "{} Module (PS3.3 {}, Usage {})",
        module.name,
        module.section,
        module.usage.label()
    );
    match requirement {
        Requirement::Required => {
            // A required Module whose rows are all Type 3 or conditional may
            // legitimately encode nothing; missing Type 1/2 rows are reported
            // by the attribute presence check.
            let rows = top_level_rows(module);
            let mandatory = rows.iter().any(|definition| {
                matches!(
                    definition.attribute.kind,
                    AttributeType::One | AttributeType::Two
                )
            });
            let encoded = rows
                .iter()
                .any(|definition| has(scope.root, tag_of(definition.attribute.tag)));
            if mandatory && !encoded && !repeats(module) {
                findings.modules.push(|| {
                    format!(
                        "{label}: required by Table {} but none of its Attributes is present",
                        iod.module_table
                    )
                });
            }
            true
        }
        // Other Modules of the IOD define every Attribute of a Module without
        // a signature, so its Attributes do not show that it is present.
        Requirement::Forbidden if present && module.signature.is_empty() && !repeats(module) => {
            if let Some(condition) = module.condition {
                findings.modules.undetermined(condition);
            }
            false
        }
        Requirement::Forbidden => {
            if present {
                findings.modules.push(|| {
                    format!(
                        "{label}: present although its usage condition is not met{}",
                        condition_suffix(module.condition)
                    )
                });
            }
            false
        }
        Requirement::Undeterminable => {
            if let (false, Some(condition)) = (present, module.condition) {
                findings.modules.undetermined(condition);
            }
            present
        }
        Requirement::Optional => {
            findings.optional.group(label, present);
            present
        }
    }
}

/// An optional Module is present when an Attribute that only it defines is
/// present (PS3.3 C.1.2.3 lets Modules share Attributes); one without such an
/// Attribute is present when any of its top-level rows is, and an Overlay
/// Plane Module when any overlay group is.
fn module_present(root: &InMemDicomObject, module: &Module, overlays: &[u16]) -> bool {
    if repeats(module) {
        return !overlays.is_empty();
    }
    if !module.signature.is_empty() {
        return module.signature.iter().any(|tag| has(root, tag_of(*tag)));
    }
    top_level_rows(module)
        .iter()
        .any(|definition| has(root, tag_of(definition.attribute.tag)))
}

fn top_level_rows(module: &Module) -> Vec<Definition> {
    let mut definitions = Vec::new();
    flatten(module.table.nodes, module.table, &mut definitions);
    definitions
}

/// Modules the IOD Content Constraints say shall not be present.
fn check_forbidden_modules(
    findings: &mut Findings,
    root: &InMemDicomObject,
    iod: &Iod,
    overlays: &[u16],
) {
    for module in iod.forbidden_modules {
        let present = present_tags(root, module.signature, overlays);
        if present.is_empty() {
            continue;
        }
        let shown = present
            .iter()
            .take(MAX_TAGS_SHOWN)
            .map(|tag| tag_text(*tag))
            .collect::<Vec<_>>()
            .join(", ");
        let more = present.len().saturating_sub(MAX_TAGS_SHOWN);
        findings.modules.push(|| {
            format!(
                "{} Module (PS3.3 {}): present ({shown}{}) although PS3.3 {} says it shall not be present in the {} IOD",
                module.name,
                module.section,
                if more > 0 { format!(" and {more} more") } else { String::new() },
                module.constraint,
                iod.name
            )
        });
    }
}

/// Modules of which the IOD Content Constraints require exactly one.
fn check_exclusive_modules(
    findings: &mut Findings,
    root: &InMemDicomObject,
    iod: &Iod,
    overlays: &[u16],
) {
    for exclusive in iod.exclusive_modules {
        let present: Vec<&str> = exclusive
            .modules
            .iter()
            .copied()
            .filter(|name| {
                iod.modules
                    .iter()
                    .find(|module| module.name == *name)
                    .is_some_and(|module| module_present(root, module, overlays))
            })
            .collect();
        match present.len() {
            1 => {}
            0 => findings.modules.push(|| {
                format!(
                    "none of the {} Modules is present; PS3.3 {} requires one of them",
                    exclusive.modules.join(", "),
                    exclusive.constraint
                )
            }),
            _ => findings.modules.push(|| {
                format!(
                    "the {} Modules are all present; PS3.3 {} permits only one of them",
                    present.join(", "),
                    exclusive.constraint
                )
            }),
        }
    }
}

/// Evaluates Functional Group usage and the Attributes of each group Item; an
/// IOD without a Functional Group Macros table defines none.
pub(super) fn check_functional_groups(findings: &mut Findings, scope: &Scope<'_>, iod: &Iod) {
    let Some(table) = iod.functional_group_table else {
        return;
    };
    let root = scope.root;
    let shared = items(root, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE).first();
    let per_frame_present = has(root, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
    let per_frame = items(root, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
    let groups = group_items(shared, per_frame);
    let permitted: BTreeSet<u32> = iod
        .functional_groups
        .iter()
        .flat_map(|group| group.sequences.iter().copied())
        .collect();
    for (path, item, _) in &groups {
        for element in item.iter() {
            let tag = element.header().tag;
            if !is_private(tag) && !permitted.contains(&raw_tag(tag)) {
                findings.modules.push(|| {
                    format!(
                        "{path}{}: not a Functional Group Macro permitted by Table {table}",
                        tag_text(raw_tag(tag))
                    )
                });
            }
        }
    }
    for group in iod.functional_groups {
        check_group_usage(findings, scope, group, shared, per_frame, per_frame_present);
        check_group_items(findings, scope, group, &groups);
    }
}

/// The Shared Item, then each Per-Frame Item, with its path and the
/// Functional Group Items that describe its Frame.
fn group_items<'a>(
    shared: Option<&'a InMemDicomObject>,
    per_frame: &'a [InMemDicomObject],
) -> Vec<(String, &'a InMemDicomObject, Frame<'a>)> {
    let shared_frame = Frame {
        shared,
        per_frame: None,
    };
    shared
        .map(|item| {
            (
                "Shared Functional Groups Sequence[1] > ".to_string(),
                item,
                shared_frame,
            )
        })
        .into_iter()
        .chain(per_frame.iter().enumerate().map(|(index, item)| {
            (
                format!("Per-Frame Functional Groups Sequence[{}] > ", index + 1),
                item,
                Frame {
                    shared,
                    per_frame: Some(item),
                },
            )
        }))
        .collect()
}

fn contains_group(item: &InMemDicomObject, group: &FunctionalGroupMacro) -> bool {
    group.sequences.iter().any(|tag| has(item, tag_of(*tag)))
}

fn check_group_usage(
    findings: &mut Findings,
    scope: &Scope<'_>,
    group: &FunctionalGroupMacro,
    shared: Option<&InMemDicomObject>,
    per_frame: &[InMemDicomObject],
    per_frame_present: bool,
) {
    let in_shared = shared.is_some_and(|item| contains_group(item, group));
    let frames_with = per_frame
        .iter()
        .filter(|item| contains_group(item, group))
        .count();
    let label = format!(
        "{} Functional Group (PS3.3 {}, Usage {})",
        group.name,
        group.section,
        group.usage.label()
    );
    if in_shared && frames_with > 0 {
        findings.modules.push(|| format!(
            "{label}: present in both the Shared and the Per-Frame Functional Groups Sequence (PS3.3 C.7.6.16.1.1)"
        ));
    }
    match group.placement {
        Placement::SharedOnly if frames_with > 0 => findings.modules.push(|| format!(
            "{label}: shall be used as a Shared Functional Group but is in {frames_with} Per-Frame Item(s)"
        )),
        Placement::NotShared if in_shared => findings.modules.push(|| format!(
            "{label}: shall not be used as a Shared Functional Group"
        )),
        _ => {}
    }
    let requirement = usage_requirement(group.usage, group.condition, scope);
    let present = in_shared || frames_with > 0;
    match requirement {
        Requirement::Required if !in_shared => {
            if !per_frame_present {
                findings.modules.push(|| format!(
                    "{label}: required for every Frame but absent from the Shared Functional Groups Sequence, and no Per-Frame Functional Groups Sequence is present"
                ));
            } else if frames_with < per_frame.len() {
                findings.modules.push(|| format!(
                    "{label}: required for every Frame but absent from the Shared Functional Groups Sequence and from {} of {} Per-Frame Items",
                    per_frame.len() - frames_with,
                    per_frame.len()
                ));
            }
        }
        Requirement::Forbidden if present => {
            findings.modules.push(|| {
                format!(
                    "{label}: present although its usage condition is not met{}",
                    condition_suffix(group.condition)
                )
            });
        }
        Requirement::Optional => findings.optional.group(label, present),
        Requirement::Undeterminable if !present => {
            if let Some(condition) = group.condition {
                findings.modules.undetermined(condition);
            }
        }
        _ => {}
    }
}

fn check_group_items(
    findings: &mut Findings,
    scope: &Scope<'_>,
    group: &FunctionalGroupMacro,
    groups: &[(String, &InMemDicomObject, Frame<'_>)],
) {
    let mut flattened = Vec::new();
    flatten(group.table.nodes, group.table, &mut flattened);
    let definitions = by_tag(&flattened);
    for (path, item, frame) in groups
        .iter()
        .filter(|(_, item, _)| contains_group(item, group))
    {
        let frame_scope = scope.frame_item(item, *frame);
        Walk { findings }.data_set(&frame_scope, path, &definitions, Presence::WhenPresent, 1);
    }
}
