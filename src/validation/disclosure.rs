//! What a validation message shows of an Attribute's Value.
//!
//! Reports are pasted into tickets, CI logs and emails, so no message shows
//! the Value of an Attribute that the PS3.15 Table E.1-1 Basic Application
//! Level Confidentiality Profile removes or replaces (actions D, Z, X, Z/D,
//! X/Z, X/D, X/Z/D and X/Z/U*); nor, where a message knows the nesting, a
//! Value in the Items of a Sequence it removes or replaces whole ("remove all
//! Sequence Items and their contained Attributes"), such as the Code Meaning
//! of Person Identification Code Sequence (0040,1101) that names a person;
//! nor the Value of a Content Item whose Value Type is free text, a name or a
//! moment (TEXT, PNAME, DATE, TIME, DATETIME) or whose Concept Table E.3.4-1
//! removes or replaces. Such a Value is shown by its length, and an
//! offending character by its position and class. Identifying UIDs (action
//! U) are withheld too. Attributes the Profile keeps or does not list,
//! including SOP Class and Transfer Syntax UIDs, are shown, quoted.

use std::fmt;

mod external;
pub(super) use external::diagnostics as external_diagnostics;

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::iod::deidentification::{
    confidentiality_attribute, structured_content_concept, DeidentificationAction as A,
};
use super::iod::{items, string_value};

/// How a message names a compound Value it withholds whole, such as a coded
/// entry: "a coded entry (value withheld: identifying)".
pub(crate) const WITHHELD: &str = "(value withheld: identifying)";

/// Content Item Value Types whose Value is free text, a name or a moment.
/// The Profile replaces the Content Sequence (0040,A730) that holds them (D),
/// and Table E.3.4-1 removes them under Concepts of every kind.
const IDENTIFYING_VALUE_TYPES: &[&str] = &["TEXT", "PNAME", "DATE", "TIME", "DATETIME"];

/// Sequences the Profile removes or replaces whose Items' Values are judged
/// each on its own rather than wholesale: Content Items, by Value Type and
/// Concept; the Icon Image and the Digital Signatures, removed for the
/// pixels and signatures they carry, by tag.
const ITEMWISE_SEQUENCES: &[Tag] = &[
    tags::CONTENT_SEQUENCE,
    tags::SPECIMEN_PREPARATION_SEQUENCE,
    tags::ACQUISITION_CONTEXT_SEQUENCE,
    tags::ICON_IMAGE_SEQUENCE,
    tags::DIGITAL_SIGNATURES_SEQUENCE,
    tags::REFERENCED_DIGITAL_SIGNATURE_SEQUENCE,
    tags::REFERENCED_SOP_INSTANCE_MAC_SEQUENCE,
];

/// The Attributes that encode a Content Item's Value (PS3.3 Table 10-2 and
/// C.17.3).
const CONTENT_VALUES: &[Tag] = &[
    tags::TEXT_VALUE,
    tags::PERSON_NAME,
    tags::DATE,
    tags::TIME,
    tags::DATE_TIME,
    tags::UID,
    tags::NUMERIC_VALUE,
    tags::FLOATING_POINT_VALUE,
    tags::RATIONAL_NUMERATOR_VALUE,
    tags::RATIONAL_DENOMINATOR_VALUE,
    tags::MEASURED_VALUE_SEQUENCE,
    tags::CONCEPT_CODE_SEQUENCE,
    tags::REFERENCED_SOP_SEQUENCE,
];

/// Whether the Basic Profile removes or replaces a Value under `action`.
/// Matched without a wildcard, so a new action code fails to compile here.
const fn removes_or_replaces(action: A) -> bool {
    match action {
        A::D | A::Z | A::X | A::U | A::ZD | A::XZ | A::XD | A::XZD | A::XZUStar => true,
        A::K | A::C => false,
    }
}

