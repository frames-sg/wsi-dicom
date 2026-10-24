//! The document-level requirements of PS3.3 C.17 that relate a document's
//! Attributes to each other and to its Content Tree: Completion and
//! Verification (Table C.17-2), Verifying Observer and Attestor (C.17.2.5),
//! the evidence Sequences against the Instances the tree references (C.17.2.3;
//! for a KOS, Table C.17.6-2 and TID 2010), the Studies a KOS references
//! (C.17.6.2.1), Identical and Predecessor Documents (C.17.2.2, Table C.17-2),
//! and the Display URI and MAC Calculation Transfer Syntax UID of each
//! Hierarchical SOP Instance Reference (Tables C.17-3 and C.17-3a).

use std::collections::BTreeMap;

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::tree::Tree;
use super::{Findings, CITATION, KEY_OBJECT_SELECTION, PS3_16};
use crate::validation::iod::structured_report::DocumentConstraints;
use crate::validation::iod::{items, string_value};
use crate::validation::transfer_syntaxes::smpte_st2110;

/// Display URI (0040,E012), which the standard dictionary of dicom-rs 0.9
/// predates.
const DISPLAY_URI: Tag = Tag(0x0040, 0xE012);
/// The SR Document Storage SOP Classes a Predecessor Document may be
/// (PS3.4 Table B.5-1).
const SR_SOP_CLASS_ROOT: &str = "1.2.840.10008.5.1.4.1.1.88.";
/// TID 2010 Row 1 Concept Name requiring every referenced Instance in the
/// KOS's Study.
const MANIFEST_WITH_DESCRIPTION: (&str, &str) = ("DCM", "131560");
/// The Concept Name of the TID 1600 Image Library CONTAINER.
const IMAGE_LIBRARY: (&str, &str) = ("DCM", "111028");

pub(super) fn check(
    root: &InMemDicomObject,
    tree: &Tree<'_>,
    constraints: &DocumentConstraints,
    findings: &mut Findings,
) {
    let referenced = References::of(tree, 0..tree.nodes.len());
    let current = Evidence::of(root, tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE);
    if constraints.iod == KEY_OBJECT_SELECTION {
        key_object_evidence(tree, &referenced, &current, findings);
        identical_studies(root, &current, findings);
        manifest_study(root, &current, findings);
    } else {
        verification(root, findings);
        attestors(root, findings);
        let other = Evidence::of(root, tags::PERTINENT_OTHER_EVIDENCE_SEQUENCE);
        report_evidence(&referenced, &current, &other, findings);
        predecessors(root, findings);
    }
    classes(&referenced, &current, findings);
    own_duplicate(root, findings);
    hierarchical_references(root, findings);
}

/// Table C.17-2: "A Value of \"VERIFIED\" shall be used only when the Value
/// of Completion Flag (0040,A491) is \"COMPLETE\"."
fn verification(root: &InMemDicomObject, findings: &mut Findings) {
    let verification = string_value(root, tags::VERIFICATION_FLAG, 1);
    let completion = string_value(root, tags::COMPLETION_FLAG, 1);
    if verification.as_deref() == Some("VERIFIED") && completion.as_deref() != Some("COMPLETE") {
        findings.push(|| {
            format!(
                "Verification Flag (0040,A493) is VERIFIED but Completion Flag (0040,A491) is {}; a Value of \"VERIFIED\" shall be used only when the Value of Completion Flag is \"COMPLETE\" ({CITATION} Table C.17-2)",
                completion.as_deref().unwrap_or("absent")
            )
        });
    }
}

/// A person as a Verifying Observer or Participant identifies them: by name,
/// and by the coded identification each Sequence may carry.
#[derive(PartialEq)]
struct Person {
    name: Option<String>,
    code: Option<(String, String)>,
}

impl Person {
    fn of(item: &InMemDicomObject, name: Tag, codes: Tag) -> Self {
        let code = items(item, codes).first().and_then(|code| {
            Some((
                string_value(code, tags::CODING_SCHEME_DESIGNATOR, 1)?,
                string_value(code, tags::CODE_VALUE, 1)
                    .or_else(|| string_value(code, tags::LONG_CODE_VALUE, 1))?,
            ))
        });
        Self {
            name: string_value(item, name, 1).map(|name| name.trim_end_matches('^').to_string()),
            code,
        }
    }

