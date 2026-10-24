//! Constraints between Attributes that the generated tables do not express,
//! checked in the Data Set and every nested Item in every profile: the
//! Selector Attribute Macro (PS3.3 10.17), coded entries (8.6), the person and
//! measurement macros (Tables 10-1, 10-2 and 10-3), patient, equipment and
//! image descriptions (C.2.2.1.1, C.7.1.4.1.1.1, C.7.5.1.1.1, C.7.6.1.1.1 and
//! Tables C.7-3, C.7-5a and C.7-9), specimen preparation (C.7.6.22.1.3), color
//! (Table C.8.12.5-1), and the SOP Common and General Reference Modules
//! (C.12.1.1.3.1.1, C.12.1.1.6, C.12.1.1.9 and C.12.4.1.2).

use dicom_core::value::Value;
use dicom_core::{Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::wsi_image::{is_code, sequence, string, strings, tag_values};
use crate::validation::digital_signatures::excluded_from_signature;
use crate::validation::iod::{element_label, is_private};
use crate::validation::{check_value_format, is_image_storage_sop_class, registered};

const CITATION: &str = "PS3.3 2026c";
/// DateTime of processing, TID 8001 Specimen Preparation.
const PROCESSING_DATETIME: (&str, &str) = ("DCM", "111702");
/// Root of the HL7 OID registry, which assigns the Hierarchical Message
/// Description Object Identifiers (C.12.1.1.6).
const HL7_ROOT: &str = "2.16.840.1.113883.";
/// VL Photographic Image and Secondary Capture Image Storage (C.2.2.1.1).
const PATIENT_PHOTO_CLASSES: [&str; 2] = [
    "1.2.840.10008.5.1.4.1.1.77.1.4",
    "1.2.840.10008.5.1.4.1.1.7",
];
/// Anatomical direction abbreviations by Anatomical Orientation Type
/// (C.7.6.1.1.1).
const BIPED: [&str; 6] = ["A", "P", "R", "L", "H", "F"];
const QUADRUPED: [&str; 13] = [
    "LE", "RT", "D", "V", "CR", "CD", "R", "M", "L", "PR", "DI", "PA", "PL",
];
/// Laterality modifier codes (CID 244) and the Image Laterality each means.
const LATERALITY_CODES: [(&str, &str, &str); 6] = [
    ("SCT", "24028007", "R"),
    ("SCT", "7771000", "L"),
    ("SCT", "51440002", "B"),
    ("SRT", "G-A100", "R"),
    ("SRT", "G-A101", "L"),
    ("SRT", "G-A102", "B"),
];
/// (name, Identification Sequence, citation): the Items follow the Values of
/// the name they identify.
const IDENTIFIED_NAMES: [(Tag, Tag, &str); 5] = [
    (
        tags::CONSULTING_PHYSICIAN_NAME,
        tags::CONSULTING_PHYSICIAN_IDENTIFICATION_SEQUENCE,
        "Table C.7-3",
    ),
    (
        tags::PHYSICIANS_OF_RECORD,
        tags::PHYSICIANS_OF_RECORD_IDENTIFICATION_SEQUENCE,
        "Table C.7-3",
    ),
    (
        tags::NAME_OF_PHYSICIANS_READING_STUDY,
        tags::PHYSICIANS_READING_STUDY_IDENTIFICATION_SEQUENCE,
        "Table C.7-3",
    ),
    (
        tags::PERFORMING_PHYSICIAN_NAME,
        tags::PERFORMING_PHYSICIAN_IDENTIFICATION_SEQUENCE,
        "Table C.7-5a",
    ),
    (
        tags::OPERATORS_NAME,
        tags::OPERATOR_IDENTIFICATION_SEQUENCE,
        "Table C.7-5a",
    ),
];

pub(super) fn validate_relations_rule(object: &DefaultDicomObject) -> Result<(), String> {
    let root: &InMemDicomObject = object;
    let mut violations = Vec::new();
    walk(root, &mut Vec::new(), &mut violations);
    patient_orientation(root, &mut violations);
    image_laterality(root, &mut violations);
    references(root, &mut violations);
    super::violation_result(violations)
}

/// The Sequence Items enclosing a Data Set, outermost first.
type Trail = Vec<(Tag, usize)>;

/// Checks `data_set` and, recursively, every Item of its Sequences.
fn walk(data_set: &InMemDicomObject, trail: &mut Trail, violations: &mut Vec<String>) {
    item_relations(data_set, trail, violations);
    for element in data_set.iter() {
        let Some(items) = element.items() else {
            continue;
        };
        for (index, item) in items.iter().enumerate() {
            trail.push((element.header().tag, index));
            walk(item, trail, violations);
            trail.pop();
        }
    }
}

/// `Keyword (gggg,eeee)`, without a keyword for unregistered tags.
fn label(tag: Tag) -> String {
    element_label("", tag)
}

/// `Sequence[n] > ` for each enclosing Item, formatted only for a message.
fn path(trail: &Trail) -> String {
    trail
        .iter()
        .map(|(tag, index)| format!("{}[{}] > ", label(*tag), index + 1))
        .collect()
}

fn item_relations(data_set: &InMemDicomObject, trail: &Trail, violations: &mut Vec<String>) {
    let within = trail.last().map(|(tag, _)| *tag);
    let mut found = Vec::new();
    selector(data_set, &mut found);
    context_identifier(data_set, &mut found);
    referenced_frames(data_set, &mut found);
    identification_items(data_set, within, &mut found);
    calibration(data_set, &mut found);
    subject_position(data_set, &mut found);
    color_space(data_set, &mut found);
    preparation_order(data_set, &mut found);
    signed_elements(data_set, &mut found);
    original_attributes(data_set, &mut found);
    measured_value(data_set, &mut found);
    if within == Some(tags::PERSON_IDENTIFICATION_CODE_SEQUENCE) {
        person_code(data_set, &mut found);
    }
    if !found.is_empty() {
        let path = path(trail);
        violations.extend(found.into_iter().map(|message| format!("{path}{message}")));
    }
}

/// The Values of an Attribute, or none when it is absent.
fn values(data_set: &InMemDicomObject, tag: Tag) -> Vec<String> {
    strings(data_set, tag).unwrap_or_default()
}

fn integers(data_set: &InMemDicomObject, tag: Tag) -> Option<Vec<i64>> {
    data_set.element(tag).ok()?.to_multi_int::<i64>().ok()
}

fn multiplicity(data_set: &InMemDicomObject, tag: Tag) -> Option<u32> {
    match data_set.element(tag).ok()?.value() {
        Value::Primitive(primitive) => Some(primitive.multiplicity()),
        _ => None,
    }
}

/// Table 10-20 and 10.17.1.2: what the Selector Attribute and Selector
/// Sequence Pointer may name, and the counts of their companion Attributes.
fn selector(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    let attribute = tag_values(data_set, tags::SELECTOR_ATTRIBUTE);
    let pointers = tag_values(data_set, tags::SELECTOR_SEQUENCE_POINTER);
    for (what, tag) in attribute
        .iter()
        .map(|tag| ("Selector Attribute (0072,0026)", tag))
        .chain(
            pointers
                .iter()
                .map(|tag| ("Selector Sequence Pointer (0072,0052)", tag)),
        )
    {
        if is_private(*tag) && tag.element() > 0x00FF {
            found.push(format!(
                "{what} names Private Data Element {} where the Value shall be ({:04X},00{:02X}) ({CITATION} 10.17.1.2)",
                label(*tag),
                tag.group(),
                tag.element() & 0xFF
            ));
        }
    }
    if let (Some(tag), Some(number)) = (
        attribute.first(),
        integers(data_set, tags::SELECTOR_VALUE_NUMBER).and_then(|values| values.first().copied()),
    ) {
        if registered(*tag).is_some_and(|(_, min, max)| min == 1 && max == Some(1)) && number != 1 {
            found.push(format!(
                "Selector Value Number (0072,0028) is {number} for {}, whose Value Multiplicity is 1; it shall be 1 ({CITATION} Table 10-20)",
                label(*tag)
            ));
        }
    }
    for pointer in &pointers {
        let sequence = registered(*pointer).is_none_or(|(vrs, _, _)| vrs.contains(&VR::SQ));
        if !is_private(*pointer) && !sequence {
            found.push(format!(
                "Selector Sequence Pointer (0072,0052) names {}, which is not a Sequence; its Values are the Sequences nesting the selected Attribute ({CITATION} Table 10-20)",
                label(*pointer)
            ));
        }
    }
    if pointers.is_empty() {
        return;
    }
    for (tag, name) in [
        (
            tags::SELECTOR_SEQUENCE_POINTER_PRIVATE_CREATOR,
            "Selector Sequence Pointer Private Creator (0072,0054)",
        ),
        (
            tags::SELECTOR_SEQUENCE_POINTER_ITEMS,
            "Selector Sequence Pointer Items (0074,1057)",
        ),
    ] {
        if let Some(count) = multiplicity(data_set, tag) {
            if count as usize != pointers.len() {
                found.push(format!(
                    "{name} has {count} Values where Selector Sequence Pointer (0072,0052) has {} ({CITATION} Table 10-20)",
                    pointers.len()
                ));
            }
        }
    }
    let creators = values(data_set, tags::SELECTOR_SEQUENCE_POINTER_PRIVATE_CREATOR);
    for (pointer, creator) in pointers.iter().zip(&creators) {
        if !is_private(*pointer) && !creator.is_empty() {
            found.push(format!(
                "Selector Sequence Pointer Private Creator (0072,0054) is \"{creator}\" for public {}; it shall be empty ({CITATION} Table 10-20)",
                label(*pointer)
            ));
        }
    }
}

/// 8.6: the Context Identifier of a PS3.16 Context Group is its number.
fn context_identifier(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    let Some(identifier) = string(data_set, tags::CONTEXT_IDENTIFIER) else {
        return;
    };
    let dcmr = string(data_set, tags::MAPPING_RESOURCE).as_deref() == Some("DCMR");
    let digits = identifier.bytes().all(|byte| byte.is_ascii_digit());
    if dcmr && (!digits || identifier.starts_with('0')) {
        found.push(format!(
            "Context Identifier (0008,010F) \"{identifier}\" of a DCMR Context Group shall be its number in digits without leading zeros or \"CID\" ({CITATION} 8.6)"
        ));
    }
}

/// Tables 10-2, 10-3 and 10-3b: Frames are numbered from 1.
fn referenced_frames(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    if let Some(numbers) = integers(data_set, tags::REFERENCED_FRAME_NUMBER) {
        if let Some(number) = numbers.iter().find(|number| **number < 1) {
            found.push(format!(
                "Referenced Frame Number (0008,1160) {number} is below 1; the first Frame is Frame number 1 ({CITATION} Table 10-3)"
            ));
        }
    }
}

/// Tables C.7-3, C.7-5a and C.12-1: the Identification Sequence Items
/// correspond in number to the Values of the name they identify, which
/// within Contributing Equipment holds for a single Item too.
fn identification_items(data_set: &InMemDicomObject, within: Option<Tag>, found: &mut Vec<String>) {
    let contributing = within == Some(tags::CONTRIBUTING_EQUIPMENT_SEQUENCE);
    for (name, identification, citation) in IDENTIFIED_NAMES {
        let names = values(data_set, name);
        let items = sequence(data_set, identification).len();
        let (strict, citation) = if contributing && name == tags::OPERATORS_NAME {
            (true, "Table C.12-1")
        } else {
            (false, citation)
        };
        if names.is_empty() || items == 0 || (!strict && items == 1) || items == names.len() {
            continue;
        }
        found.push(format!(
            "{} has {items} Items for the {} Values of {}; their number and order shall correspond ({CITATION} {citation})",
            label(identification),
            names.len(),
            label(name)
        ));
    }
}

/// C.7.5.1.1.1: Date and Time of Last Calibration are pairs, oldest first.
fn calibration(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    let dates = values(data_set, tags::DATE_OF_LAST_CALIBRATION);
    let times = values(data_set, tags::TIME_OF_LAST_CALIBRATION);
    if dates.is_empty() {
        return;
    }
    if !times.is_empty() && times.len() != dates.len() {
        found.push(format!(
            "Date of Last Calibration (0018,1200) has {} Values and Time of Last Calibration (0018,1201) {}; when both are present they are pairs ({CITATION} C.7.5.1.1.1)",
            dates.len(),
            times.len()
        ));
        return;
    }
    let moments: Vec<(&str, &str)> = dates
        .iter()
        .enumerate()
        .map(|(index, date)| (date.as_str(), times.get(index).map_or("", String::as_str)))
        .collect();
    if moments.windows(2).any(|pair| pair[1] < pair[0]) {
        found.push(format!(
            "Date and Time of Last Calibration (0018,1200)/(0018,1201) are not ordered from the oldest to the most recent ({CITATION} C.7.5.1.1.1)"
        ));
    }
}

/// C.7.1.4.1.1.1: subject holder positions are ordinals counted from 1.
fn subject_position(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    let Some(position) = integers(data_set, tags::SUBJECT_RELATIVE_POSITION_IN_IMAGE) else {
        return;
    };
    if position.iter().any(|value| *value < 1) {
        found.push(format!(
            "Subject Relative Position in Image (0010,0028) {position:?} has a Value below 1; each starts at one for the first subject holder ({CITATION} C.7.1.4.1.1.1)"
        ));
    }
}

/// Table C.8.12.5-1: the Color Space names the space the ICC Profile defines.
fn color_space(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    let (Some(term), Ok(profile)) = (
        string(data_set, tags::COLOR_SPACE),
        data_set.element(tags::ICC_PROFILE),
    ) else {
        return;
    };
    let Ok(bytes) = profile.to_bytes() else {
        return;
    };
    if crate::icc::defines_color_space(&bytes, &term) == Some(false) {
        found.push(format!(
            "Color Space (0028,2002) is {term}, but the primaries of ICC Profile (0028,2000) are not those of {term} ({CITATION} Table C.8.12.5-1 and C.11.15.1.2)"
        ));
    }
}

/// C.7.6.22.1.3: Specimen Preparation Sequence Items are in ascending
/// chronological order, compared where two DateTimes of processing share an
/// offset, over the precision both have.
fn preparation_order(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    let processed: Vec<(usize, String)> = sequence(data_set, tags::SPECIMEN_PREPARATION_SEQUENCE)
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            sequence(step, tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE)
                .iter()
                .find(|content| concept_is(content, PROCESSING_DATETIME))
                .and_then(|content| string(content, tags::DATE_TIME))
                .map(|datetime| (index, datetime))
        })
        .collect();
    for pair in processed.windows(2) {
        let ((earlier_index, earlier), (later_index, later)) = (&pair[0], &pair[1]);
        let (earlier_moment, earlier_offset) = split_offset(earlier);
        let (later_moment, later_offset) = split_offset(later);
        let common = earlier_moment.len().min(later_moment.len());
        if earlier_offset == later_offset && later_moment[..common] < earlier_moment[..common] {
            found.push(format!(
                "Specimen Preparation Sequence (0040,0610) Item {} (processed {later}) precedes Item {} (processed {earlier}); Items shall be in ascending chronological order ({CITATION} C.7.6.22.1.3)",
                later_index + 1,
                earlier_index + 1
            ));
        }
    }
}

