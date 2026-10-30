//! Dimension Index rules beyond the generated presence conditions (PS3.3
//! C.7.6.17.1 and C.7.6.16.2.2): what the pointers may name, their Private
//! Creators, and how each Frame's Dimension Index Values follow the indexed
//! Attribute.

use std::collections::BTreeMap;

use dicom_core::value::Value;
use dicom_core::{Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;

use super::wsi_image::{sequence, string, tag_values};
use crate::validation::disclosure::{shown, withholds};
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
    frame_indices(object, &dimensions, shared, per_frame, &mut violations);
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
            "{label} > {} {} is not the Private Creator {} that reserves the block of {} ({CITATION})",
            tag_text(raw_tag(creator_tag)),
            shown(creator_tag, &declared),
            shown(block, &reserved[0]),
            tag_text(raw_tag(pointer))
        ));
    }
}

/// Dimension Index Values of each Frame: one per Item of the Dimension Index
/// Sequence (C.7.6.16.2.2); Frames with the same index have nominally the same
/// Value of the indexed Attribute, and Frames without it share one index of
/// their own.
fn frame_indices(
    object: &InMemDicomObject,
    dimensions: &[Dimension],
    shared: Option<&InMemDicomObject>,
    per_frame: &[InMemDicomObject],
    violations: &mut Vec<String>,
) {
    let mut indices: Vec<BTreeMap<u32, Seen>> =
        (0..dimensions.len()).map(|_| BTreeMap::new()).collect();
    let mut miscounted = 0;
    let mut zeroed = 0;
    for frame in per_frame {
        let Some(values) = sequence(frame, tags::FRAME_CONTENT_SEQUENCE)
            .first()
            .and_then(|content| content.element(tags::DIMENSION_INDEX_VALUES).ok())
            .and_then(|element| element.to_multi_int::<u32>().ok())
        else {
            continue;
        };
        if values.contains(&0) {
            zeroed += 1;
        }
        if values.len() != dimensions.len() {
            miscounted += 1;
            continue;
        }
        for ((dimension, index), map) in dimensions.iter().zip(&values).zip(indices.iter_mut()) {
            map.entry(*index)
                .or_default()
                .observe(indexed_value(dimension, shared, frame));
        }
    }
    if zeroed > 0 {
        violations.push(format!(
            "{zeroed} Frame(s) have a Dimension Index Value of 0; each index value is an ordinal number starting from 1 ({CITATION})"
        ));
    }
    if miscounted > 0 {
        violations.push(format!(
            "{miscounted} Frame(s) have Dimension Index Values whose count is not the {} Items of the Dimension Index Sequence (PS3.3 {EDITION} C.7.6.16.2.2)",
            dimensions.len()
        ));
    }
    for (dimension, map) in dimensions.iter().zip(&indices) {
        let name = tag_text(raw_tag(dimension.pointer));
        if !map.is_empty() && map.values().all(|seen| seen.first.is_none()) {
            violations.push(format!(
                "Dimension Index Pointer {name} names an Attribute no Frame has ({CITATION})"
            ));
            continue;
        }
        let scale = || physical_scale(dimension.pointer, object, shared, per_frame);
        if let Some(departure) = departure(map, dimension.pointer, scale) {
            violations.push(format!(
                "{departure} for {name}; Frames assigned the same index shall contain nominally the same Value ({CITATION})"
            ));
        }
        let lacking: Vec<u32> = map
            .iter()
            .filter(|(_, seen)| seen.without)
            .map(|(index, _)| *index)
            .collect();
        if lacking.len() > 1
            || lacking
                .first()
                .is_some_and(|index| map[index].first.is_some())
        {
            violations.push(format!(
                "Frames without {name} shall share one index of their own, but use {lacking:?} ({CITATION})"
            ));
        }
    }
}

/// What the Frames of one index hold of the indexed Attribute. Only decimal
/// Values are all kept, since only their comparison needs more than the
/// first Value and the first one unlike it.
#[derive(Default)]
struct Seen {
    first: Option<Indexed>,
    unlike: Option<Indexed>,
    /// Frames holding the Attribute.
    count: usize,
    /// Every decimal Value, in Frame order.
    decimals: Vec<Vec<f64>>,
    /// Whether a Frame of the index lacks the Attribute.
    without: bool,
}

impl Seen {
    fn observe(&mut self, value: Option<Indexed>) {
        let Some(value) = value else {
            self.without = true;
            return;
        };
        self.count += 1;
        if let Indexed::Decimals(numbers) = &value {
            self.decimals.push(numbers.clone());
        }
        match &self.first {
            None => self.first = Some(value),
            Some(first) if self.unlike.is_none() && *first != value => self.unlike = Some(value),
            Some(_) => {}
        }
    }

    fn all_decimal(&self) -> bool {
        self.decimals.len() == self.count
    }
}

