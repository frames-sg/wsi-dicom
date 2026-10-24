//! PS3.15 Annex E Attribute Confidentiality Profiles: an object whose Patient
//! Identity Removed (0012,0062) is YES, or that names a De-identification
//! Method, is held to every requirement of the Basic Application Level
//! Confidentiality Profile (E.1.1, E.2) and of the Options its
//! De-identification Method Code Sequence (0012,0064) declares (E.3) that the
//! object itself can show. What only the object before de-identification
//! could show (whether a Value is a dummy, was cleaned or was kept) is not
//! claimed; `actions` reports only what the file proves.

mod actions;
mod encrypted;
mod set;

use std::collections::BTreeSet;
use std::path::Path;

use dicom_core::value::Value;
use dicom_core::PrimitiveValue;
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::disclosure::{shown, shown_or};
use super::iod::deidentification::{ConfidentialityOption, BASIC_PROFILE};
use super::iod::{dicom_edition, items, string_value, Coded, Family};
use super::wsi_conformance::rule_path;
use super::{intrinsic_check, ValidationCheck, ValidationStatus};

pub(in crate::validation) use set::{run_deidentification_set_check, DeidentificationCorpus};

pub(in crate::validation) const DEIDENTIFICATION_RULE: &str =
    "intrinsic-wsi-dicom-deidentification";
const CITATION: &str = concat!("DICOM PS3.15 ", dicom_edition!(), " Annex E");

/// Advisories shown in a message; the rest are counted.
const MAX_SHOWN: usize = 20;
/// Codes outside CID 7050 an advisory names; the rest are counted.
const MAX_OTHER_CODES: usize = 5;
/// The top-level Attributes that hold an image's pixels (PS3.3 C.7.6.3,
/// C.7.6.24 and C.7.6.25).
const PIXEL_DATA_TAGS: [dicom_core::Tag; 3] = [
    tags::PIXEL_DATA,
    tags::FLOAT_PIXEL_DATA,
    tags::DOUBLE_FLOAT_PIXEL_DATA,
];

/// What an object declares of its de-identification (PS3.15 E.1.1).
#[derive(Debug, Default)]
pub(super) struct Declaration {
    /// Patient Identity Removed (0012,0062), without padding.
    identity_removed: Option<String>,
    /// De-identification Method (0012,0063) has a Value.
    described: bool,
    /// De-identification Method Code Sequence (0012,0064) has Items.
    coded: bool,
    /// (113100, DCM) is among the codes.
    basic: bool,
    options: BTreeSet<ConfidentialityOption>,
    /// The first `MAX_OTHER_CODES` codes outside CID 7050, as encoded.
    other_codes: Vec<String>,
    /// How many codes outside CID 7050 are encoded.
    other_count: usize,
    /// 1-based Items of (0012,0064) without a Code Value.
    uncoded_items: Vec<usize>,
}

impl Declaration {
    pub(super) fn of(object: &InMemDicomObject) -> Self {
        let mut declaration = Self {
            identity_removed: string_value(object, tags::PATIENT_IDENTITY_REMOVED, 1),
            described: string_value(object, tags::DEIDENTIFICATION_METHOD, 1).is_some(),
            ..Self::default()
        };
        let methods = items(object, tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE);
        declaration.coded = !methods.is_empty();
        for (index, item) in methods.iter().enumerate() {
            let Some(coded) = Coded::of(item) else {
                declaration.uncoded_items.push(index + 1);
                continue;
            };
            if coded.is(BASIC_PROFILE.scheme, BASIC_PROFILE.value) {
                declaration.basic = true;
            } else if let Some(option) = ConfidentialityOption::ALL
                .iter()
                .copied()
                .find(|option| coded.is(option.code().scheme, option.code().value))
            {
                declaration.options.insert(option);
            } else {
                declaration.other_count += 1;
                if declaration.other_codes.len() < MAX_OTHER_CODES {
                    declaration.other_codes.push(describe(item));
                }
            }
        }
        declaration
    }

    /// Patient Identity Removed is YES, or a De-identification Method is
    /// named.
    pub(super) fn claimed(&self) -> bool {
        self.identity_removed.as_deref() == Some("YES") || self.described || self.coded
    }