fn concept_is(content: &InMemDicomObject, concept: (&str, &str)) -> bool {
    sequence(content, tags::CONCEPT_NAME_CODE_SEQUENCE)
        .first()
        .is_some_and(|code| is_code(code, concept))
}

/// A DT Value without and with its UTC offset suffix.
fn split_offset(datetime: &str) -> (&str, &str) {
    match datetime.find(['+', '-']) {
        Some(index) => datetime.split_at(index),
        None => (datetime, ""),
    }
}

/// C.12.1.1.3.1.1: Data Elements Signed names elements of the Data Set holding
/// the MAC Parameters Sequence, in Data Set order, none of them excluded.
fn signed_elements(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    for (index, mac) in sequence(data_set, tags::MAC_PARAMETERS_SEQUENCE)
        .iter()
        .enumerate()
    {
        let signed = tag_values(mac, tags::DATA_ELEMENTS_SIGNED);
        let what = format!(
            "MAC Parameters Sequence (4FFE,0001)[{}] > Data Elements Signed (0400,0020)",
            index + 1
        );
        if signed.windows(2).any(|pair| pair[1] <= pair[0]) {
            found.push(format!(
                "{what} does not list its Tags in the order they appear in the Data Set ({CITATION} C.12.1.1.3.1.1)"
            ));
        }
        for tag in signed {
            let element = data_set.element(tag).ok();
            if let Some(reason) = excluded_from_signature(tag, element) {
                found.push(format!(
                    "{what} lists {}, but {reason} shall not be signed ({CITATION} C.12.1.1.3.1.1)",
                    label(tag)
                ));
            } else if element.is_none() {
                found.push(format!(
                    "{what} lists {}, which is not at the level of the MAC Parameters Sequence ({CITATION} C.12.1.1.3.1.1)",
                    label(tag)
                ));
            }
        }
    }
}