/// Whether messages withhold the Value of `tag`: Table E.1-1's Basic
/// Profile removes or replaces it, or it is the Text Value (0040,A160) of a
/// TEXT Content Item. A Private Creator Data Element (gggg,0010-00FF) is
/// shown although its Private Attributes row is X: it names the block a
/// private defect is in, the public Attributes that repeat it (Dimension
/// Index Private Creator (0020,9213), Selector Sequence Pointer Private
/// Creator (0072,0054)) are not listed, and E.3.10 retains it with the safe
/// Private Attributes it defines.
pub(crate) fn withholds(tag: Tag) -> bool {
    if is_private_creator(tag) {
        return false;
    }
    tag == tags::TEXT_VALUE
        || confidentiality_attribute(tag).is_some_and(|row| removes_or_replaces(row.basic))
}

/// PS3.5 7.8.1: (gggg,0010-00FF) of an odd group gggg.
fn is_private_creator(tag: Tag) -> bool {
    tag.group() % 2 == 1 && (0x0010..=0x00FF).contains(&tag.element())
}

/// Whether messages withhold the Values nested in the Items of Sequence
/// `tag`: the Basic Profile removes or replaces it whole. X/Z/U* Sequences
/// of references and `ITEMWISE_SEQUENCES` are judged Value by Value.
fn withholds_items(tag: Tag) -> bool {
    !ITEMWISE_SEQUENCES.contains(&tag)
        && confidentiality_attribute(tag)
            .is_some_and(|row| row.basic != A::XZUStar && removes_or_replaces(row.basic))
}

/// Whether messages withhold the Value of a Content Item of `value_type`
/// whose Concept Name is `concept` (Coding Scheme Designator, Code Value).
pub(crate) fn withholds_content(value_type: &str, concept: Option<(&str, &str)>) -> bool {
    IDENTIFYING_VALUE_TYPES.contains(&value_type)
        || concept
            .and_then(|(scheme, value)| structured_content_concept(scheme, value, value_type))
            .is_some_and(|row| removes_or_replaces(row.basic))
}

/// Whether messages withhold the Value of Content Item `item`, from its
/// Value Type (0040,A040) and Concept Name Code Sequence (0040,A043); `false`
/// for an Item that is not a Content Item.
pub(crate) fn withholds_item_content(item: &InMemDicomObject) -> bool {
    let Some(value_type) = string_value(item, tags::VALUE_TYPE, 1) else {
        return false;
    };
    let concept = items(item, tags::CONCEPT_NAME_CODE_SEQUENCE).first();
    let field = |tag| concept.and_then(|concept| string_value(concept, tag, 1));
    let (scheme, value) = (
        field(tags::CODING_SCHEME_DESIGNATOR),
        field(tags::CODE_VALUE),
    );
    withholds_content(&value_type, scheme.as_deref().zip(value.as_deref()))
}

/// The Value of `tag` as a message shows it: quoted, or withheld.
pub(crate) fn shown(tag: Tag, value: &str) -> Shown<'_> {
    Shown::new(value, withholds(tag))
}

/// A Value whose Attribute the message does not know, withheld.
pub(crate) fn withheld(value: &str) -> Shown<'_> {
    Shown::new(value, true)
}

/// The Value of `tag` nested in an Item of Sequence `sequence` of the top
/// level Data Set, as a message shows it (`Nesting`).
pub(crate) fn shown_in(sequence: Tag, tag: Tag, value: &str) -> Shown<'_> {
    Nesting::default().below(sequence).shown(tag, value)
}

/// `shown` of a Value that may be absent, `absent` naming its absence.
pub(crate) fn shown_or(tag: Tag, value: Option<&str>, absent: &str) -> String {
    value.map_or_else(|| absent.to_string(), |value| shown(tag, value).to_string())
}

/// The Values of `tag` as a message lists them, each `shown`, joined by
/// `separator`.
pub(crate) fn shown_all<'a>(
    tag: Tag,
    values: impl IntoIterator<Item = &'a str>,
    separator: &str,
) -> String {
    values
        .into_iter()
        .map(|value| shown(tag, value).to_string())
        .collect::<Vec<_>>()
        .join(separator)
}

/// For two differing Values of `tag`, where they first differ when messages
/// withhold them; nothing when the Values are shown and say it themselves.
pub(crate) fn difference<'a>(tag: Tag, left: &'a str, right: &'a str) -> Difference<'a> {
    Difference {
        left,
        right,
        withheld: withholds(tag),
    }
}