    fn same(&self, other: &Self) -> bool {
        let named = self.name.is_some() && self.name == other.name;
        let coded = self.code.is_some() && self.code == other.code;
        named || coded
    }
}

/// C.17.2.5: "An individual shall not be identified in both the Verifying
/// Observer Sequence (as the legal authenticator) and in the Participant
/// Sequence as an Attestor."
fn attestors(root: &InMemDicomObject, findings: &mut Findings) {
    let verifiers: Vec<Person> = items(root, tags::VERIFYING_OBSERVER_SEQUENCE)
        .iter()
        .map(|item| {
            Person::of(
                item,
                tags::VERIFYING_OBSERVER_NAME,
                tags::VERIFYING_OBSERVER_IDENTIFICATION_CODE_SEQUENCE,
            )
        })
        .collect();
    for (position, participant) in items(root, tags::PARTICIPANT_SEQUENCE).iter().enumerate() {
        if string_value(participant, tags::PARTICIPATION_TYPE, 1).as_deref() != Some("ATTEST") {
            continue;
        }
        let attestor = Person::of(
            participant,
            tags::PERSON_NAME,
            tags::PERSON_IDENTIFICATION_CODE_SEQUENCE,
        );
        if verifiers.iter().any(|verifier| verifier.same(&attestor)) {
            findings.push(|| {
                format!(
                    "Participant Sequence (0040,A07A)[{}], an Attestor, identifies {} who is also in Verifying Observer Sequence (0040,A073); an individual shall not be identified in both the Verifying Observer Sequence and in the Participant Sequence as an Attestor ({CITATION} C.17.2.5)",
                    position + 1,
                    attestor.name.as_deref().map_or("the individual".to_string(), |name| format!("\"{name}\""))
                )
            });
        }
    }
}

/// A Composite SOP Instance the Content Tree references.
struct Reference {
    class: Option<String>,
    node: usize,
}

/// The Instances Content Items reference, by SOP Instance UID: the IMAGE,
/// WAVEFORM and COMPOSITE Items' Instances, and the presentation states and
/// Real World Value Mappings an IMAGE Item references with its image
/// (C.17.2.3: "not only the images and waveforms ..., but also all
/// presentation states, Real World Value maps and other accompanying
/// Composite Instances").
struct References(BTreeMap<String, Reference>);

impl References {
    fn of(tree: &Tree<'_>, nodes: impl Iterator<Item = usize>) -> Self {
        let mut found = BTreeMap::new();
        for index in nodes {
            let node = &tree.nodes[index];
            if node.is_by_reference()
                || !matches!(node.value_type(), Some("IMAGE" | "WAVEFORM" | "COMPOSITE"))
            {
                continue;
            }
            for reference in items(node.object, tags::REFERENCED_SOP_SEQUENCE) {
                let accompanying = [
                    tags::REFERENCED_SOP_SEQUENCE,
                    tags::REFERENCED_REAL_WORLD_VALUE_MAPPING_INSTANCE_SEQUENCE,
                ]
                .into_iter()
                .flat_map(|tag| items(reference, tag));
                for instance in std::iter::once(reference).chain(accompanying) {
                    if let Some(uid) = string_value(instance, tags::REFERENCED_SOP_INSTANCE_UID, 1)
                    {
                        found.entry(uid).or_insert_with(|| Reference {
                            class: string_value(instance, tags::REFERENCED_SOP_CLASS_UID, 1),
                            node: index,
                        });
                    }
                }
            }
        }
        Self(found)
    }

    fn contains(&self, uid: &str) -> bool {
        self.0.contains_key(uid)
    }
}

/// An Instance an evidence Sequence lists, with its Study.
struct Listed {
    class: Option<String>,
    study: Option<String>,
    item: usize,
}

/// A Hierarchical SOP Instance Reference Macro Sequence (Table C.17-3) by
/// SOP Instance UID, and the Studies its Items name.
struct Evidence {
    name: &'static str,
    instances: BTreeMap<String, Listed>,
    studies: Vec<String>,
}

