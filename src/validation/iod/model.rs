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

/// Whether a Value Set is suggested (Baseline) or required (Defined); PS3.16 6.1
/// and the PS3.3 Definitions of BCID, DCID, BTID and DTID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Strength {
    Baseline,
    Defined,
}

/// The Value Set of a Code Sequence or Content Item Sequence.
#[derive(Clone, Copy, Debug)]
pub(super) enum ValueSet {
    /// Codes of any of these PS3.16 Context Groups.
    ContextGroups(Strength, &'static [u32]),
    /// Content Items of this PS3.16 Template.
    Template(Strength, u32),
}

#[derive(Debug)]
pub(super) struct Attribute {
    pub(super) tag: u32,
    pub(super) name: &'static str,
    pub(super) kind: AttributeType,
    pub(super) condition: Option<ConditionId>,
    pub(super) enumerations: &'static [Enumeration],
    pub(super) items: ItemCount,
    pub(super) value_set: Option<ValueSet>,
    /// Item content of a Sequence Attribute.
    pub(super) children: &'static [Node],
}

#[derive(Debug)]
pub(super) enum Node {
    Attribute(Attribute),
    /// "Include Table ..." row: the Macro's rows at this nesting level, with the
    /// Value Sets the including row binds to the Macro's Code Sequences.
    Include(&'static Table, &'static [(u32, ValueSet)]),
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

/// One normative sentence of the standard, as extracted.
#[derive(Clone, Copy, Debug)]
pub(super) struct Statement {
    pub(super) part: &'static str,
    pub(super) section: &'static str,
    /// The Attribute whose description holds the sentence.
    pub(super) keyword: Option<&'static str>,
    pub(super) text: &'static str,
}

/// A PS3.16 coded concept; `legacy` is its SNOMED-RT identifier, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Code {
    pub(super) scheme: &'static str,
    pub(super) value: &'static str,
    pub(super) meaning: &'static str,
    pub(super) legacy: &'static str,
}

#[derive(Debug)]
pub(super) struct ContextGroup {
    pub(super) id: u32,
    pub(super) name: &'static str,
    pub(super) extensible: bool,
    /// Coding Schemes the group comprises whole (e.g. UCUM for CID 82).
    pub(super) schemes: &'static [&'static str],
    /// Sorted by Coding Scheme Designator, then Code Value.
    pub(super) codes: &'static [Code],
}

/// The Concept Name a Template row constrains.
#[derive(Clone, Copy, Debug)]
pub(super) enum Concept {
    /// EV: exactly this code.
    Enumerated(Code),
    /// DT: this code, or one of the same meaning.
    Defined(Code),
    /// A Context Group of names, or for an INCLUDE row the included Template.
    Set(ValueSet),
}

/// PS3.16 6.1.7 Requirement Type of a Template row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowRequirement {
    M,
    Mc,
    U,
}

/// PS3.16 6.1.8 Condition of a Template row; `iff` forbids the row otherwise.
#[derive(Clone, Copy, Debug)]
pub(super) enum TemplateCondition {
    RowAbsent {
        iff: bool,
        row: &'static str,
    },
    RowValue {
        iff: bool,
        row: &'static str,
        scheme: &'static str,
        value: &'static str,
    },
}

#[derive(Debug)]
pub(super) struct TemplateRow {
    pub(super) row: &'static str,
    pub(super) value_type: &'static str,
    pub(super) concept: Concept,
    pub(super) min: u32,
    pub(super) max: Option<u32>,
    pub(super) requirement: RowRequirement,
    pub(super) condition: Option<TemplateCondition>,
    pub(super) constraint: Option<ValueSet>,
}

#[derive(Debug)]
pub(super) struct Template {
    pub(super) id: u32,
    pub(super) name: &'static str,
    pub(super) extensible: bool,
    /// Content Items appear in row order.
    pub(super) ordered: bool,
    pub(super) rows: &'static [TemplateRow],
}