/// How the Frames of one index fail to hold nominally the same Value, if
/// they do. Integer Values (US, SS, UL, SL, IS, SV, UV, and Column and Row
/// Position in Total Image Pixel Matrix whatever their VR), text and Sequence
/// content are discrete, so they are compared exactly; decimal Values (DS, FL,
/// FD) are physical coordinates, which `decimal_departure` compares.
/// The Values of `pointer`, the indexed Attribute, are shown as
/// `disclosure` allows.
fn departure(
    map: &BTreeMap<u32, Seen>,
    pointer: Tag,
    scale: impl FnOnce() -> Option<Scale>,
) -> Option<String> {
    if map.values().all(Seen::all_decimal) {
        decimal_departure(map, pointer, scale)
    } else {
        exact_departure(map, pointer)
    }
}

/// The first index whose Frames hold two different Values.
fn exact_departure(map: &BTreeMap<u32, Seen>, pointer: Tag) -> Option<String> {
    map.iter().find_map(|(index, seen)| {
        let (first, unlike) = (seen.first.as_ref()?, seen.unlike.as_ref()?);
        Some(match (first.shown(pointer), unlike.shown(pointer)) {
            (Some(first), Some(unlike)) => {
                format!("Frames with index {index} have different Values, {first} and {unlike},")
            }
            _ => format!("Frames with index {index} have different Values"),
        })
    })
}

/// Decimal Values locate a Frame in physical space, such as the Z, X and Y
/// Offsets in the Slide Coordinate System, and vary within one index: each
/// tile of one focal plane has its own focus. Where indices compete, a Frame
/// departs only when its Value lies nearer another index's typical Value than
/// half the gap between the two typical Values. A lone index has nothing to
/// compete with, so its Values may spread only over the physical scale of the
/// Attribute, where one is known, and are otherwise compared exactly.
fn decimal_departure(
    map: &BTreeMap<u32, Seen>,
    pointer: Tag,
    scale: impl FnOnce() -> Option<Scale>,
) -> Option<String> {
    let points: Vec<(u32, &[Vec<f64>])> = map
        .iter()
        .filter(|(_, seen)| !seen.decimals.is_empty())
        .map(|(index, seen)| (*index, seen.decimals.as_slice()))
        .collect();
    let width = points
        .first()
        .and_then(|(_, values)| values.first())
        .map_or(0, Vec::len);
    let uniform = width > 0
        && points
            .iter()
            .all(|(_, values)| values.iter().all(|value| value.len() == width));
    match points.as_slice() {
        _ if !uniform => exact_departure(map, pointer),
        [(index, values)] => match scale() {
            Some(scale) => spread_beyond(*index, values, pointer, &scale),
            None => exact_departure(map, pointer),
        },
        _ => nearer_another_index(&points, pointer),
    }
}

/// The physical extent the Values of a lone index may spread over.
struct Scale {
    extent: f64,
    what: &'static str,
}

/// The scale of a decimal indexed Attribute, where the image states one:
/// Z Offsets lie within the Imaged Volume Depth, which is in µm as they are
/// (Tables C.8.12.4-1 and C.8.12.6.1-1); X and Y Offsets of one index locate the same
/// pixel row or column, so they lie within half a pixel, of the larger
/// Pixel Spacing, since Image Orientation (Slide) may turn either axis along
/// either spacing.
fn physical_scale(
    pointer: Tag,
    object: &InMemDicomObject,
    shared: Option<&InMemDicomObject>,
    per_frame: &[InMemDicomObject],
) -> Option<Scale> {
    let positive = |value: &f64| value.is_finite() && *value > 0.0;
    match pointer {
        tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM => object
            .element(tags::IMAGED_VOLUME_DEPTH)
            .ok()
            .and_then(|element| element.to_float64().ok())
            .filter(positive)
            .map(|extent| Scale {
                extent,
                what: "the Imaged Volume Depth (0048,0003)",
            }),
        tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM | tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM => {
            shared
                .into_iter()
                .chain(per_frame)
                .filter_map(|item| sequence(item, tags::PIXEL_MEASURES_SEQUENCE).first())
                .filter_map(|measures| measures.element(tags::PIXEL_SPACING).ok())
                .filter_map(|element| element.to_multi_float64().ok())
                .flatten()
                .filter(positive)
                .reduce(f64::max)
                .map(|spacing| Scale {
                    extent: spacing / 2.0,
                    what: "half the Pixel Spacing (0028,0030)",
                })
        }
        _ => None,
    }
}

/// The Values of a lone index that spread, in some component, beyond `scale`.
fn spread_beyond(index: u32, values: &[Vec<f64>], pointer: Tag, scale: &Scale) -> Option<String> {
    let width = values.first().map_or(0, Vec::len);
    (0..width).find_map(|component| {
        let (low, high) = values.iter().map(|value| value[component]).fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(low, high), value| (low.min(value), high.max(value)),
        );
        (high - low > scale.extent).then(|| {
            format!(
                "Frames with index {index} have Values from {} to {}, which spread more than {}, {}, although no other index competes with it,",
                number(pointer, &low.to_string()),
                number(pointer, &high.to_string()),
                scale.what,
                scale.extent
            )
        })
    })
}