impl Evidence {
    fn of(root: &InMemDicomObject, tag: Tag) -> Self {
        let name = if tag == tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE {
            "Current Requested Procedure Evidence Sequence (0040,A375)"
        } else {
            "Pertinent Other Evidence Sequence (0040,A385)"
        };
        let mut instances = BTreeMap::new();
        let mut studies = Vec::new();
        for (position, study) in items(root, tag).iter().enumerate() {
            let study_uid = string_value(study, tags::STUDY_INSTANCE_UID, 1);
            studies.extend(study_uid.clone());
            for series in items(study, tags::REFERENCED_SERIES_SEQUENCE) {
                for sop in items(series, tags::REFERENCED_SOP_SEQUENCE) {
                    if let Some(uid) = string_value(sop, tags::REFERENCED_SOP_INSTANCE_UID, 1) {
                        instances.entry(uid).or_insert_with(|| Listed {
                            class: string_value(sop, tags::REFERENCED_SOP_CLASS_UID, 1),
                            study: study_uid.clone(),
                            item: position + 1,
                        });
                    }
                }
            }
        }
        studies.sort();
        studies.dedup();
        Self {
            name,
            instances,
            studies,
        }
    }
}

/// C.17.2.3: every Instance the Content Tree references is current evidence,
/// which the Current Requested Procedure Evidence Sequence includes, or other
/// pertinent evidence, which the Pertinent Other Evidence Sequence lists; and
/// "the same SOP Instance shall not be referenced in both of these
/// Sequences". Which of the two an Instance belongs in depends on the
/// Requested Procedure it was acquired for, which is not encoded.
fn report_evidence(
    referenced: &References,
    current: &Evidence,
    other: &Evidence,
    findings: &mut Findings,
) {
    for (uid, _) in referenced.0.iter().filter(|(uid, _)| {
        !current.instances.contains_key(*uid) && !other.instances.contains_key(*uid)
    }) {
        findings.push(|| {
            format!(
                "the Content Tree references SOP Instance {uid}, which neither {} nor {} lists; the Current Requested Procedure Evidence Sequence shall include all current evidence referenced in the Content Tree, and the Pertinent Other Evidence Sequence references the other evidence ({CITATION} C.17.2.3)",
                current.name, other.name
            )
        });
    }
    for uid in current
        .instances
        .keys()
        .filter(|uid| other.instances.contains_key(*uid))
    {
        findings.push(|| {
            format!(
                "SOP Instance {uid} is in both {} and {}; the same SOP Instance shall not be referenced in both of these Sequences ({CITATION} C.17.2.3)",
                current.name, other.name
            )
        });
    }
}

/// Table C.17.6-2: the Current Requested Procedure Evidence Sequence lists
/// "all Composite SOP Instances referenced in the Content Sequence. No other
/// instances shall be included"; TID 2010: the Image Library "shall not
/// include entries for instances not referenced in Rows 8, 9 and 10".
fn key_object_evidence(
    tree: &Tree<'_>,
    referenced: &References,
    current: &Evidence,
    findings: &mut Findings,
) {
    for (uid, reference) in referenced
        .0
        .iter()
        .filter(|(uid, _)| !current.instances.contains_key(*uid))
    {
        let at = tree.at(reference.node);
        findings.push(|| {
            format!(
                "{at} references SOP Instance {uid}, which {} does not list; it is the list of all Composite SOP Instances referenced in the Content Sequence ({CITATION} Table C.17.6-2; {PS3_16} TID 2010)",
                current.name
            )
        });
    }
    for (uid, listed) in current
        .instances
        .iter()
        .filter(|(uid, _)| !referenced.contains(uid))
    {
        findings.push(|| {
            format!(
                "{}[{}] lists SOP Instance {uid}, which no Content Item references; no other instances than those referenced in the Content Sequence shall be included in this Sequence ({CITATION} Table C.17.6-2)",
                current.name, listed.item
            )
        });
    }
    let root_items: Vec<usize> = tree.children(0).collect();
    let selected = References::of(tree, root_items.iter().copied());
    for library in root_items.iter().copied().filter(|index| {
        let node = &tree.nodes[*index];
        node.value_type() == Some("CONTAINER") && concept_is(node.object, IMAGE_LIBRARY)
    }) {
        let entries = References::of(tree, descendants(tree, library).into_iter());
        for (uid, entry) in entries.0.iter().filter(|(uid, _)| !selected.contains(uid)) {
            let at = tree.at(entry.node);
            findings.push(|| {
                format!(
                    "{at}, an Image Library entry, references SOP Instance {uid}, which no IMAGE, WAVEFORM or COMPOSITE Content Item of the root references; the Image Library shall not include entries for instances not referenced in Rows 8, 9 and 10 ({PS3_16} TID 2010)"
                )
            });
        }
    }
}