    /// Whether the Options applied are known: at least one CID 7050 code is
    /// encoded. A text description alone does not say which Options apply.
    pub(super) fn options_known(&self) -> bool {
        self.basic || !self.options.is_empty()
    }

    pub(super) fn has(&self, option: ConfidentialityOption) -> bool {
        self.options.contains(&option)
    }
}

/// A coded entry as its message names it.
fn describe(item: &InMemDicomObject) -> String {
    let field = |tag| string_value(item, tag, 1).unwrap_or_default();
    format!(
        "({}, {}, \"{}\")",
        field(tags::CODE_VALUE),
        field(tags::CODING_SCHEME_DESIGNATOR),
        field(tags::CODE_MEANING)
    )
}

/// What the rule found in one object.
#[derive(Debug, Default)]
pub(super) struct Findings {
    /// Bounded: a message is formatted only while it will be shown.
    pub(super) violations: Family,
    /// Departures from the standard's advice and conflicts within it, which
    /// are never failures: at most one of each kind per object.
    pub(super) advisories: Vec<String>,
}

/// The de-identification check of one object of a pathology-slide IOD or
/// an A.35 KOS or SR document.
pub(in crate::validation) fn run_deidentification_check(
    label: &Path,
    object: &DefaultDicomObject,
) -> ValidationCheck {
    let declaration = Declaration::of(object);
    let mut findings = Findings::default();
    // E.1.1 binds an Encrypted Attributes Sequence wherever it is used.
    encrypted::check_encrypted_attributes(object, &mut findings);
    if declaration.claimed() {
        check_declaration(&declaration, &mut findings);
        check_option_attributes(object, &declaration, &mut findings);
        actions::check_data_set(object, &declaration, &mut findings);
        check_label_pixels(object, &declaration, &mut findings);
    } else if findings.violations.is_empty() {
        return intrinsic_check(
            DEIDENTIFICATION_RULE,
            rule_path(label),
            ValidationStatus::Passed,
            format!(
                "{CITATION}: Patient Identity Removed is not YES; no de-identification is claimed"
            ),
        );
    }
    report(
        label,
        &findings.violations,
        findings.advisories.iter().cloned(),
    )
}

/// The check of `violations` at `label` (an empty label for the set), its
/// message ending in `advisories`, which never fail it.
fn report(
    label: &Path,
    violations: &Family,
    advisories: impl ExactSizeIterator<Item = String>,
) -> ValidationCheck {
    let mut check = violations.check(label, DEIDENTIFICATION_RULE, CITATION);
    check.message.push_str(&advisory_suffix(advisories));
    check
}

/// "; advisory: " and the first `MAX_SHOWN` of `advisories`, then how many
/// more; "" when there are none. Only the advisories shown are formatted.
fn advisory_suffix(advisories: impl ExactSizeIterator<Item = String>) -> String {
    let total = advisories.len();
    if total == 0 {
        return String::new();
    }
    let shown: Vec<String> = advisories.take(MAX_SHOWN).collect();
    let mut suffix = format!("; advisory: {}", shown.join("; "));
    if total > MAX_SHOWN {
        suffix.push_str(&format!("; and {} more", total - MAX_SHOWN));
    }
    suffix
}

