//! Dimension Index rules beyond the generated presence conditions (PS3.3
//! C.7.6.17.1 and C.7.6.16.2.2): what the pointers may name, their Private
//! Creators, and how each Frame's Dimension Index Values follow the indexed
//! Attribute.

use std::collections::BTreeMap;

use dicom_core::value::Value;
use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;

use super::wsi_image::{sequence, string, tag_values};
use crate::validation::iod::{dicom_edition, EDITION};
use crate::validation::iod::{raw_tag, tag_text};

const CITATION: &str = concat!("PS3.3 ", dicom_edition!(), " C.7.6.17.1");

/// One Item of the Dimension Index Sequence.
struct Dimension {
    pointer: Tag,
    group: Option<Tag>,
}

pub(super) fn validate_dimension_indices(object: &InMemDicomObject) -> Vec<String> {
    let mut violations = Vec::new();
    let shared = sequence(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE).first();
    let per_frame = sequence(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
    let mut dimensions = Vec::new();
    for (index, item) in sequence(object, tags::DIMENSION_INDEX_SEQUENCE)
        .iter()
        .enumerate()
    {
        let label = format!("Dimension Index Sequence[{}]", index + 1);
        let Some(pointer) = tag_values(item, tags::DIMENSION_INDEX_POINTER)
            .first()
            .copied()
        else {
            continue;
        };
        let group = tag_values(item, tags::FUNCTIONAL_GROUP_POINTER)
            .first()
            .copied();
        if pointer == tags::FRAME_CONTENT_SEQUENCE || pointer == tags::DIMENSION_INDEX_VALUES {
            violations.push(format!(
                "{label} > Dimension Index Pointer {} shall not name Frame Content Sequence or Dimension Index Values ({CITATION})",
                tag_text(raw_tag(pointer))
            ));
        }
        let names_group = shared.into_iter().chain(per_frame).any(|item| {
            item.element(pointer)
                .is_ok_and(|element| element.items().is_some())
        });
        if names_group && group.is_some() {
            violations.push(format!(
                "{label} > Functional Group Pointer is present although the Dimension Index Pointer {} names a Functional Group Sequence ({CITATION})",
                tag_text(raw_tag(pointer))
            ));
        }
        // The block a Private Data Element belongs to is reserved where the
        // element is: in the Functional Group Item, or at the top level.
        let holders: Vec<&InMemDicomObject> = match group {
            Some(group) => group_items(shared, per_frame, group),
            None => vec![object],
        };
        private_creator(
            &label,
            pointer,
            tags::DIMENSION_INDEX_PRIVATE_CREATOR,
            item,
            &holders,
            &mut violations,
        );
        if let Some(group) = group {
            let frame_items: Vec<&InMemDicomObject> = shared.into_iter().chain(per_frame).collect();
            private_creator(
                &label,
                group,
                tags::FUNCTIONAL_GROUP_PRIVATE_CREATOR,
                item,
                &frame_items,
                &mut violations,
            );
        }
        dimensions.push(Dimension { pointer, group });
    }
    if dimensions.is_empty() || per_frame.is_empty() {
        // TILED_FULL Frames without Per-Frame Items carry no index Values.
        return violations;
    }
    frame_indices(&dimensions, shared, per_frame, &mut violations);
    violations
}

/// The Items of Functional Group Sequence `group` in the Shared and Per-Frame
/// Items.
fn group_items<'a>(
    shared: Option<&'a InMemDicomObject>,
    per_frame: &'a [InMemDicomObject],
    group: Tag,
) -> Vec<&'a InMemDicomObject> {
    shared
        .into_iter()
        .chain(per_frame)
        .flat_map(|item| sequence(item, group))
        .collect()
}

/// A private pointer's creator Attribute shall hold the Private Creator of the
/// block the pointed element is in.
fn private_creator(
    label: &str,
    pointer: Tag,
    creator_tag: Tag,
    item: &InMemDicomObject,
    holders: &[&InMemDicomObject],
    violations: &mut Vec<String>,
) {
    if pointer.group().is_multiple_of(2) || pointer.element() < 0x1000 {
        return;
    }
    let Some(declared) = string(item, creator_tag) else {
        return; // its presence is a generated 1C condition
    };
    let block = Tag(pointer.group(), pointer.element() >> 8);
    let reserved: Vec<String> = holders
        .iter()
        .filter_map(|holder| string(holder, block))
        .collect();
    if !reserved.is_empty() && !reserved.contains(&declared) {
        violations.push(format!(
            "{label} > {} \"{declared}\" is not the Private Creator \"{}\" that reserves the block of {} ({CITATION})",
            tag_text(raw_tag(creator_tag)),
            reserved[0],
            tag_text(raw_tag(pointer))
        ));
    }
}