/// Every Content Item below `index`, by value.
fn descendants(tree: &Tree<'_>, index: usize) -> Vec<usize> {
    let mut found = Vec::new();
    let mut pending: Vec<usize> = tree.children(index).collect();
    while let Some(next) = pending.pop() {
        found.push(next);
        pending.extend(tree.children(next));
    }
    found
}

fn concept_is(object: &InMemDicomObject, (scheme, value): (&str, &str)) -> bool {
    items(object, tags::CONCEPT_NAME_CODE_SEQUENCE)
        .first()
        .is_some_and(|code| {
            string_value(code, tags::CODING_SCHEME_DESIGNATOR, 1).as_deref() == Some(scheme)
                && string_value(code, tags::CODE_VALUE, 1).as_deref() == Some(value)
        })
}

/// An Instance the tree references and an evidence Sequence lists is the
/// same Instance: the same Referenced SOP Class UID (Table 10-11).
fn classes(referenced: &References, current: &Evidence, findings: &mut Findings) {
    for (uid, reference) in &referenced.0 {
        let Some(listed) = current.instances.get(uid) else {
            continue;
        };
        if let (Some(class), Some(listed_class)) = (&reference.class, &listed.class) {
            if class != listed_class {
                findings.push(|| {
                    format!(
                        "the Content Tree references SOP Instance {uid} with Referenced SOP Class UID {class}, but {}[{}] lists it with {listed_class}; Referenced SOP Class UID identifies the referenced SOP Class ({CITATION} Table 10-11)",
                        current.name, listed.item
                    )
                });
            }
        }
    }
}

/// The document's own Study Instance UID.
fn own_study(root: &InMemDicomObject) -> Option<String> {
    string_value(root, tags::STUDY_INSTANCE_UID, 1)
}

/// C.17.6.2.1: "If the Current Requested Procedure Evidence Sequence
/// (0040,A375) references SOP Instances both in the current Study and in one
/// or more other Studies, this document shall be duplicated into each of
/// those other Studies, and the duplicates shall be referenced in the
/// Identical Documents Sequence (0040,A525)."
fn identical_studies(root: &InMemDicomObject, current: &Evidence, findings: &mut Findings) {
    let Some(own) = own_study(root) else {
        return;
    };
    if !current.studies.contains(&own) {
        return;
    }
    let duplicated: Vec<String> = items(root, tags::IDENTICAL_DOCUMENTS_SEQUENCE)
        .iter()
        .filter_map(|study| string_value(study, tags::STUDY_INSTANCE_UID, 1))
        .collect();
    let missing: Vec<&String> = current
        .studies
        .iter()
        .filter(|study| **study != own && !duplicated.contains(study))
        .collect();
    if !missing.is_empty() {
        findings.push(|| {
            format!(
                "{} references SOP Instances in the document's Study and in Studies {missing:?}, for which Identical Documents Sequence (0040,A525) references no duplicate; this document shall be duplicated into each of those other Studies, and the duplicates shall be referenced in the Identical Documents Sequence ({CITATION} C.17.6.2.1)",
                current.name
            )
        });
    }
}

/// TID 2010: "When the Row 1 Concept Name is (131560, DCM, \"Manifest with
/// Description\") then all referenced instances shall be in the same Study
/// as the KOS."
fn manifest_study(root: &InMemDicomObject, current: &Evidence, findings: &mut Findings) {
    if !concept_is(root, MANIFEST_WITH_DESCRIPTION) {
        return;
    }
    let Some(own) = own_study(root) else {
        return;
    };
    for (uid, listed) in current
        .instances
        .iter()
        .filter(|(_, listed)| listed.study.as_ref().is_some_and(|study| *study != own))
    {
        findings.push(|| {
            format!(
                "the Manifest with Description references SOP Instance {uid} in Study {}, not the KOS's Study {own}; when the Row 1 Concept Name is (131560, DCM, \"Manifest with Description\") then all referenced instances shall be in the same Study as the KOS ({PS3_16} TID 2010)",
                listed.study.as_deref().unwrap_or_default()
            )
        });
    }
}