/// PS3.15 E.1.1 and E.1.2: Patient Identity Removed YES, with the method
/// named by CID 7050 codes and/or text.
fn check_declaration(declaration: &Declaration, findings: &mut Findings) {
    match declaration.identity_removed.as_deref() {
        Some("YES") => {}
        Some("NO") => findings.violations.push(|| {
            "Patient Identity Removed (0012,0062) is NO while a De-identification Method is named; E.1.1 requires YES on a de-identified object, and E.1.2 requires a re-identified one to remove De-identification Method (0012,0063) and De-identification Method Code Sequence (0012,0064)"
                .to_string()
        }),
        Some(other) => findings.violations.push(|| {
            format!(
                "Patient Identity Removed (0012,0062) is {} while a De-identification Method is named; E.1.1 requires YES",
                shown(tags::PATIENT_IDENTITY_REMOVED, other)
            )
        }),
        None => findings.violations.push(|| {
            "Patient Identity Removed (0012,0062) is absent while a De-identification Method is named; E.1.1 requires it with the Value YES"
                .to_string()
        }),
    }
    if !declaration.described && !declaration.options_known() {
        findings.violations.push(|| {
            "neither a CID 7050 code in De-identification Method Code Sequence (0012,0064) nor a De-identification Method (0012,0063) text names the Profile and Options used; E.1.1 requires one or both"
                .to_string()
        });
    }
    for index in &declaration.uncoded_items {
        findings.violations.push(|| {
            format!(
                "De-identification Method Code Sequence (0012,0064) Item {index} has no Code Value; E.1.1 requires codes from CID 7050"
            )
        });
    }
    if declaration.other_count > 0 {
        let hidden = declaration.other_count - declaration.other_codes.len();
        findings.advisories.push(format!(
            "De-identification Method Code Sequence (0012,0064) holds {}{} outside CID 7050, which extends the extensible group; what they protect is not checked",
            declaration.other_codes.join(", "),
            if hidden > 0 {
                format!(" and {hidden} more codes")
            } else {
                String::new()
            }
        ));
    }
    if !declaration.options.is_empty() && !declaration.basic {
        findings.advisories.push(
            "De-identification Method Code Sequence (0012,0064) names Options without (113100, DCM, \"Basic Application Confidentiality Profile\"); E.3 specifies each Option in addition to the Basic Profile, whose code E.1.1 adds"
                .to_string(),
        );
    }
    if !declaration.options_known() && declaration.described {
        findings.advisories.push(
            "the Options applied are named by De-identification Method (0012,0063) text alone, so only the actions every Option of Table E.1-1 shares are checked"
                .to_string(),
        );
    }
}

/// The Attributes the Basic Profile and each declared Option add (E.2,
/// E.3.1, E.3.2, E.3.6).
fn check_option_attributes(
    object: &InMemDicomObject,
    declaration: &Declaration,
    findings: &mut Findings,
) {
    use ConfidentialityOption::{
        RetainLongitudinalTemporalInformationFullDates as FullDates,
        RetainLongitudinalTemporalInformationModifiedDates as ModifiedDates,
    };
    check_pixel_options(object, declaration, findings);
    let temporal = (
        tags::LONGITUDINAL_TEMPORAL_INFORMATION_MODIFIED,
        "Longitudinal Temporal Information Modified (0028,0303)",
    );
    if !declaration.options_known() {
        let value = string_value(object, temporal.0, 1);
        if !matches!(
            value.as_deref(),
            Some("REMOVED" | "MODIFIED" | "UNMODIFIED")
        ) {
            findings.violations.push(|| {
                format!(
                    "{} is {}; E.2 and E.3.6 add it with REMOVED, MODIFIED or UNMODIFIED whichever Options apply",
                    temporal.1,
                    shown_or(temporal.0, value.as_deref(), "absent")
                )
            });
        }
        return;
    }
    match (declaration.has(FullDates), declaration.has(ModifiedDates)) {
        (true, true) => findings.violations.push(|| {
            "De-identification Method Code Sequence (0012,0064) declares both the Full Dates and the Modified Dates Options; E.3.6 specifies them as mutually exclusive"
                .to_string()
        }),
        (true, false) => require_value(
            object,
            temporal,
            "UNMODIFIED",
            "E.3.6 Retain Longitudinal Temporal Information Full Dates Option",
            findings,
        ),
        (false, true) => require_value(
            object,
            temporal,
            "MODIFIED",
            "E.3.6 Retain Longitudinal Temporal Information Modified Dates Option",
            findings,
        ),
        (false, false) => require_value(
            object,
            temporal,
            "REMOVED",
            "E.2, as no Retain Longitudinal Temporal Information Option is declared,",
            findings,
        ),
    }
}

