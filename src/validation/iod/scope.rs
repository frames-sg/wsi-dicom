//! Evaluation context and three-valued logic for PS3.3 conditions.

use dicom_core::header::HasLength;
use dicom_core::value::Value;
use dicom_core::{PrimitiveValue, Tag};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use crate::validation::disclosure::Nesting;

/// Kleene logic: conditions that depend on facts absent from the object are `Unknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tri {
    True,
    False,
    Unknown,
}

impl Tri {
    pub(super) fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    pub(super) fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }

    pub(super) fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }
}

impl From<bool> for Tri {
    fn from(value: bool) -> Self {
        if value {
            Self::True
        } else {
            Self::False
        }
    }
}

/// Outcome of a condition for the Data Element, Module or Macro it governs.
///
/// For a conditional Enumerated Values list, `Required` means the list applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Requirement {
    Required,
    /// May be present or absent.
    Optional,
    /// Shall not be present.
    Forbidden,
    /// The object does not carry the facts the condition depends on.
    Undeterminable,
}

/// What the condition text allows when its condition is not met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Otherwise {
    /// "May be present otherwise."
    MayBePresent,
    /// No such clause: PS3.5 7.4.2/7.4.4 and PS3.3 A.1.3.2 forbid presence.
    NotPresent,
}

pub(super) fn required_if(condition: Tri, otherwise: Otherwise) -> Requirement {
    match (condition, otherwise) {
        (Tri::True, _) => Requirement::Required,
        (Tri::False, Otherwise::MayBePresent) => Requirement::Optional,
        (Tri::False, Otherwise::NotPresent) => Requirement::Forbidden,
        (Tri::Unknown, _) => Requirement::Undeterminable,
    }
}

/// "Shall not be present if/when ..." on a Type 3 Attribute.
pub(super) fn forbidden_if(condition: Tri) -> Requirement {
    match condition {
        Tri::True => Requirement::Forbidden,
        Tri::False => Requirement::Optional,
        Tri::Unknown => Requirement::Undeterminable,
    }
}

/// "Shall be present only if ..." / "May be used only if ...".
pub(super) fn permitted_only_if(condition: Tri) -> Requirement {
    match condition {
        Tri::True => Requirement::Optional,
        Tri::False => Requirement::Forbidden,
        Tri::Unknown => Requirement::Undeterminable,
    }
}

/// The Functional Group Items that describe one Frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame<'a> {
    pub(super) shared: Option<&'a InMemDicomObject>,
    pub(super) per_frame: Option<&'a InMemDicomObject>,
}

/// The Data Set in which a condition is evaluated, with its enclosing Data Sets.
#[derive(Clone, Copy, Debug)]
pub(super) struct Scope<'a> {
    pub(super) root: &'a InMemDicomObject,
    pub(super) item: &'a InMemDicomObject,
    pub(super) parent: Option<&'a Scope<'a>>,
    pub(super) frame: Option<Frame<'a>>,
    pub(super) transfer_syntax: &'a str,
    /// What messages withhold of the Values of `item`.
    pub(super) nesting: Nesting,
}

impl<'a> Scope<'a> {
    pub(super) fn top(root: &'a InMemDicomObject, transfer_syntax: &'a str) -> Self {
        Self {
            root,
            item: root,
            parent: None,
            frame: None,
            transfer_syntax,
            nesting: Nesting::default(),
        }
    }

    pub(super) fn child(&'a self, item: &'a InMemDicomObject) -> Self {
        Self {
            root: self.root,
            item,
            parent: Some(self),
            frame: self.frame,
            transfer_syntax: self.transfer_syntax,
            nesting: self.nesting,
        }
    }

    /// A Functional Group Item of one Frame.
    pub(super) fn frame_item(&'a self, item: &'a InMemDicomObject, frame: Frame<'a>) -> Self {
        Self {
            item,
            parent: Some(self),
            frame: Some(frame),
            ..*self
        }
    }

    pub(super) fn is_top_level(&self) -> bool {
        std::ptr::eq(self.item, self.root)
    }

    pub(super) fn parent_item(&self) -> Option<&'a InMemDicomObject> {
        self.parent.map(|parent| parent.item)
    }

    pub(super) fn sop_class(&self) -> Option<String> {
        string_value(self.root, tags::SOP_CLASS_UID, 1)
    }

    /// Image Type (0008,0008) Value `index` (1-based) of the Image.
    pub(super) fn image_type_is(&self, index: usize, values: &[&str]) -> Tri {
        value_in(self.root, tags::IMAGE_TYPE, index, values)
    }

    pub(super) fn photometric_is(&self, value: &str) -> Tri {
        value_in(self.root, tags::PHOTOMETRIC_INTERPRETATION, 1, &[value])
    }

    pub(super) fn dimension_organization_type_is(&self, value: &str) -> Tri {
        value_in(self.root, tags::DIMENSION_ORGANIZATION_TYPE, 1, &[value])
    }

    /// Searches this Frame's Per-Frame Item first, then the Shared Item.
    pub(super) fn frame_groups(&self) -> impl Iterator<Item = &'a InMemDicomObject> {
        let frame = self.frame;
        frame
            .and_then(|frame| frame.per_frame)
            .into_iter()
            .chain(frame.and_then(|frame| frame.shared))
    }

    /// Items of the Shared and every Per-Frame Functional Groups Sequence Item.
    pub(super) fn all_functional_group_items(&self) -> impl Iterator<Item = &'a InMemDicomObject> {
        items(self.root, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
            .iter()
            .chain(items(self.root, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE))
    }
}