/// Dimension Index Values of each Frame: one per Item of the Dimension Index
/// Sequence (C.7.6.16.2.2); Frames with the same index have nominally the same
/// Value of the indexed Attribute, and Frames without it share one index of
/// their own.
fn frame_indices(
    dimensions: &[Dimension],
    shared: Option<&InMemDicomObject>,
    per_frame: &[InMemDicomObject],
    violations: &mut Vec<String>,
) {
    let mut indices: Vec<BTreeMap<u32, Vec<Option<Indexed>>>> =
        vec![BTreeMap::new(); dimensions.len()];
    let mut miscounted = 0;
    for frame in per_frame {
        let Some(values) = sequence(frame, tags::FRAME_CONTENT_SEQUENCE)
            .first()
            .and_then(|content| content.element(tags::DIMENSION_INDEX_VALUES).ok())
            .and_then(|element| element.to_multi_int::<u32>().ok())
        else {
            continue;
        };
        if values.len() != dimensions.len() {
            miscounted += 1;
            continue;
        }
        for ((dimension, index), map) in dimensions.iter().zip(&values).zip(indices.iter_mut()) {
            let indexed = indexed_value(dimension, shared, frame);
            let entry = map.entry(*index).or_default();
            if !entry.contains(&indexed) {
                entry.push(indexed);
            }
        }
    }
    if miscounted > 0 {
        violations.push(format!(
            "{miscounted} Frame(s) have Dimension Index Values whose count is not the {} Items of the Dimension Index Sequence (PS3.3 {EDITION} C.7.6.16.2.2)",
            dimensions.len()
        ));
    }
    for (dimension, map) in dimensions.iter().zip(&indices) {
        let name = tag_text(raw_tag(dimension.pointer));
        if !map.is_empty() && map.values().flatten().all(Option::is_none) {
            violations.push(format!(
                "Dimension Index Pointer {name} names an Attribute no Frame has ({CITATION})"
            ));
            continue;
        }
        if let Some(index) = map
            .iter()
            .find_map(|(index, seen)| (seen.iter().flatten().count() > 1).then_some(index))
        {
            violations.push(format!(
                "Frames with index {index} for {name} have different Values of it; Frames assigned the same index shall have nominally the same Value ({CITATION})"
            ));
        }
        let lacking: Vec<u32> = map
            .iter()
            .filter(|(_, seen)| seen.contains(&None))
            .map(|(index, _)| *index)
            .collect();
        if lacking.len() > 1
            || lacking
                .first()
                .is_some_and(|index| map[index].iter().any(Option::is_some))
        {
            violations.push(format!(
                "Frames without {name} shall share one index of their own, but use {lacking:?} ({CITATION})"
            ));
        }
    }
}

/// The indexed Attribute of one Frame, from its Per-Frame Item or else the
/// Shared Item, compared nominally.
#[derive(Clone, Debug)]
enum Indexed {
    Numbers(Vec<f64>),
    Text(String),
}

impl PartialEq for Indexed {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Numbers(left), Self::Numbers(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(a, b)| (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0))
            }
            (Self::Text(left), Self::Text(right)) => left == right,
            _ => false,
        }
    }
}

fn indexed_value(
    dimension: &Dimension,
    shared: Option<&InMemDicomObject>,
    frame: &InMemDicomObject,
) -> Option<Indexed> {
    let element = [Some(frame), shared]
        .into_iter()
        .flatten()
        .find_map(|item| match dimension.group {
            Some(group) => sequence(item, group)
                .first()
                .and_then(|group_item| group_item.element(dimension.pointer).ok()),
            None => item.element(dimension.pointer).ok(),
        })?;
    indexed(element)
}

fn indexed(element: &InMemElement) -> Option<Indexed> {
    match element.value() {
        Value::Primitive(primitive) if primitive.multiplicity() == 0 => None,
        Value::Primitive(_) => match element.to_multi_float64() {
            Ok(numbers) if element.vr() != dicom_core::VR::CS => Some(Indexed::Numbers(numbers)),
            _ => element
                .to_str()
                .ok()
                .map(|text| Indexed::Text(text.trim().to_string()))
                .filter(|indexed| !matches!(indexed, Indexed::Text(text) if text.is_empty())),
        },
        // A pointer to a Functional Group Sequence indexes all of its content.
        other => Some(Indexed::Text(format!("{other:?}"))),
    }
}