/// The first Frame Value nearer another index's typical Value than half the
/// gap between the two typical Values, which are the componentwise medians.
/// Typical Values are searched in order of their first component, so only
/// those within twice a Frame's distance from its own typical Value are
/// compared.
fn nearer_another_index(points: &[(u32, &[Vec<f64>])], pointer: Tag) -> Option<String> {
    let mut typical: Vec<(u32, Vec<f64>)> = points
        .iter()
        .map(|(index, values)| (*index, median(values)))
        .collect();
    typical.sort_by(|left, right| left.1[0].total_cmp(&right.1[0]));
    let own: BTreeMap<u32, &[f64]> = typical
        .iter()
        .map(|(index, center)| (*index, center.as_slice()))
        .collect();
    for (index, values) in points {
        let center = own[index];
        for value in values.iter() {
            let reach = 2.0 * distance(value, center);
            if reach == 0.0 || !reach.is_finite() {
                continue;
            }
            let start = typical.partition_point(|(_, other)| other[0] < center[0] - reach);
            for (other_index, other) in typical[start..]
                .iter()
                .take_while(|(_, other)| other[0] <= center[0] + reach)
            {
                let gap = distance(center, other);
                if other_index != index && distance(value, other) < gap / 2.0 {
                    return Some(format!(
                        "a Frame with index {index} has Value {}, nearer to the typical Value {} of index {other_index} than half the gap from the typical Value {} of index {index}, so the two indices cannot be told apart",
                        number(pointer, &joined(value)),
                        number(pointer, &joined(other)),
                        number(pointer, &joined(center))
                    ));
                }
            }
        }
    }
    None
}

fn median(values: &[Vec<f64>]) -> Vec<f64> {
    let width = values.first().map_or(0, Vec::len);
    (0..width)
        .map(|component| {
            let mut column: Vec<f64> = values.iter().map(|value| value[component]).collect();
            column.sort_by(f64::total_cmp);
            let middle = column.len() / 2;
            if column.len() % 2 == 1 {
                column[middle]
            } else {
                (column[middle - 1] + column[middle]) / 2.0
            }
        })
        .collect()
}

fn distance(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f64>()
        .sqrt()
}

fn joined<T: ToString>(value: &[T]) -> String {
    value
        .iter()
        .map(T::to_string)
        .collect::<Vec<_>>()
        .join("\\")
}

/// A numeric Value of `pointer` as a message shows it: as it is, or
/// withheld (`disclosure`).
fn number(pointer: Tag, value: &str) -> String {
    if withholds(pointer) {
        shown(pointer, value).to_string()
    } else {
        value.to_string()
    }
}

/// The indexed Attribute of one Frame, from its Per-Frame Item or else the
/// Shared Item.
#[derive(Clone, Debug, PartialEq)]
enum Indexed {
    Decimals(Vec<f64>),
    Integers(Vec<i64>),
    Text(String),
    /// The content of a Functional Group Sequence the pointer names.
    Items(String),
}

impl Indexed {
    /// The Value of `pointer` as a message shows it; Sequence content is
    /// not shown.
    fn shown(&self, pointer: Tag) -> Option<String> {
        match self {
            Indexed::Decimals(numbers) => Some(number(pointer, &joined(numbers))),
            Indexed::Integers(numbers) => Some(number(pointer, &joined(numbers))),
            Indexed::Text(text) => Some(shown(pointer, text).to_string()),
            Indexed::Items(_) => None,
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

/// Pixel positions are integers whatever VR a file gives them.
const PIXEL_POSITIONS: [Tag; 2] = [
    tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
    tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
];

fn indexed(element: &InMemElement) -> Option<Indexed> {
    let primitive = match element.value() {
        Value::Primitive(primitive) => primitive,
        // A pointer to a Functional Group Sequence indexes all of its content.
        other => return Some(Indexed::Items(format!("{other:?}"))),
    };
    if primitive.multiplicity() == 0 {
        return None;
    }
    let integer = PIXEL_POSITIONS.contains(&element.header().tag)
        || matches!(
            element.vr(),
            VR::US | VR::SS | VR::UL | VR::SL | VR::IS | VR::SV | VR::UV
        );
    let parsed = if integer {
        element.to_multi_int::<i64>().ok().map(Indexed::Integers)
    } else if matches!(element.vr(), VR::DS | VR::FL | VR::FD) {
        element.to_multi_float64().ok().map(Indexed::Decimals)
    } else {
        None
    };
    parsed.or_else(|| {
        element
            .to_str()
            .ok()
            .map(|text| text.trim_matches([' ', '\0']).to_string())
            .filter(|text| !text.is_empty())
            .map(Indexed::Text)
    })
}
