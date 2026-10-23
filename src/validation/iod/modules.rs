//! Module usage (Table A.32.8-1) and Functional Group usage (Table A.32.8-2).

use std::collections::BTreeSet;

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::conditions::evaluate;
use super::engine::{by_tag, flatten, Definition, Presence, Walk};
use super::model::{AttributeType, FunctionalGroupMacro, Module, Placement, Usage};
use super::report::{condition_suffix, tag_text, Findings};
use super::scope::{
    has, integer_value, is_private, items, raw_tag, tag_of, Frame, Requirement, Scope,
};
use super::tables::{ConditionId, FUNCTIONAL_GROUPS, MODULES};

/// Evaluates the Modules of the IOD and the Attributes of those that apply.
pub(super) fn check_modules(findings: &mut Findings, scope: &Scope<'_>) {
    let mut definitions = Vec::new();
    for module in MODULES {
        if module_applies(findings, scope, module) {
            flatten(module.table.nodes, module.table, &mut definitions);
        }
    }
    Walk { findings }.data_set(scope, "", &by_tag(&definitions), Presence::Evaluate, 0);
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

fn module_applies(findings: &mut Findings, scope: &Scope<'_>, module: &Module) -> bool {
    let present = module_present(scope.root, module, false);
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
            if has_mandatory_rows(module) && !module_present(scope.root, module, true) {
                findings.modules.push(|| {
                    format!(
                        "{label}: required by Table A.32.8-1 but none of its Attributes is present"
                    )
                });
            }
            true
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
            if !present {
                findings.optional_absent.group(label);
            }
            present
        }
    }
}

/// An optional Module is present when an Attribute that only it defines is
/// present (PS3.3 C.1.2.3 lets Modules share Attributes); `any_row` counts
/// every top-level row, as for a required Module.
fn module_present(root: &InMemDicomObject, module: &Module, any_row: bool) -> bool {
    if !any_row && !module.signature.is_empty() {
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

fn has_mandatory_rows(module: &Module) -> bool {
    top_level_rows(module).iter().any(|definition| {
        matches!(
            definition.attribute.kind,
            AttributeType::One | AttributeType::Two
        )
    })
}

/// Evaluates Functional Group usage and the Attributes of each group Item.
pub(super) fn check_functional_groups(findings: &mut Findings, scope: &Scope<'_>) {
    let root = scope.root;
    let shared = items(root, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE).first();
    let per_frame_present = has(root, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
    let per_frame = items(root, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
    if per_frame_present {
        if let Some(frames) = integer_value(root, tags::NUMBER_OF_FRAMES) {
            if i64::try_from(per_frame.len()).ok() != Some(frames) {
                findings.modules.push(|| format!(
                    "Per-Frame Functional Groups Sequence (5200,9230): {} Items for Number of Frames {frames}; the number of Items shall be the same as the number of Frames (PS3.3 C.7.6.16 Table C.7.6.16-1)",
                    per_frame.len()
                ));
            }
        }
    }
    let permitted: BTreeSet<u32> = FUNCTIONAL_GROUPS
        .iter()
        .flat_map(|group| group.sequences.iter().copied())
        .collect();
    let groups = shared
        .into_iter()
        .map(|item| ("Shared Functional Groups Sequence", 1, item));
    let frames = per_frame
        .iter()
        .enumerate()
        .map(|(index, item)| ("Per-Frame Functional Groups Sequence", index + 1, item));
    for (sequence, position, item) in groups.chain(frames) {
        for element in item.iter() {
            let tag = element.header().tag;
            if !is_private(tag) && !permitted.contains(&raw_tag(tag)) {
                findings.modules.push(|| format!(
                    "{sequence}[{position}] > {}: not a Functional Group Macro permitted by Table A.32.8-2",
                    tag_text(raw_tag(tag))
                ));
            }
        }
    }
    for group in FUNCTIONAL_GROUPS {
        check_group_usage(findings, scope, group, shared, per_frame, per_frame_present);
        check_group_items(findings, scope, group, shared, per_frame);
    }
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
        Requirement::Optional if !present => findings.optional_absent.group(label),
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
    shared: Option<&InMemDicomObject>,
    per_frame: &[InMemDicomObject],
) {
    let mut flattened = Vec::new();
    flatten(group.table.nodes, group.table, &mut flattened);
    let definitions = by_tag(&flattened);
    if let Some(item) = shared.filter(|item| contains_group(item, group)) {
        let frame_scope = scope.frame_item(
            item,
            Frame {
                shared: Some(item),
                per_frame: None,
            },
        );
        Walk { findings }.data_set(
            &frame_scope,
            "Shared Functional Groups Sequence[1] > ",
            &definitions,
            Presence::WhenPresent,
            1,
        );
    }
    for (index, item) in per_frame.iter().enumerate() {
        if !contains_group(item, group) {
            continue;
        }
        let frame_scope = scope.frame_item(
            item,
            Frame {
                shared,
                per_frame: Some(item),
            },
        );
        let path = format!("Per-Frame Functional Groups Sequence[{}] > ", index + 1);
        Walk { findings }.data_set(&frame_scope, &path, &definitions, Presence::WhenPresent, 1);
    }
}