/// The SOP Instances a Hierarchical SOP Instance Reference Macro Sequence
/// references, with the Referenced SOP Class UID of each.
fn hierarchical_instances(root: &InMemDicomObject, tag: Tag) -> Vec<(String, Option<String>)> {
    items(root, tag)
        .iter()
        .flat_map(|study| items(study, tags::REFERENCED_SERIES_SEQUENCE))
        .flat_map(|series| items(series, tags::REFERENCED_SOP_SEQUENCE))
        .filter_map(|sop| {
            Some((
                string_value(sop, tags::REFERENCED_SOP_INSTANCE_UID, 1)?,
                string_value(sop, tags::REFERENCED_SOP_CLASS_UID, 1),
            ))
        })
        .collect()
}

/// Table C.17-2: Predecessor Documents "Shall refer to SR SOP Instances ...
/// whose content has been wholly or partially included in this document",
/// which is another document than this one.
fn predecessors(root: &InMemDicomObject, findings: &mut Findings) {
    let own = string_value(root, tags::SOP_INSTANCE_UID, 1);
    for (uid, class) in hierarchical_instances(root, tags::PREDECESSOR_DOCUMENTS_SEQUENCE) {
        if own.as_deref() == Some(uid.as_str()) {
            findings.push(|| {
                format!(
                    "Predecessor Documents Sequence (0040,A360) references this document itself, SOP Instance {uid}; it references the SR SOP Instances whose content this document includes ({CITATION} Table C.17-2)"
                )
            });
        }
        if let Some(class) = class.filter(|class| !class.starts_with(SR_SOP_CLASS_ROOT)) {
            findings.push(|| {
                format!(
                    "Predecessor Documents Sequence (0040,A360) references SOP Instance {uid} of SOP Class {class}, which is not an SR SOP Class; it shall refer to SR SOP Instances ({CITATION} Table C.17-2)"
                )
            });
        }
    }
}

/// C.17.2.2: the Identical Documents Sequence references "all other
/// duplicate SOP Instances", "stored with different SOP Instance UIDs".
fn own_duplicate(root: &InMemDicomObject, findings: &mut Findings) {
    let Some(own) = string_value(root, tags::SOP_INSTANCE_UID, 1) else {
        return;
    };
    if hierarchical_instances(root, tags::IDENTICAL_DOCUMENTS_SEQUENCE)
        .iter()
        .any(|(uid, _)| *uid == own)
    {
        findings.push(|| {
            format!(
                "Identical Documents Sequence (0040,A525) references this document itself, SOP Instance {own}; it shall contain references to all other duplicate SOP Instances, which are stored with different SOP Instance UIDs ({CITATION} C.17.2.2)"
            )
        });
    }
}

/// The Hierarchical SOP Instance Reference Macro Sequences of the document
/// IODs (Tables C.17-2 and C.17.6-2).
const HIERARCHICAL_SEQUENCES: [(Tag, &str); 4] = [
    (
        tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
        "Current Requested Procedure Evidence Sequence (0040,A375)",
    ),
    (
        tags::PERTINENT_OTHER_EVIDENCE_SEQUENCE,
        "Pertinent Other Evidence Sequence (0040,A385)",
    ),
    (
        tags::IDENTICAL_DOCUMENTS_SEQUENCE,
        "Identical Documents Sequence (0040,A525)",
    ),
    (
        tags::PREDECESSOR_DOCUMENTS_SEQUENCE,
        "Predecessor Documents Sequence (0040,A360)",
    ),
];

