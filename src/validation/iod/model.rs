//! Types for the generated PS3.3 requirement tables and PS3.6 dictionary.

use dicom_core::VR;

use super::tables::ConditionId;

/// Data Element Type from PS3.5 7.4, declared in the PS3.3 C.1 order so that
/// the lowest Type is the one that applies when Modules overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum AttributeType {
    One,
    OneC,
    Two,
    TwoC,
    Three,
}

impl AttributeType {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::OneC => "1C",
            Self::Two => "2",
            Self::TwoC => "2C",
            Self::Three => "3",
        }
    }

    /// Type 1 and 1C Data Elements need a non-empty Value when required.
    pub(super) fn requires_value(self) -> bool {
        matches!(self, Self::One | Self::OneC)
    }
}

/// Sequence Item count constraint parsed from the Attribute description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ItemCount {
    Unconstrained,
    Exactly(u32),
    AtLeast(u32),
    AtMost(u32),
    /// "The number of Items shall be the same as the number of Frames".
    NumberOfFrames,
    /// Same count as the Value of the first tag when present, else the second.
    ValueOf(u32, u32),
}

/// Enumerated Values for all Values, or for one 1-based Value, of an Attribute.
#[derive(Debug)]
pub(super) struct Enumeration {
    pub(super) value: Option<u8>,
    pub(super) condition: Option<ConditionId>,
    pub(super) values: &'static [&'static str],
}

#[derive(Debug)]
pub(super) struct Attribute {
    pub(super) tag: u32,
    pub(super) name: &'static str,
    pub(super) kind: AttributeType,
    pub(super) condition: Option<ConditionId>,
    pub(super) enumerations: &'static [Enumeration],
    pub(super) items: ItemCount,
    /// Item content of a Sequence Attribute.
    pub(super) children: &'static [Node],
}

#[derive(Debug)]
pub(super) enum Node {
    Attribute(Attribute),
    /// "Include Table ..." row: the Macro's rows at this nesting level.
    Include(&'static Table),
}

/// One PS3.3 Module or Macro table.
#[derive(Debug)]
pub(super) struct Table {
    /// "PS3.3" or "PS3.10".
    pub(super) part: &'static str,
    pub(super) label: &'static str,
    pub(super) section: &'static str,
    pub(super) nodes: &'static [Node],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Usage {
    Mandatory,
    Conditional,
    UserOption,
}

impl Usage {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Mandatory => "M",
            Self::Conditional => "C",
            Self::UserOption => "U",
        }
    }
}

/// Functional Group placement constraint from Table A.32.8-2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Placement {
    Any,
    SharedOnly,
    NotShared,
}

#[derive(Debug)]
pub(super) struct Module {
    pub(super) name: &'static str,
    pub(super) section: &'static str,
    pub(super) table: &'static Table,
    pub(super) usage: Usage,
    pub(super) condition: Option<ConditionId>,
    /// Top-level tags defined by no other Module of the IOD.
    pub(super) signature: &'static [u32],
}

#[derive(Debug)]
pub(super) struct FunctionalGroupMacro {
    pub(super) name: &'static str,
    pub(super) section: &'static str,
    pub(super) table: &'static Table,
    pub(super) usage: Usage,
    pub(super) placement: Placement,
    pub(super) condition: Option<ConditionId>,
    /// Functional Group Sequence Attributes defined by the Macro.
    pub(super) sequences: &'static [u32],
}

/// PS3.6 Value Multiplicity: `min`, `min-max`, or `min-n` stepping by `step`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ValueMultiplicity {
    pub(super) min: u32,
    pub(super) max: Option<u32>,
    pub(super) step: u32,
}

impl ValueMultiplicity {
    pub(super) fn admits(self, count: u32) -> bool {
        count >= self.min
            && self.max.is_none_or(|max| count <= max)
            && (count - self.min).is_multiple_of(self.step.max(1))
    }

    pub(super) fn describe(self) -> String {
        match self.max {
            Some(max) if max == self.min => self.min.to_string(),
            Some(max) => format!("{}-{max}", self.min),
            None if self.step > 1 => format!("{}-{}n", self.min, self.step),
            None => format!("{}-n", self.min),
        }
    }
}

#[derive(Debug)]
pub(super) struct DictionaryEntry {
    pub(super) tag: u32,
    /// Repeating-group entries zero the `xx` nibbles of `tag` and `mask`.
    pub(super) mask: u32,
    pub(super) vr: &'static [VR],
    pub(super) vm: ValueMultiplicity,
    pub(super) retired: bool,
}