pub(super) fn tag_of(value: u32) -> Tag {
    Tag((value >> 16) as u16, (value & 0xFFFF) as u16)
}

pub(in crate::validation) fn raw_tag(tag: Tag) -> u32 {
    u32::from(tag.group()) << 16 | u32::from(tag.element())
}

pub(super) fn has(object: &InMemDicomObject, tag: Tag) -> bool {
    object.get(tag).is_some()
}

pub(super) fn present(object: &InMemDicomObject, tag: Tag) -> Tri {
    has(object, tag).into()
}

pub(super) fn absent(object: &InMemDicomObject, tag: Tag) -> Tri {
    (!has(object, tag)).into()
}

/// Present with at least one non-empty Value (PS3.5 7.4.1).
pub(super) fn has_value(object: &InMemDicomObject, tag: Tag) -> bool {
    object
        .get(tag)
        .is_some_and(|element| element_has_value(element.value()))
}

pub(super) fn element_has_value(value: &Value<InMemDicomObject>) -> bool {
    match value {
        Value::Sequence(sequence) => !sequence.items().is_empty(),
        Value::PixelSequence(_) => true,
        Value::Primitive(primitive) => primitive_has_value(primitive),
    }
}

fn primitive_has_value(value: &PrimitiveValue) -> bool {
    match value {
        PrimitiveValue::Empty => false,
        PrimitiveValue::Str(text) => text_has_value(text),
        PrimitiveValue::Strs(values) => values.iter().any(|text| text_has_value(text)),
        other => other.calculate_byte_len() > 0,
    }
}

/// Delimiters and padding alone are not a Value (PS3.5 7.4.1).
fn text_has_value(text: &str) -> bool {
    text.chars()
        .any(|character| !matches!(character, ' ' | '\0' | '^' | '='))
}

pub(in crate::validation) fn items(object: &InMemDicomObject, tag: Tag) -> &[InMemDicomObject] {
    object
        .get(tag)
        .and_then(|element| element.items())
        .unwrap_or(&[])
}

/// Value `index` (1-based) of a string Attribute, without padding.
pub(in crate::validation) fn string_value(
    object: &InMemDicomObject,
    tag: Tag,
    index: usize,
) -> Option<String> {
    let element = object.get(tag)?;
    let position = index.checked_sub(1)?;
    let trimmed = |value: &str| {
        let value = value.trim_matches(|character| character == ' ' || character == '\0');
        (!value.is_empty()).then(|| value.to_string())
    };
    // Text Values are read in place; other VRs go through their string form.
    match element.value() {
        Value::Primitive(PrimitiveValue::Str(value)) => {
            (position == 0).then_some(value.as_str()).and_then(trimmed)
        }
        Value::Primitive(PrimitiveValue::Strs(values)) => values
            .iter()
            .flat_map(|value| value.split('\\'))
            .nth(position)
            .and_then(trimmed),
        _ => trimmed(element.to_multi_str().ok()?.get(position)?),
    }
}

/// Whether Value `index` (1-based) is one of `values`; absent counts as not.
pub(super) fn value_in(object: &InMemDicomObject, tag: Tag, index: usize, values: &[&str]) -> Tri {
    Tri::from(
        string_value(object, tag, index).is_some_and(|value| values.contains(&value.as_str())),
    )
}

pub(in crate::validation) fn integer_value(object: &InMemDicomObject, tag: Tag) -> Option<i64> {
    object.get(tag)?.to_int::<i64>().ok()
}

/// Data Element Tag Values of an AT Attribute.
pub(in crate::validation) fn tag_values(object: &InMemDicomObject, tag: Tag) -> Vec<Tag> {
    match object.get(tag).map(|element| element.value()) {
        Some(Value::Primitive(PrimitiveValue::Tags(values))) => values.to_vec(),
        _ => Vec::new(),
    }
}

pub(in crate::validation) fn is_private(tag: Tag) -> bool {
    tag.group() % 2 == 1
}

/// The element's header-declared length, when defined.
pub(super) fn declared_length(element: &dicom_object::mem::InMemElement) -> Option<u32> {
    element.header().length().get()
}
