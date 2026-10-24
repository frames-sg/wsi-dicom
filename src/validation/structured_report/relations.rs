//! The relationships of a Content Tree (PS3.3 C.17.3.2 and A.35.x.3.1): the
//! Value Types the IOD permits, its Relationship Content Constraints table,
//! by-reference relationships and the loops they could make, and the SELECTED
//! FROM Targets an SCOORD or TCOORD Content Item needs (C.18.6, C.18.7).

use dicom_dictionary_std::tags;

use super::tree::Tree;
use super::{Findings, CITATION};
use crate::validation::iod::is_image_storage_sop_class;
use crate::validation::iod::structured_report::DocumentConstraints;
use crate::validation::iod::{items, string_value};

/// The IODs whose relationships may be conveyed by-reference (A.35.3.3.1.2
/// and A.35.13.3.1.2); the others convey them by-value only.
const BY_REFERENCE_IODS: [&str; 2] = ["A.35.3", "A.35.13"];
/// The Relationship Types that "shall not be conveyed by-reference" where
/// by-reference relationships are permitted.
const BY_VALUE_ONLY: [&str; 2] = ["HAS CONCEPT MOD", "CONTAINS"];
/// The Enumerated Values of Value Type and Relationship Type in Tables
/// C.17-5 and C.17-6; the IOD checks report other Values.
const VALUE_TYPES: [&str; 16] = [
    "TEXT",
    "NUM",
    "CODE",
    "DATE",
    "TIME",
    "DATETIME",
    "UIDREF",
    "PNAME",
    "COMPOSITE",
    "IMAGE",
    "WAVEFORM",
    "SCOORD",
    "SCOORD3D",
    "TCOORD",
    "CONTAINER",
    "TABLE",
];
const RELATIONSHIP_TYPES: [&str; 7] = [
    "CONTAINS",
    "HAS PROPERTIES",
    "HAS OBS CONTEXT",
    "HAS ACQ CONTEXT",
    "INFERRED FROM",
    "SELECTED FROM",
    "HAS CONCEPT MOD",
];
const SELECTED_FROM: &str = "SELECTED FROM";

/// Holds the tree to the IOD's Content Constraints.
pub(super) fn check(tree: &Tree<'_>, constraints: &DocumentConstraints, findings: &mut Findings) {
    root(tree, findings);
    let by_reference = BY_REFERENCE_IODS.contains(&constraints.iod);
    for index in 1..tree.nodes.len() {
        let node = &tree.nodes[index];
        if node.is_by_reference() {
            reference(tree, index, constraints, by_reference, findings);
        } else {
            value_type(tree, index, constraints, findings);
        }
        relationship(tree, index, constraints, findings);
    }
    if by_reference {
        loops(tree, constraints.iod, findings);
    }
    check_selected_from(tree, findings);
}

/// C.17.3.1: the root Content Item is of type CONTAINER.
fn root(tree: &Tree<'_>, findings: &mut Findings) {
    match tree.root().value_type() {
        Some("CONTAINER") | None => {}
        Some(other) => findings.push(|| {
            format!(
                "the root Content Item has Value Type (0040,A040) {other}; the root Content Item of the SR Document Content Tree is of type CONTAINER ({CITATION} C.17.3.1)"
            )
        }),
    }
}

/// A.35.x.3.1.1: the Value Types Content Sequence Items may have.
fn value_type(
    tree: &Tree<'_>,
    index: usize,
    constraints: &DocumentConstraints,
    findings: &mut Findings,
) {
    let Some(value_type) = tree.nodes[index].value_type() else {
        return;
    };
    if VALUE_TYPES.contains(&value_type) && !constraints.value_types.contains(&value_type) {
        let at = tree.at(index);
        findings.push(|| {
            format!(
                "{at} has Value Type (0040,A040) {value_type}, which the IOD does not permit in Content Sequence (0040,A730); Enumerated Values are {} ({CITATION} {}.3.1.1)",
                constraints.value_types.join(", "),
                constraints.iod
            )
        });
    }
}