/// E.3.1 and E.3.2: the Clean Pixel Data and Clean Recognizable Visual
/// Features Options add Burned In Annotation (0028,0301) and Recognizable
/// Visual Features (0028,0302) NO. Both act on the Pixel Data, and E.3.1
/// says "This Option is intended to apply to the Pixel Data (7FE0,0010)
/// Attribute that occurs in the top level Data Set of an Image Storage SOP
/// Instance": the pixels of an image, which a Parametric Map holds in Float
/// or Double Float Pixel Data. For an object with none of them (a KOS or
/// SR document, Bulk Annotations) an advisory says the Options' Attributes
/// do not apply, never a failure.
fn check_pixel_options(
    object: &InMemDicomObject,
    declaration: &Declaration,
    findings: &mut Findings,
) {
    let options = [
        (
            ConfidentialityOption::CleanPixelData,
            (
                tags::BURNED_IN_ANNOTATION,
                "Burned In Annotation (0028,0301)",
            ),
            "E.3.1 Clean Pixel Data Option",
            "E.3.1: \"This Option is intended to apply to the Pixel Data (7FE0,0010) Attribute that occurs in the top level Data Set of an Image Storage SOP Instance\"",
        ),
        (
            ConfidentialityOption::CleanRecognizableVisualFeatures,
            (
                tags::RECOGNIZABLE_VISUAL_FEATURES,
                "Recognizable Visual Features (0028,0302)",
            ),
            "E.3.2 Clean Recognizable Visual Features Option",
            "E.3.2 acts on \"visual information within the Pixel Data\"",
        ),
    ];
    let pixels = PIXEL_DATA_TAGS.iter().any(|tag| object.get(*tag).is_some());
    for (option, attribute, clause, scope) in options {
        if !declaration.has(option) {
            continue;
        }
        if pixels {
            require_value(object, attribute, "NO", clause, findings);
        } else {
            findings.advisories.push(format!(
                "the {clause} is declared, but the object has no Pixel Data (7FE0,0010), Float Pixel Data (7FE0,0008) or Double Float Pixel Data (7FE0,0009) in its top level Data Set, so its {} NO does not apply ({scope})",
                attribute.1
            ));
        }
    }
}

fn require_value(
    object: &InMemDicomObject,
    (tag, name): (dicom_core::Tag, &str),
    required: &str,
    clause: &str,
    findings: &mut Findings,
) {
    let value = string_value(object, tag, 1);
    if value.as_deref() != Some(required) {
        findings.violations.push(|| {
            format!(
                "{name} is {}; {clause} adds it with the Value {required}",
                shown_or(tag, value.as_deref(), "absent")
            )
        });
    }
}

/// PS3.15 E.2 Note: unless the Clean Pixel Data Option is specified, the
/// Profile does not address information in the pixels, and a LABEL or
/// OVERVIEW image shows the slide label (PS3.3 C.8.12.4.1.1).
fn check_label_pixels(
    object: &InMemDicomObject,
    declaration: &Declaration,
    findings: &mut Findings,
) {
    if declaration.has(ConfidentialityOption::CleanPixelData) {
        return;
    }
    let Some(flavor) = string_value(object, tags::IMAGE_TYPE, 3)
        .filter(|flavor| matches!(flavor.as_str(), "LABEL" | "OVERVIEW"))
    else {
        return;
    };
    findings.advisories.push(format!(
        "Image Type (0008,0008) Value 3 is {flavor} and the Clean Pixel Data Option (113101, DCM) is not declared: the label pixels are not covered by the declared options (E.2 Note: the Basic Profile does not address information in the pixels)"
    ));
}

/// The Value is absent in effect: zero length, or padding alone (PS3.5
/// 6.2), or a Sequence without Items.
pub(super) fn is_empty(element: &InMemElement) -> bool {
    let blank = |text: &str| text.trim_matches([' ', '\0']).is_empty();
    match element.value() {
        Value::Sequence(sequence) => sequence.items().is_empty(),
        Value::PixelSequence(_) => false,
        Value::Primitive(PrimitiveValue::Empty) => true,
        Value::Primitive(PrimitiveValue::Str(text)) => blank(text),
        Value::Primitive(PrimitiveValue::Strs(values)) => values.iter().all(|text| blank(text)),
        Value::Primitive(primitive) => primitive.calculate_byte_len() == 0,
    }
}

#[cfg(test)]
mod tests;
