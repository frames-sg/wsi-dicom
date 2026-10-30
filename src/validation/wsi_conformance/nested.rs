//! One walk of an object's nested Items, shared by the rules that hold
//! content wherever Modules nest it: ICC Profiles (C.11.15.1.1) and Icon
//! Images (C.7.6.1.1.6, and F.7 in a Directory Record). Where an Item lies
//! is kept as Tags and indices, and formatted only for a finding.

use std::fmt;

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use crate::validation::iod::{element_label, MAX_NESTING};

/// What one walk of an object finds, at most `MAX_NESTING` Items deep.
pub(super) struct Nested<'a> {
    /// Each Data Set holding an ICC Profile, the top-level one first.
    pub(super) profiles: Vec<Found<'a>>,
    /// Each Item of an Icon Image Sequence.
    pub(super) icons: Vec<Found<'a>>,
}

/// A Data Set the walk found, and the Sequence and Item index (from 0) of
/// each step to it from the top-level Data Set.
pub(super) struct Found<'a> {
    pub(super) item: &'a InMemDicomObject,
    steps: Vec<(Tag, usize)>,
}

impl<'a> Nested<'a> {
    pub(super) fn of(object: &'a InMemDicomObject) -> Self {
        let mut nested = Self {
            profiles: Vec::new(),
            icons: Vec::new(),
        };
        nested.visit(object, &mut Vec::new());
        nested
    }

    fn visit(&mut self, data_set: &'a InMemDicomObject, steps: &mut Vec<(Tag, usize)>) {
        if data_set.element(tags::ICC_PROFILE).is_ok() {
            self.profiles.push(Found {
                item: data_set,
                steps: steps.clone(),
            });
        }
        for element in data_set.iter() {
            let Some(items) = element.items() else {
                continue;
            };
            let tag = element.header().tag;
            for (index, item) in items.iter().enumerate() {
                steps.push((tag, index));
                if tag == tags::ICON_IMAGE_SEQUENCE {
                    self.icons.push(Found {
                        item,
                        steps: steps.clone(),
                    });
                }
                if steps.len() <= MAX_NESTING {
                    self.visit(item, steps);
                }
                steps.pop();
            }
        }
    }
}

impl Found<'_> {
    /// Whether a Directory Record holds the Item (PS3.3 F.5).
    pub(super) fn in_record(&self) -> bool {
        self.steps
            .iter()
            .any(|(tag, _)| *tag == tags::DIRECTORY_RECORD_SEQUENCE)
    }

    /// Where the ICC rule says a Data Set is: "the top-level Data Set", or
    /// each step as "Optical Path item 0", joined by " > ".
    pub(super) fn profile_label(&self) -> String {
        if self.steps.is_empty() {
            return "the top-level Data Set".to_string();
        }
        self.steps
            .iter()
            .map(|(tag, index)| {
                let name = match *tag {
                    tags::OPTICAL_PATH_SEQUENCE => "Optical Path".to_string(),
                    tags::ICON_IMAGE_SEQUENCE => "Icon Image Sequence".to_string(),
                    tag => element_label("", tag),
                };
                format!("{name} item {index}")
            })
            .collect::<Vec<_>>()
            .join(" > ")
    }

    /// Where the pixel attributes rule says an Icon Image is, such as
    /// "DirectoryRecordSequence (0004,1220)[1] > Icon Image Sequence
    /// (0088,0200) Item 1", numbering Items from 1. It is formatted only
    /// when a message shows it.
    pub(super) fn icon_label(&self) -> IconLabel<'_> {
        IconLabel(&self.steps)
    }
}

pub(super) struct IconLabel<'s>(&'s [(Tag, usize)]);

impl fmt::Display for IconLabel<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(((_, index), within)) = self.0.split_last() else {
            return formatter.write_str("the top-level Data Set");
        };
        for (tag, index) in within {
            write!(formatter, "{}[{}] > ", element_label("", *tag), index + 1)?;
        }
        write!(
            formatter,
            "Icon Image Sequence (0088,0200) Item {}",
            index + 1
        )
    }
}