/// Where in a Data Set a Value sits, as far as withholding it goes: below a
/// Sequence whose Items the Profile removes or replaces whole, or in a
/// Content Item whose Value Type or Concept identifies. Walkers of nested
/// Data Sets carry it down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Nesting {
    /// Below a Sequence whose Items messages withhold.
    items: bool,
    /// In a Content Item whose Value messages withhold.
    content: bool,
}

impl Nesting {
    /// Whether messages withhold every Value here, below a Sequence the
    /// Profile removes or replaces whole.
    pub(crate) fn withholds_every_value(self) -> bool {
        self.items
    }

    /// Whether messages withhold the Value of `tag` here.
    pub(crate) fn withholds(self, tag: Tag) -> bool {
        (self.content && CONTENT_VALUES.contains(&tag)) || withholds(tag) || self.items
    }

    /// The Value of `tag` here as a message shows it.
    pub(crate) fn shown(self, tag: Tag, value: &str) -> Shown<'_> {
        Shown::new(value, self.withholds(tag))
    }

    /// The Nesting of `item`, an Item of Sequence `sequence` here.
    pub(crate) fn item(self, sequence: Tag, item: &InMemDicomObject) -> Self {
        self.below(sequence).content_item(item)
    }

    /// This Nesting for `item`, a Content Item or other Item.
    pub(crate) fn content_item(self, item: &InMemDicomObject) -> Self {
        self.with_content(withholds_item_content(item))
    }

    /// This Nesting for an Item that is a Content Item whose Value messages
    /// withhold when `withheld` (`withholds_content`).
    pub(crate) fn with_content(self, withheld: bool) -> Self {
        Self {
            content: withheld,
            ..self
        }
    }

    /// The Nesting of an Item of Sequence `sequence` here that is not
    /// read as a Data Set (a Content Item's Value Type unknown).
    pub(crate) fn below(self, sequence: Tag) -> Self {
        Self {
            items: self.withholds_nested(sequence),
            content: false,
        }
    }

    fn withholds_nested(self, sequence: Tag) -> bool {
        self.items
            || withholds_items(sequence)
            || (self.content && CONTENT_VALUES.contains(&sequence))
    }
}

/// A Value as a message shows it. Shown: quoted and escaped as Rust quotes a
/// string, at most `limit` characters. Withheld: its length alone, e.g.
/// `(value withheld: identifying; 8 characters)`; an empty Value reveals
/// nothing and is shown.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shown<'a> {
    value: &'a str,
    withheld: bool,
    limit: Option<usize>,
}

impl<'a> Shown<'a> {
    fn new(value: &'a str, withheld: bool) -> Self {
        Self {
            value,
            withheld: withheld && !value.is_empty(),
            limit: None,
        }
    }

    /// Quotes at most `limit` characters of a shown Value, marking the cut
    /// with "..."; a withheld Value still counts all its characters.
    pub(crate) fn excerpt(self, limit: usize) -> Self {
        Self {
            limit: Some(limit),
            ..self
        }
    }

    /// Character `c`, at 1-based `position` of the Value, as a message names
    /// it: quoted when the Value is shown, else by position and class.
    pub(crate) fn character(self, c: char, position: usize) -> Character {
        Character {
            c,
            position,
            withheld: self.withheld,
        }
    }
}

impl fmt::Display for Shown<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.withheld {
            let count = self.value.chars().count();
            let plural = if count == 1 { "" } else { "s" };
            return write!(
                f,
                "(value withheld: identifying; {count} character{plural})"
            );
        }
        match self.limit {
            Some(limit) if self.value.chars().count() > limit => {
                let cut: String = self.value.chars().take(limit).collect();
                write!(f, "{:?}", format!("{cut}..."))
            }
            _ => write!(f, "{:?}", self.value),
        }
    }
}

/// An offending character of a Value (`Shown::character`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Character {
    c: char,
    position: usize,
    withheld: bool,
}