/// A.35.x.3.1.2: whether the IOD admits the relationship from the Source
/// Content Item to the Target (`index`, or the Item it references).
fn relationship(
    tree: &Tree<'_>,
    index: usize,
    constraints: &DocumentConstraints,
    findings: &mut Findings,
) {
    let node = &tree.nodes[index];
    let Some(relationship) = node
        .relationship()
        .filter(|r| RELATIONSHIP_TYPES.contains(r))
    else {
        return;
    };
    let source = node
        .parent
        .and_then(|parent| tree.nodes[parent].value_type());
    let (Some(source), Some(target)) = (source, tree.target_value_type(index)) else {
        return;
    };
    if !VALUE_TYPES.contains(&source) || !VALUE_TYPES.contains(&target) {
        return;
    }
    if !constraints.admits(source, relationship, target) {
        let at = tree.at(index);
        findings.push(|| {
            format!(
                "{at}: a {source} Source Content Item {relationship} a {target} Target Content Item, which Table {} does not permit ({CITATION} {}.3.1.2)",
                constraints.table, constraints.iod
            )
        });
    }
}

/// A by-reference Content Item: forbidden by the by-value IODs; elsewhere
/// not for HAS CONCEPT MOD or CONTAINS, and naming a Content Item of the
/// tree that conveys a Value Type.
fn reference(
    tree: &Tree<'_>,
    index: usize,
    constraints: &DocumentConstraints,
    by_reference: bool,
    findings: &mut Findings,
) {
    let node = &tree.nodes[index];
    let at = tree.at(index);
    let identifier = || {
        node.reference
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join("\\")
    };
    if !by_reference {
        findings.push(|| {
            format!(
                "{at} has Referenced Content Item Identifier (0040,DB73) {}; relationships between Content Items in the content of this IOD shall be conveyed in the by-value mode ({CITATION} {}.3.1.2)",
                identifier(),
                constraints.iod
            )
        });
        return;
    }
    if let Some(relationship) = node.relationship().filter(|r| BY_VALUE_ONLY.contains(r)) {
        findings.push(|| {
            format!(
                "{at} conveys a {relationship} relationship by-reference (Referenced Content Item Identifier (0040,DB73) {}); the HAS CONCEPT MOD and CONTAINS relationships shall not be conveyed by-reference ({CITATION} {}.3.1.2)",
                identifier(),
                constraints.iod
            )
        });
    }
    match tree.target(index) {
        None if tree.unread > 0 => findings.unexamined(format!(
            "the Target of {at}, Referenced Content Item Identifier {}, beyond the Content Items read",
            identifier()
        )),
        None => findings.push(|| {
            format!(
                "{at} has Referenced Content Item Identifier (0040,DB73) {}, which identifies no Content Item of the Content Tree; it is 1 for the root and then the ordinal position of each Content Item along the by-value path ({CITATION} Table C.17-6)",
                identifier()
            )
        }),
        Some(target) if tree.nodes[target].is_by_reference() => findings.push(|| {
            format!(
                "{at} references Content Item {} by-reference, which is itself a by-reference Content Item; the Target Content Item of a relationship conveys a Value Type ({CITATION} C.17.3.2.5)",
                tree.identifier(target)
            )
        }),
        Some(_) => {}
    }
}