/// C.12.1.1.9.1 and C.12.1.1.9.2: a nonconforming Attribute is identified by a
/// Selector Attribute, and its Modified Attributes Item holds it empty.
fn original_attributes(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    for (index, original) in sequence(data_set, tags::ORIGINAL_ATTRIBUTES_SEQUENCE)
        .iter()
        .enumerate()
    {
        let modified = sequence(original, tags::MODIFIED_ATTRIBUTES_SEQUENCE).first();
        for (entry, nonconforming) in
            sequence(original, tags::NONCONFORMING_MODIFIED_ATTRIBUTES_SEQUENCE)
                .iter()
                .enumerate()
        {
            let what = format!(
                "Original Attributes Sequence (0400,0561)[{}] > Nonconforming Modified Attributes Sequence (0400,0551)[{}]",
                index + 1,
                entry + 1
            );
            let Some(tag) = tag_values(nonconforming, tags::SELECTOR_ATTRIBUTE)
                .first()
                .copied()
            else {
                found.push(format!(
                    "{what} has no Selector Attribute (0072,0026); it identifies a single Attribute ({CITATION} C.12.1.1.9.2)"
                ));
                continue;
            };
            if nonconforming
                .element(tags::SELECTOR_SEQUENCE_POINTER)
                .is_ok()
            {
                continue; // the enclosing Sequence holds its prior Value
            }
            let held_empty = modified
                .and_then(|item| item.element(tag).ok())
                .is_some_and(is_empty);
            if !held_empty {
                found.push(format!(
                    "{what} records nonconforming {}, which the Modified Attributes Sequence (0400,0550) shall hold with a zero length Value ({CITATION} C.12.1.1.9.1)",
                    label(tag)
                ));
            }
        }
    }
}