impl fmt::Display for Character {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.withheld {
            return write!(f, "{:?}", self.c);
        }
        write!(f, "{} (", self.position)?;
        let c = self.c;
        match c {
            _ if c.is_control() => write!(f, "control character U+{:04X}", u32::from(c))?,
            '\\' => f.write_str("backslash")?,
            ' ' => f.write_str("space")?,
            _ if c.is_ascii_uppercase() => f.write_str("upper case letter")?,
            _ if c.is_ascii_lowercase() => f.write_str("lower case letter")?,
            _ if c.is_ascii_digit() => f.write_str("digit")?,
            _ if c.is_ascii() => f.write_str("punctuation")?,
            _ if c.is_alphabetic() => f.write_str("non-ASCII letter")?,
            _ => f.write_str("non-ASCII character")?,
        }
        f.write_str(")")
    }
}

/// Where two withheld Values first differ (`difference`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Difference<'a> {
    left: &'a str,
    right: &'a str,
    withheld: bool,
}

impl fmt::Display for Difference<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.withheld || self.left == self.right {
            return Ok(());
        }
        let common = self
            .left
            .chars()
            .zip(self.right.chars())
            .take_while(|(left, right)| left == right)
            .count();
        write!(f, ", first differing at character {}", common + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_object::mem::InMemElement;

    #[test]
    fn identifying_attributes_are_withheld_and_others_shown() {
        for tag in [
            tags::PATIENT_NAME,
            tags::PATIENT_ID,
            tags::PATIENT_BIRTH_DATE,
            tags::PATIENT_SEX,
            tags::ACCESSION_NUMBER,
            tags::STUDY_ID,
            tags::STUDY_DATE,
            tags::CONTAINER_IDENTIFIER,
            tags::SPECIMEN_IDENTIFIER,
            tags::INSTITUTION_NAME,
            tags::DEVICE_SERIAL_NUMBER,
            tags::TEXT_VALUE,
            tags::PERSON_NAME,
            tags::STUDY_INSTANCE_UID,
            tags::SERIES_INSTANCE_UID,
            tags::SOP_INSTANCE_UID,
            tags::FRAME_OF_REFERENCE_UID,
            tags::REFERENCED_SOP_INSTANCE_UID,
            // An odd group: the Private Attributes row (X).
            Tag(0x0009, 0x1001),
        ] {
            assert!(withholds(tag), "{tag}");
            let shown = shown(tag, "DOE^JANE").to_string();
            assert_eq!(
                shown, "(value withheld: identifying; 8 characters)",
                "{tag}"
            );
        }
        for tag in [
            // The Private Creator of the block above.
            Tag(0x0009, 0x0010),
            tags::SOP_CLASS_UID,
            tags::TRANSFER_SYNTAX_UID,
            tags::MODALITY,
            tags::IMAGE_TYPE,
            tags::CODE_MEANING,
            tags::PIXEL_SPACING,
        ] {
            assert!(!withholds(tag), "{tag}");
            assert_eq!(shown(tag, "A\\B\n").to_string(), "\"A\\\\B\\n\"", "{tag}");
        }
    }

    #[test]
    fn a_withheld_value_shows_its_length_and_an_empty_one_shows_itself() {
        assert_eq!(
            shown(tags::PATIENT_ID, "M").to_string(),
            "(value withheld: identifying; 1 character)"
        );
        assert_eq!(shown(tags::PATIENT_ID, "").to_string(), "\"\"");
        let long = "X".repeat(70);
        assert_eq!(
            shown(tags::PATIENT_ID, &long).excerpt(48).to_string(),
            "(value withheld: identifying; 70 characters)"
        );
        assert_eq!(
            shown(tags::MODALITY, "ABCDEF").excerpt(3).to_string(),
            "\"ABC...\""
        );
    }

    #[test]
    fn an_offending_character_of_a_withheld_value_is_named_by_class() {
        let cases = [
            ('\u{7}', "3 (control character U+0007)"),
            ('\\', "3 (backslash)"),
            ('a', "3 (lower case letter)"),
            ('é', "3 (non-ASCII letter)"),
            ('€', "3 (non-ASCII character)"),
        ];
        for (c, expected) in cases {
            let withheld = shown(tags::PATIENT_NAME, "DOE").character(c, 3);
            assert_eq!(withheld.to_string(), expected);
            let named = shown(tags::MODALITY, "SM").character(c, 3);
            assert_eq!(named.to_string(), format!("{c:?}"));
        }
    }

    #[test]
    fn differing_withheld_values_say_where_they_differ() {
        assert_eq!(
            difference(tags::PATIENT_ID, "MRN12345", "MRN12399").to_string(),
            ", first differing at character 7"
        );
        assert_eq!(
            difference(tags::PATIENT_ID, "MRN", "MRN1").to_string(),
            ", first differing at character 4"
        );
        assert_eq!(difference(tags::MODALITY, "SM", "OT").to_string(), "");
    }

    #[test]
    fn content_items_are_withheld_by_value_type_and_concept() {
        for value_type in ["TEXT", "PNAME", "DATE", "TIME", "DATETIME"] {
            assert!(withholds_content(value_type, None), "{value_type}");
        }
        assert!(!withholds_content("CODE", Some(("DCM", "121071"))));
        // Table E.3.4-1: Subject Sex (121032, DCM) CODE and Subject Age
        // (121033, DCM) NUM are removed (X).
        assert!(withholds_content("CODE", Some(("DCM", "121032"))));
        assert!(withholds_content("NUM", Some(("DCM", "121033"))));
        assert!(!withholds_content("NUM", Some(("DCM", "121206"))));
    }

    fn content_item(value_type: &str, scheme: &str, value: &str) -> InMemDicomObject {
        let mut concept = InMemDicomObject::new_empty();
        concept.put(InMemElement::new(
            tags::CODING_SCHEME_DESIGNATOR,
            VR::SH,
            PrimitiveValue::from(scheme),
        ));
        concept.put(InMemElement::new(
            tags::CODE_VALUE,
            VR::SH,
            PrimitiveValue::from(value),
        ));
        let mut item = InMemDicomObject::new_empty();
        item.put(InMemElement::new(
            tags::VALUE_TYPE,
            VR::CS,
            PrimitiveValue::from(value_type),
        ));
        item.put(DataElement::new(
            tags::CONCEPT_NAME_CODE_SEQUENCE,
            VR::SQ,
            dicom_core::value::DataSetSequence::from(vec![concept]),
        ));
        item
    }

    #[test]
    fn nesting_withholds_below_removed_sequences_and_in_identifying_content_items() {
        let top = Nesting::default();
        let plain = InMemDicomObject::new_empty();
        assert!(!top.withholds(tags::CODE_MEANING));
        // Person Identification Code Sequence (0040,1101) is D: its Code
        // Meaning is a person's name (PS3.3 Table 10-1).
        let person = top.item(tags::PERSON_IDENTIFICATION_CODE_SEQUENCE, &plain);
        assert!(person.withholds(tags::CODE_MEANING));
        assert!(person
            .item(tags::CONCEPT_CODE_SEQUENCE, &plain)
            .withholds(tags::CODE_VALUE));
        // X/Z/U* references withhold identifying UIDs, but keep class UIDs.
        let source = top.item(tags::SOURCE_IMAGE_SEQUENCE, &plain);
        assert!(source.withholds(tags::REFERENCED_SOP_INSTANCE_UID));
        assert!(!source.withholds(tags::REFERENCED_SOP_CLASS_UID));
        // Content Items are judged by Value Type and Concept.
        let code = content_item("CODE", "DCM", "121071");
        let finding = top.item(tags::CONTENT_SEQUENCE, &code);
        assert!(!finding.withholds(tags::CODE_MEANING));
        assert!(!finding
            .item(tags::CONCEPT_CODE_SEQUENCE, &plain)
            .withholds(tags::CODE_MEANING));
        let text = content_item("TEXT", "DCM", "121071");
        assert!(top
            .item(tags::CONTENT_SEQUENCE, &text)
            .withholds(tags::TEXT_VALUE));
        let sex = content_item("CODE", "DCM", "121032");
        let sex = top.item(tags::CONTENT_SEQUENCE, &sex);
        assert!(sex
            .item(tags::CONCEPT_CODE_SEQUENCE, &plain)
            .withholds(tags::CODE_VALUE));
        assert!(!sex
            .item(tags::CONCEPT_NAME_CODE_SEQUENCE, &plain)
            .withholds(tags::CODE_VALUE));
        // A Content Item's own nesting starts afresh below its Content Sequence.
        let nested = sex.item(tags::CONTENT_SEQUENCE, &code);
        assert!(!nested.withholds(tags::CODE_MEANING));
    }
}