/// Table C.17-3: Display URI "Shall include the fully specified scheme,
/// authority, path, and query components in accordance with RFC 3986";
/// Table C.17-3a: "When computing the MAC, only Transfer Syntaxes that
/// explicitly include the VR and use Little Endian encoding shall be used."
fn hierarchical_references(root: &InMemDicomObject, findings: &mut Findings) {
    for (tag, name) in HIERARCHICAL_SEQUENCES {
        for (position, study) in items(root, tag).iter().enumerate() {
            let place = || format!("{name}[{}]", position + 1);
            if let Some(uri) = string_value(study, DISPLAY_URI, 1) {
                if let Some(departure) = uri_departure(&uri) {
                    findings.push(|| {
                        format!(
                            "{} > Display URI (0040,E012) \"{uri}\" {departure}; it shall include the fully specified scheme, authority, path, and query components in accordance with RFC 3986 ({CITATION} Table C.17-3)",
                            place()
                        )
                    });
                }
            }
            for series in items(study, tags::REFERENCED_SERIES_SEQUENCE) {
                for sop in items(series, tags::REFERENCED_SOP_SEQUENCE) {
                    for mac in items(sop, tags::REFERENCED_SOP_INSTANCE_MAC_SEQUENCE) {
                        mac_transfer_syntax(mac, &place, findings);
                    }
                }
            }
        }
    }
}

fn mac_transfer_syntax(
    mac: &InMemDicomObject,
    place: &dyn Fn() -> String,
    findings: &mut Findings,
) {
    let Some(uid) = string_value(mac, tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID, 1) else {
        return;
    };
    let unfit = match uid.as_str() {
        "1.2.840.10008.1.2" => {
            Some("Implicit VR Little Endian, which does not explicitly include the VR")
        }
        "1.2.840.10008.1.20" => {
            Some("Papyrus 3 Implicit VR Little Endian, which does not explicitly include the VR")
        }
        "1.2.840.10008.1.2.2" => {
            Some("Explicit VR Big Endian, which does not use Little Endian encoding")
        }
        uid if smpte_st2110(uid).is_some() => {
            Some("an SMPTE ST 2110 Transfer Syntax, which defines no Data Set encoding")
        }
        uid if uid.starts_with("1.2.840.10008.1.2.") => None,
        _ => {
            findings.unexamined(format!(
                "whether MAC Calculation Transfer Syntax UID {uid} of {} explicitly includes the VR and uses Little Endian encoding: it is not a Transfer Syntax of PS3.6",
                place()
            ));
            None
        }
    };
    if let Some(unfit) = unfit {
        findings.push(|| {
            format!(
                "{} > Referenced SOP Instance MAC Sequence > MAC Calculation Transfer Syntax UID (0400,0010) is {uid}, {unfit}; when computing the MAC, only Transfer Syntaxes that explicitly include the VR and use Little Endian encoding shall be used ({CITATION} Table C.17-3a)",
                place()
            )
        });
    }
}

/// Why `uri` is not an RFC 3986 URI with a scheme, an authority, a path and a
/// query, or `None` when it is one.
fn uri_departure(uri: &str) -> Option<&'static str> {
    let Some((scheme, rest)) = uri.split_once(':') else {
        return Some("has no scheme");
    };
    let mut characters = scheme.chars();
    let scheme_valid = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters
            .all(|character| character.is_ascii_alphanumeric() || "+-.".contains(character));
    if !scheme_valid {
        return Some("has no valid scheme");
    }
    let Some(hierarchy) = rest.strip_prefix("//") else {
        return Some("has no authority");
    };
    let authority_end = hierarchy.find(['/', '?', '#']).unwrap_or(hierarchy.len());
    if authority_end == 0 {
        return Some("has an empty authority");
    }
    let after_authority = &hierarchy[authority_end..];
    let path_end = after_authority
        .find(['?', '#'])
        .unwrap_or(after_authority.len());
    if path_end == 0 {
        return Some("has no path");
    }
    let query = after_authority[path_end..]
        .strip_prefix('?')
        .map(|query| query.split('#').next().unwrap_or_default());
    if query.is_none_or(str::is_empty) {
        return Some("has no query");
    }
    if !uri_characters(uri) {
        return Some("holds a character RFC 3986 does not allow unencoded");
    }
    None
}

/// Whether every character is unreserved, reserved, or part of a
/// percent-encoding (RFC 3986 2.1 to 2.3).
fn uri_characters(uri: &str) -> bool {
    let bytes = uri.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            let encoded = bytes
                .get(index + 1..index + 3)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit));
            if !encoded {
                return false;
            }
            index += 3;
            continue;
        }
        if !(byte.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=".contains(&byte)) {
            return false;
        }
        index += 1;
    }
    true
}