fn is_empty(element: &InMemElement) -> bool {
    match element.value() {
        Value::Primitive(primitive) => {
            primitive.multiplicity() == 0 || primitive.to_str().trim_matches([' ', '\0']).is_empty()
        }
        Value::Sequence(items) => items.items().is_empty(),
        Value::PixelSequence(_) => false,
    }
}

/// Table 10-2: one Numeric Value, matched by the floating point and rational
/// encodings, wherever a Content Item or Measured Value Item holds one.
fn measured_value(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    let Some(numeric) = multiplicity(data_set, tags::NUMERIC_VALUE) else {
        return;
    };
    if numeric > 1 {
        found.push(format!(
            "Numeric Value (0040,A30A) has {numeric} Values; only a single Value shall be present ({CITATION} Table 10-2)"
        ));
    }
    for (tag, name) in [
        (
            tags::FLOATING_POINT_VALUE,
            "Floating Point Value (0040,A161)",
        ),
        (
            tags::RATIONAL_NUMERATOR_VALUE,
            "Rational Numerator Value (0040,A162)",
        ),
        (
            tags::RATIONAL_DENOMINATOR_VALUE,
            "Rational Denominator Value (0040,A163)",
        ),
    ] {
        if let Some(count) = multiplicity(data_set, tag) {
            if count != numeric {
                found.push(format!(
                    "{name} has {count} Values where Numeric Value (0040,A30A) has {numeric} ({CITATION} Table 10-2)"
                ));
            }
        }
    }
}

