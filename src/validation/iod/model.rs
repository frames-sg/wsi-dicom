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
pub(in crate::validation) enum Strength {
    Baseline,
    Defined,
}

/// The Value Set of a Code Sequence or Content Item Sequence.
#[derive(Clone, Copy, Debug)]
pub(in crate::validation) enum ValueSet {
    /// Codes of any of these PS3.16 Context Groups.
    ContextGroups(Strength, &'static [u32]),
    /// Content Items of this PS3.16 Template.
    Template(Strength, u32),
    /// This one code, which the IOD Content Constraint of this section requires.
    Code(Code, &'static str),
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
    /// "Include Table ..." row: the Macro's rows at this nesting level. A
    /// row binding Value Sets in the Macro includes a copy that carries them.
    Include(&'static Table),
    /// "Include Table ... if and only if ..." (Tables C.17-5 and C.17-6): the
    /// Macro's rows in a Data Set for which the condition is Required.
    IncludeIf(&'static Table, ConditionId),
    /// "Include Record Selection Keys" (Table F.3-3): the rows of the table
    /// that the Data Set's Value of the selection key names.
    Select(&'static Selection),
}

/// The table a Data Set includes by the Value of one of its Attributes.
#[derive(Debug)]
pub(super) struct Selection {
    pub(super) key: u32,
    /// Each Value and the table it selects.
    pub(super) tables: &'static [(&'static str, &'static Table)],
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

/// Functional Group placement constraint from the IOD's Functional Group
/// Macros table.
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
    /// Top-level tags defined by no other Module of the IOD; empty when other
    /// Modules define all its Attributes.
    pub(super) signature: &'static [u32],
}

/// A Module an IOD Content Constraint forbids, identified by the top-level
/// Attributes no Module of the IOD defines.
#[derive(Debug)]
pub(super) struct ForbiddenModule {
    pub(super) name: &'static str,
    pub(super) section: &'static str,
    /// The section stating the constraint.
    pub(super) constraint: &'static str,
    pub(super) signature: &'static [u32],
}

/// Modules of which an IOD Content Constraint requires exactly one.
#[derive(Debug)]
pub(super) struct ExclusiveModules {
    pub(super) constraint: &'static str,
    pub(super) modules: &'static [&'static str],
}

/// A Storage SOP Class of PS3.4 Table B.5-1.
#[derive(Debug)]
pub(super) struct SopClass {
    pub(super) uid: &'static str,
    pub(super) name: &'static str,
}

/// One PS3.3 Annex A IOD: its Module and Functional Group Macro tables and
/// the IOD Content Constraints the tables cannot express.
#[derive(Debug)]
pub(super) struct Iod {
    /// "A.32.8".
    pub(super) section: &'static str,
    pub(super) name: &'static str,
    pub(super) module_table: &'static str,
    pub(super) functional_group_table: Option<&'static str>,
    pub(super) sop_classes: &'static [SopClass],
    pub(super) modules: &'static [Module],
    pub(super) functional_groups: &'static [FunctionalGroupMacro],
    pub(super) forbidden_modules: &'static [ForbiddenModule],
    pub(super) exclusive_modules: &'static [ExclusiveModules],
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
    /// The sections of the IODs whose scope holds the sentence.
    pub(super) iods: &'static [&'static str],
    pub(super) strength: crate::validation::RequirementStrength,
}

/// A PS3.16 coded concept; `legacy` is its SNOMED-RT identifier, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::validation) struct Code {
    pub(in crate::validation) scheme: &'static str,
    pub(in crate::validation) value: &'static str,
    pub(in crate::validation) meaning: &'static str,
    pub(in crate::validation) legacy: &'static str,
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
pub(in crate::validation) enum Concept {
    /// EV: exactly this code.
    Enumerated(Code),
    /// DT: this code, or one of the same meaning.
    Defined(Code),
    /// A Context Group of names, or for an INCLUDE row the included Template.
    Set(ValueSet),
    /// No Concept Name constraint (e.g. an IMAGE row of TID 2010).
    Any,
    /// The Template parameter that the including row binds, e.g. "$Measurement".
    Parameter(&'static str),
}

/// PS3.16 6.1.7 Requirement Type of a Template row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::validation) enum RowRequirement {
    M,
    Mc,
    U,
    Uc,
}

/// PS3.16 6.1.8 Condition of a Template row; `iff` forbids the row otherwise.
#[derive(Clone, Copy, Debug)]
pub(in crate::validation) enum TemplateCondition {
    /// "IF[F] Row N is absent".
    Absent { iff: bool, row: &'static str },
    /// "IF[F] Row N is present". Modelled whether or not a Template the IODs
    /// reach has such a condition in this edition.
    #[allow(dead_code)]
    Present { iff: bool, row: &'static str },
    /// "IF[F] Row N value is (code)".
    HasCode {
        iff: bool,
        row: &'static str,
        scheme: &'static str,
        value: &'static str,
    },
}

#[derive(Debug)]
pub(in crate::validation) struct TemplateRow {
    pub(in crate::validation) row: &'static str,
    /// Nesting Level (NL): 0 for a row at the Template's top, n for one under
    /// n ">", a Target Content Item of the nearest row above it one level up.
    pub(in crate::validation) level: u8,
    /// Relationship Type with its parent Content Item ("Rel with Parent"),
    /// e.g. "CONTAINS"; empty where the Template gives none.
    // Read by the SR content rule through `structured_report`.
    #[allow(dead_code)]
    pub(in crate::validation) relationship: &'static str,
    pub(in crate::validation) value_type: &'static str,
    pub(in crate::validation) concept: Concept,
    pub(in crate::validation) min: u32,
    pub(in crate::validation) max: Option<u32>,
    pub(in crate::validation) requirement: RowRequirement,
    /// `condition_text` where it has one of the forms the IOD checks evaluate.
    pub(in crate::validation) condition: Option<TemplateCondition>,
    /// The Condition cell as PS3.16 words it; empty if none.
    // Read by the SR content rule through `structured_report`.
    #[allow(dead_code)]
    pub(in crate::validation) condition_text: &'static str,
    /// `constraint_text` where it names only Context Groups.
    pub(in crate::validation) constraint: Option<ValueSet>,
    /// The Value Set Constraint cell as PS3.16 words it, parameters
    /// included; empty if none.
    // Read by the SR content rule through `structured_report`.
    #[allow(dead_code)]
    pub(in crate::validation) constraint_text: &'static str,
}

#[derive(Debug)]
pub(in crate::validation) struct Template {
    pub(in crate::validation) id: u32,
    pub(in crate::validation) name: &'static str,
    pub(in crate::validation) extensible: bool,
    /// Content Items appear in row order.
    pub(in crate::validation) ordered: bool,
    pub(in crate::validation) rows: &'static [TemplateRow],
}