/// A.35.3.3.1.2 and A.35.13.3.1.2: "Relationships by-reference to ancestor
/// Content Items are forbidden in this IOD to prevent loops." Each Target
/// that is its Source or an ancestor of it is reported where it is named;
/// then one depth-first walk of the other relationships, by-value and
/// by-reference, finds the loops that chains of by-reference relationships
/// make.
fn loops(tree: &Tree<'_>, iod: &str, findings: &mut Findings) {
    const UNSEEN: u8 = 0;
    const OPEN: u8 = 1;
    const DONE: u8 = 2;
    let mut ancestral = vec![false; tree.nodes.len()];
    for (index, node) in tree.nodes.iter().enumerate() {
        let (Some(source), Some(target)) = (
            node.parent,
            node.reference.as_ref().and_then(|_| tree.target(index)),
        ) else {
            continue;
        };
        if tree.is_ancestor(target, source) {
            ancestral[index] = true;
            let at = tree.at(index);
            findings.push(|| {
                format!(
                    "{at} references Content Item {} by-reference, which is {}; relationships by-reference to ancestor Content Items are forbidden in this IOD to prevent loops ({CITATION} {iod}.3.1.2)",
                    tree.identifier(target),
                    if target == source {
                        "its Source Content Item"
                    } else {
                        "an ancestor of its Source Content Item"
                    }
                )
            });
        }
    }
    let mut state = vec![UNSEEN; tree.nodes.len()];
    // (Content Item, next child to follow)
    let mut stack: Vec<(usize, usize)> = vec![(0, tree.nodes[0].children.start)];
    state[0] = OPEN;
    while let Some((node, next)) = stack.last().copied() {
        if next >= tree.nodes[node].children.end {
            state[node] = DONE;
            stack.pop();
            continue;
        }
        if let Some(top) = stack.last_mut() {
            top.1 += 1;
        }
        let Some(target) = tree.target(next).filter(|_| !ancestral[next]) else {
            continue;
        };
        match state[target] {
            UNSEEN => {
                state[target] = OPEN;
                stack.push((target, tree.nodes[target].children.start));
            }
            OPEN => {
                let at = tree.at(next);
                findings.push(|| {
                    format!(
                        "{at} closes a loop of relationships through by-reference Content Items back to Content Item {}; relationships by-reference to ancestor Content Items are forbidden in this IOD to prevent loops ({CITATION} {iod}.3.1.2)",
                        tree.identifier(target)
                    )
                });
            }
            _ => {}
        }
    }
}

/// C.18.6 and C.18.7: an SCOORD Content Item is the Source of one or more
/// SELECTED FROM relationships with IMAGE Targets, each referencing an
/// image; a TCOORD Content Item of one or more with SCOORD, IMAGE or
/// WAVEFORM Targets, and Referenced Sample Positions only for channels of a
/// single multiplex group (Table C.18.7-1). These bind Content Items in any
/// IOD.
pub(super) fn check_selected_from(tree: &Tree<'_>, findings: &mut Findings) {
    for index in 0..tree.nodes.len() {
        match tree.nodes[index].value_type() {
            Some("SCOORD") if !tree.nodes[index].is_by_reference() => {
                spatial_sources(tree, index, findings);
            }
            Some("TCOORD") if !tree.nodes[index].is_by_reference() => {
                temporal_sources(tree, index, findings);
            }
            _ => {}
        }
    }
}

/// The resolved Targets of the SELECTED FROM relationships of `index`, with
/// the Value Type of each.
fn selected<'t>(tree: &'t Tree<'_>, index: usize) -> impl Iterator<Item = (usize, &'t str)> + 't {
    tree.children(index)
        .filter(|child| tree.nodes[*child].relationship() == Some(SELECTED_FROM))
        .filter_map(|child| {
            let target = tree.target(child)?;
            Some((target, tree.nodes[target].value_type()?))
        })
}