/// Table 10-1: a Code Meaning written as a person name follows the PN rules.
fn person_code(data_set: &InMemDicomObject, found: &mut Vec<String>) {
    let Some(meaning) = string(data_set, tags::CODE_MEANING) else {
        return;
    };
    if !meaning.contains('^') {
        return; // a single name component is not a PN-form Code Meaning
    }
    if let Err(reason) = check_value_format(VR::PN, &meaning) {
        found.push(format!(
            "Code Meaning (0008,0104) \"{meaning}\" is encoded as a person name but is not a valid PN Value: {reason} ({CITATION} Table 10-1)"
        ));
    }
}

/// C.7.6.1.1.1: each Value is one to three anatomical direction abbreviations
/// of the Anatomical Orientation Type.
fn patient_orientation(root: &InMemDicomObject, violations: &mut Vec<String>) {
    let orientation = values(root, tags::PATIENT_ORIENTATION);
    let abbreviations: &[&str] = match string(root, tags::ANATOMICAL_ORIENTATION_TYPE).as_deref() {
        None | Some("BIPED") => &BIPED,
        Some("QUADRUPED") => &QUADRUPED,
        Some(_) => return,
    };
    for value in orientation.iter().filter(|value| !value.is_empty()) {
        if !matches!(fewest_abbreviations(value, abbreviations), Some(1..=3)) {
            violations.push(format!(
                "Patient Orientation (0020,0020) Value \"{value}\" is not one to three of the abbreviations {abbreviations:?} ({CITATION} C.7.6.1.1.1)"
            ));
        }
    }
}