fn spatial_sources(tree: &Tree<'_>, index: usize, findings: &mut Findings) {
    let at = tree.at(index);
    let mut images = 0;
    for (target, value_type) in selected(tree, index) {
        if value_type != "IMAGE" {
            continue;
        }
        images += 1;
        let Some(reference) =
            items(tree.nodes[target].object, tags::REFERENCED_SOP_SEQUENCE).first()
        else {
            continue;
        };
        let Some(class) = string_value(reference, tags::REFERENCED_SOP_CLASS_UID, 1) else {
            continue;
        };
        if is_image_storage_sop_class(&class) {
            continue;
        }
        if class.starts_with("1.2.840.10008.") {
            let image = tree.at(target);
            findings.push(|| {
                format!(
                    "{image}, an IMAGE Target of a SELECTED FROM relationship of the SCOORD {at}, references Referenced SOP Class UID {class}, which is not an Image Storage SOP Class; each IMAGE Target Content Item shall contain a reference to one single-frame or Multi-frame Image ({CITATION} C.18.6)"
                )
            });
        } else {
            findings.unexamined(format!(
                "whether Referenced SOP Class UID {class} of {}, a private SOP Class, is an image (C.18.6)",
                tree.at(target)
            ));
        }
    }
    if images == 0 {
        findings.push(|| {
            format!(
                "{at} is an SCOORD Content Item without a SELECTED FROM relationship to an IMAGE Target Content Item; an SCOORD Content Item shall always be the Source Content Item of one or more SELECTED FROM Relationships with IMAGE Target Content Items ({CITATION} C.18.6)"
            )
        });
    }
}

fn temporal_sources(tree: &Tree<'_>, index: usize, findings: &mut Findings) {
    let at = tree.at(index);
    let targets: Vec<(usize, &str)> = selected(tree, index)
        .filter(|(_, value_type)| {
            matches!(*value_type, "SCOORD" | "SCOORD3D" | "IMAGE" | "WAVEFORM")
        })
        .collect();
    if targets.is_empty() {
        findings.push(|| {
            format!(
                "{at} is a TCOORD Content Item without a SELECTED FROM relationship to an SCOORD, IMAGE or WAVEFORM Target Content Item; a TCOORD Content Item shall be the Source Content Item of one or more SELECTED FROM relationships with one or more SCOORD, IMAGE or WAVEFORM Content Items ({CITATION} C.18.7)"
            )
        });
    }
    if tree.nodes[index]
        .object
        .get(tags::REFERENCED_SAMPLE_POSITIONS)
        .is_none()
    {
        return;
    }
    let waveforms: Vec<usize> = targets
        .iter()
        .filter(|(_, value_type)| *value_type == "WAVEFORM")
        .map(|(target, _)| *target)
        .collect();
    if waveforms.is_empty() {
        findings.push(|| {
            format!(
                "{at} > Referenced Sample Positions (0040,A132) is present, but the TCOORD selects from no WAVEFORM whose Referenced Waveform Channels (0040,A0B0) could refer to a single multiplex group; it shall only be used when they do ({CITATION} Table C.18.7-1)"
            )
        });
        return;
    }
    for waveform in waveforms {
        multiplex_group(tree, index, waveform, findings);
    }
}

/// Table C.18.7-1: Referenced Sample Positions only when the WAVEFORM's
/// Referenced Waveform Channels, (multiplex group, channel) pairs, name one
/// multiplex group (C.18.5.1.1).
fn multiplex_group(tree: &Tree<'_>, source: usize, waveform: usize, findings: &mut Findings) {
    let Some(reference) = items(tree.nodes[waveform].object, tags::REFERENCED_SOP_SEQUENCE).first()
    else {
        return;
    };
    let at = tree.at(source);
    let Some(channels) = reference
        .get(tags::REFERENCED_WAVEFORM_CHANNELS)
        .and_then(|element| element.to_multi_int::<i64>().ok())
    else {
        findings.unexamined(format!(
            "whether the WAVEFORM {} that {at} selects its Referenced Sample Positions from has a single multiplex group: it names no Referenced Waveform Channels, so the reference applies to all of the waveform's channels, which is not in the validated object",
            tree.at(waveform)
        ));
        return;
    };
    let mut groups: Vec<i64> = channels.iter().step_by(2).copied().collect();
    groups.sort_unstable();
    groups.dedup();
    if groups.len() > 1 {
        findings.push(|| {
            format!(
                "{at} > Referenced Sample Positions (0040,A132) is present, but the WAVEFORM {} it selects from has Referenced Waveform Channels (0040,A0B0) in multiplex groups {groups:?}; it shall only be used when they refer to channels within a single multiplex group ({CITATION} Table C.18.7-1)",
                tree.at(waveform)
            )
        });
    }
}