/// The fewest abbreviations `value` reads as, left to right.
fn fewest_abbreviations(value: &str, abbreviations: &[&str]) -> Option<usize> {
    if value.is_empty() {
        return Some(0);
    }
    abbreviations
        .iter()
        .filter_map(|abbreviation| value.strip_prefix(abbreviation))
        .filter_map(|rest| fewest_abbreviations(rest, abbreviations))
        .map(|count| count + 1)
        .min()
}

/// Table C.7-9: Image Laterality agrees with Laterality and the laterality
/// modifiers of the Primary Anatomic Structures.
fn image_laterality(root: &InMemDicomObject, violations: &mut Vec<String>) {
    let Some(image) = string(root, tags::IMAGE_LATERALITY) else {
        return;
    };
    let series = string(root, tags::LATERALITY);
    let modifiers = sequence(root, tags::PRIMARY_ANATOMIC_STRUCTURE_SEQUENCE)
        .iter()
        .flat_map(|structure| {
            sequence(
                structure,
                tags::PRIMARY_ANATOMIC_STRUCTURE_MODIFIER_SEQUENCE,
            )
        })
        .filter_map(|code| {
            LATERALITY_CODES
                .iter()
                .find(|(scheme, value, _)| is_code(code, (scheme, value)))
                .map(|(_, _, laterality)| ("a Primary Anatomic Structure Modifier", *laterality))
        });
    for (source, laterality) in series
        .as_deref()
        .map(|laterality| ("Laterality (0020,0060)", laterality))
        .into_iter()
        .chain(modifiers)
    {
        if laterality != image {
            violations.push(format!(
                "Image Laterality (0020,0062) is {image} but {source} is {laterality}; it shall be consistent ({CITATION} Table C.7-9)"
            ));
        }
    }
}

/// C.2.2.1.1, C.12.1.1.6 and C.12.4.1.2: the SOP Classes that references to
/// patient photos, HL7 Structured Documents and source Instances may name.
fn references(root: &InMemDicomObject, violations: &mut Vec<String>) {
    for photo in sequence(root, tags::REFERENCED_PATIENT_PHOTO_SEQUENCE) {
        for class in referenced_sop_classes(photo) {
            if !PATIENT_PHOTO_CLASSES.contains(&class.as_str()) {
                violations.push(format!(
                    "Referenced Patient Photo Sequence (0010,1100) references SOP Class {class}; it shall be VL Photographic or Secondary Capture Image Storage ({CITATION} C.2.2.1.1)"
                ));
            }
        }
    }
    for document in sequence(root, tags::HL7_STRUCTURED_DOCUMENT_REFERENCE_SEQUENCE) {
        if let Some(class) = string(document, tags::REFERENCED_SOP_CLASS_UID) {
            if !class.starts_with(HL7_ROOT) {
                violations.push(format!(
                    "HL7 Structured Document Reference Sequence (0040,A390) Referenced SOP Class UID {class} is not the HL7 Object Identifier of a Hierarchical Message Description ({CITATION} C.12.1.1.6)"
                ));
            }
        }
    }
    for source in sequence(root, tags::SOURCE_INSTANCE_SEQUENCE) {
        if let Some(class) = string(source, tags::REFERENCED_SOP_CLASS_UID) {
            if is_image_storage_sop_class(&class) {
                violations.push(format!(
                    "Source Instance Sequence (0042,0013) references Image Storage SOP Class {class}; images are referenced by Source Image Sequence (0008,2112) ({CITATION} C.12.4.1.2)"
                ));
            }
        }
    }
}

/// Referenced SOP Class UIDs of the Referenced SOP Sequence Items nested in
/// a Hierarchical SOP Instance Reference.
fn referenced_sop_classes(item: &InMemDicomObject) -> Vec<String> {
    let mut classes: Vec<String> = sequence(item, tags::REFERENCED_SOP_SEQUENCE)
        .iter()
        .filter_map(|reference| string(reference, tags::REFERENCED_SOP_CLASS_UID))
        .collect();
    for series in sequence(item, tags::REFERENCED_SERIES_SEQUENCE) {
        classes.extend(referenced_sop_classes(series));
    }
    classes
}
