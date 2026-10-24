//! Relations between the objects of a validated set that hold in every profile
//! (PS3.3 C.7.1.4.1.1, C.7.4.1, C.7.6.1.1.5, C.7.6.16.1, C.7.6.16.2.2.4,
//! C.7.6.16.2.5.1, C.7.6.17.2, C.7.6.22.1.2 and C.12.3; PS3.5 9).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};
use sha2::{Digest, Sha256};

use super::super::ValidationCheck;
use super::rule_check;
use super::wsi_image::{functional_group_items, integer, is_code, sequence, string, tag_values};
use crate::validation::iod::{dicom_edition, EDITION};

pub(super) const SERIES_SET_RULE: &str = "intrinsic-wsi-dicom-series-set";
/// Localizer, the Purpose of Reference of a planning image (C.7.6.16.2.5.1).
const LOCALIZER: (&str, &str) = ("DCM", "121311");
/// Patient Module Attributes (C.7.1.1) of a group of organisms imaged
/// together that each member imaged alone also describes.
const GROUP_SHARED: [(dicom_core::Tag, &str); 6] = [
    (tags::PATIENT_BIRTH_DATE, "Patient's Birth Date (0010,0030)"),
    (tags::PATIENT_BIRTH_TIME, "Patient's Birth Time (0010,0032)"),
    (tags::PATIENT_SEX, "Patient's Sex (0010,0040)"),
    (tags::STRAIN_DESCRIPTION, "Strain Description (0010,0212)"),
    (
        tags::PATIENT_SPECIES_DESCRIPTION,
        "Patient Species Description (0010,2201)",
    ),
    (
        tags::PATIENT_BREED_DESCRIPTION,
        "Patient Breed Description (0010,2292)",
    ),
];
/// Attributes that may differ between the SOP Instances of a Concatenation
/// (C.7.6.16.2.2.4), with those compared separately and the Pixel Data.
const CONCATENATION_VARIANT: [dicom_core::Tag; 11] = [
    tags::NUMBER_OF_FRAMES,
    tags::CONCATENATION_FRAME_OFFSET_NUMBER,
    tags::IN_CONCATENATION_NUMBER,
    tags::SOP_INSTANCE_UID,
    tags::INSTANCE_CREATION_DATE,
    tags::INSTANCE_CREATION_TIME,
    tags::EXTENDED_OFFSET_TABLE,
    tags::EXTENDED_OFFSET_TABLE_LENGTHS,
    tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
    tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
    tags::PIXEL_DATA,
];

/// What one object contributes to the set relations.
#[derive(Debug)]
pub(super) struct SeriesFacts {
    path: PathBuf,
    study: Option<String>,
    series: Option<String>,
    sop_instance: Option<String>,
    patient_id: Option<String>,
    /// The present GROUP_SHARED Values, by their position in that list.
    patient: Vec<(usize, String)>,
    /// Lossy Image Compression (0028,2110).
    lossy: Option<String>,
    frame_of_reference: Option<String>,
    /// Container Identifier of the imaged slide.
    container: Option<String>,
    instance_number: Option<i64>,
    number_of_frames: Option<i64>,
    /// Identifying UIDs by the role they play.
    uids: Vec<(&'static str, String)>,
    concatenation: Option<Concatenation>,
    /// Dimension Organization UID and the (pointer, group pointer) it indexes.
    dimensions: Vec<(String, String)>,
    /// SOP Instances this object references as localizers.
    localizers: Vec<String>,
    /// (Specimen Identifier, issuer) and Specimen UID.
    specimens: Vec<((String, String), String)>,
    /// Multi-frame Source SOP Instance UID and digest of each Frame
    /// Extraction Sequence Item.
    extractions: Vec<(Option<String>, String)>,
    arrangement: Option<Arrangement>,
}

/// A Group of Patients imaged together: the group's Patient ID, its members'
/// Patient IDs and where each member is in the image.
#[derive(Debug)]
struct Arrangement {
    group: String,
    members: BTreeSet<String>,
    positions: BTreeSet<(String, Vec<i64>)>,
}

#[derive(Debug)]
struct Concatenation {
    uid: String,
    number: Option<i64>,
    total: Option<i64>,
    frame_offset: Option<i64>,
    source: Option<String>,
    dimension_index: String,
    shared_groups: String,
    per_frame_structure: String,
    others: String,
}

impl SeriesFacts {
    pub(super) fn of(object: &DefaultDicomObject, path: &Path) -> Self {
        let root: &InMemDicomObject = object;
        let mut uids = Vec::new();
        for (role, tag) in [
            ("Study Instance UID", tags::STUDY_INSTANCE_UID),
            ("Series Instance UID", tags::SERIES_INSTANCE_UID),
            ("SOP Instance UID", tags::SOP_INSTANCE_UID),
            ("Frame of Reference UID", tags::FRAME_OF_REFERENCE_UID),
            ("Pyramid UID", tags::PYRAMID_UID),
            ("Concatenation UID", tags::CONCATENATION_UID),
        ] {
            if let Some(uid) = string(root, tag) {
                uids.push((role, uid));
            }
        }
        for item in sequence(root, tags::DIMENSION_ORGANIZATION_SEQUENCE) {
            if let Some(uid) = string(item, tags::DIMENSION_ORGANIZATION_UID) {
                uids.push(("Dimension Organization UID", uid));
            }
        }
        let mut specimens = Vec::new();
        for item in sequence(root, tags::SPECIMEN_DESCRIPTION_SEQUENCE) {
            let Some(uid) = string(item, tags::SPECIMEN_UID) else {
                continue;
            };
            uids.push(("Specimen UID", uid.clone()));
            if let Some(identifier) = string(item, tags::SPECIMEN_IDENTIFIER) {
                let issuer = sequence(item, tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE)
                    .first()
                    .map(|issuer| digest(issuer, &[]))
                    .unwrap_or_default();
                specimens.push(((identifier, issuer), uid));
            }
        }
        let dimensions = sequence(root, tags::DIMENSION_INDEX_SEQUENCE)
            .iter()
            .filter_map(|item| {
                let uid = string(item, tags::DIMENSION_ORGANIZATION_UID)?;
                let pointers = format!(
                    "{:?}{:?}",
                    tag_values(item, tags::DIMENSION_INDEX_POINTER),
                    tag_values(item, tags::FUNCTIONAL_GROUP_POINTER)
                );
                Some((uid, pointers))
            })
            .collect();
        let localizers = functional_group_items(root)
            .into_iter()
            .flat_map(|(_, item)| sequence(item, tags::REFERENCED_IMAGE_SEQUENCE))
            .filter(|reference| {
                sequence(reference, tags::PURPOSE_OF_REFERENCE_CODE_SEQUENCE)
                    .iter()
                    .any(|code| is_code(code, LOCALIZER))
            })
            .filter_map(|reference| string(reference, tags::REFERENCED_SOP_INSTANCE_UID))
            .collect();
        let extractions = sequence(root, tags::FRAME_EXTRACTION_SEQUENCE)
            .iter()
            .map(|item| {
                (
                    string(item, tags::MULTI_FRAME_SOURCE_SOP_INSTANCE_UID),
                    digest(item, &[]),
                )
            })
            .collect();
        Self {
            path: path.to_path_buf(),
            study: string(root, tags::STUDY_INSTANCE_UID),
            series: string(root, tags::SERIES_INSTANCE_UID),
            sop_instance: string(root, tags::SOP_INSTANCE_UID),
            patient_id: string(root, tags::PATIENT_ID),
            patient: GROUP_SHARED
                .iter()
                .enumerate()
                .filter_map(|(index, (tag, _))| Some((index, string(root, *tag)?)))
                .collect(),
            lossy: string(root, tags::LOSSY_IMAGE_COMPRESSION),
            frame_of_reference: string(root, tags::FRAME_OF_REFERENCE_UID),
            container: string(root, tags::CONTAINER_IDENTIFIER),
            instance_number: integer(root, tags::INSTANCE_NUMBER),
            number_of_frames: integer(root, tags::NUMBER_OF_FRAMES),
            uids,
            concatenation: concatenation(root),
            dimensions,
            localizers,
            specimens,
            extractions,
            arrangement: arrangement(root),
        }
    }
}

fn arrangement(root: &InMemDicomObject) -> Option<Arrangement> {
    let members = sequence(root, tags::GROUP_OF_PATIENTS_IDENTIFICATION_SEQUENCE);
    if members.is_empty() {
        return None;
    }
    let positions: BTreeSet<(String, Vec<i64>)> = members
        .iter()
        .filter_map(|member| {
            let position = member
                .element(tags::SUBJECT_RELATIVE_POSITION_IN_IMAGE)
                .ok()?
                .to_multi_int::<i64>()
                .ok()?;
            Some((string(member, tags::PATIENT_ID)?, position))
        })
        .collect();
    Some(Arrangement {
        group: string(root, tags::PATIENT_ID)?,
        members: members
            .iter()
            .filter_map(|member| string(member, tags::PATIENT_ID))
            .collect(),
        positions,
    })
}

fn concatenation(root: &InMemDicomObject) -> Option<Concatenation> {
    let uid = string(root, tags::CONCATENATION_UID)?;
    let per_frame: BTreeSet<String> = sequence(root, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .iter()
        .flat_map(|item| structure(item, ""))
        .collect();
    Some(Concatenation {
        uid,
        number: integer(root, tags::IN_CONCATENATION_NUMBER),
        total: integer(root, tags::IN_CONCATENATION_TOTAL_NUMBER),
        frame_offset: integer(root, tags::CONCATENATION_FRAME_OFFSET_NUMBER),
        source: string(root, tags::SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE),
        dimension_index: sequence(root, tags::DIMENSION_INDEX_SEQUENCE)
            .iter()
            .map(|item| digest(item, &[]))
            .collect(),
        shared_groups: sequence(root, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
            .iter()
            .map(|item| digest(item, &[]))
            .collect(),
        per_frame_structure: per_frame.into_iter().collect::<Vec<_>>().join(","),
        others: digest(root, &CONCATENATION_VARIANT),
    })
}

/// The tag paths of a Data Set and its Items, without Values.
fn structure(object: &InMemDicomObject, prefix: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for element in object.iter() {
        let tag = element.header().tag;
        let path = format!("{prefix}{:04X}{:04X}", tag.group(), tag.element());
        if let Some(items) = element.items() {
            for item in items {
                paths.extend(structure(item, &format!("{path}>")));
            }
        }
        paths.push(path);
    }
    paths
}

/// A digest of a Data Set's elements and Values, without `except`.
fn digest(object: &InMemDicomObject, except: &[dicom_core::Tag]) -> String {
    let mut hash = Sha256::new();
    for element in object
        .iter()
        .filter(|element| !except.contains(&element.header().tag))
    {
        hash.update(format!("{:?}{:?}", element.header().tag, element.value()).as_bytes());
    }
    format!("{:x}", hash.finalize())
}

pub(super) fn run_series_set_check(facts: &[SeriesFacts]) -> Option<ValidationCheck> {
    if facts.is_empty() {
        return None;
    }
    let mut violations = Vec::new();
    frames_of_reference(facts, &mut violations);
    uid_roles(facts, &mut violations);
    instance_numbers(facts, &mut violations);
    concatenations(facts, &mut violations);
    dimension_organizations(facts, &mut violations);
    localizers(facts, &mut violations);
    specimen_identifiers(facts, &mut violations);
    frame_extractions(facts, &mut violations);
    arrangements(facts, &mut violations);
    group_attributes(facts, &mut violations);
    lossy_versions(facts, &mut violations);
    Some(rule_check(
        Path::new(""),
        SERIES_SET_RULE,
        concat!(
            "DICOM PS3.3 ",
            dicom_edition!(),
            " C.7.1.4, C.7.4.1, C.7.6.1.1.5, C.7.6.16, C.7.6.22 and C.12.3, and PS3.5 ",
            dicom_edition!(),
            " 9"
        ),
        super::violation_result(violations),
    ))
}

/// C.7.4.1.1.1: each Series has a single Frame of Reference UID, and the
/// images sharing a slide-related one (C.8.12.2.1) are of the same slide.
fn frames_of_reference(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let mut slides = BTreeMap::<&str, (&str, &Path)>::new();
    for object in facts {
        let (Some(frame), Some(container)) = (&object.frame_of_reference, &object.container) else {
            continue;
        };
        match slides.get(frame.as_str()) {
            Some((existing, path)) if existing != container => violations.push(format!(
                "Frame of Reference UID {frame} is shared by images of slides {existing} ({}) and {container} ({}); images sharing a slide-related Frame of Reference are spatially related to one slide (PS3.3 {EDITION} C.7.4.1.1.1 and C.8.12.2.1)",
                path.display(),
                object.path.display()
            )),
            Some(_) => {}
            None => {
                slides.insert(frame, (container, &object.path));
            }
        }
    }
    let mut series = BTreeMap::<&str, (&str, &Path)>::new();
    for object in facts {
        let (Some(uid), Some(frame)) = (&object.series, &object.frame_of_reference) else {
            continue;
        };
        match series.get(uid.as_str()) {
            Some((existing, path)) if existing != frame => violations.push(format!(
                "Series {uid} has Frame of Reference UIDs {existing} ({}) and {frame} ({}); each Series has a single one (PS3.3 {EDITION} C.7.4.1.1.1)",
                path.display(),
                object.path.display()
            )),
            Some(_) => {}
            None => {
                series.insert(uid, (frame, &object.path));
            }
        }
    }
}

/// PS3.5 9: a UID identifies one thing, so one UID never plays two roles, and
/// a Series belongs to one Study.
fn uid_roles(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let mut roles = BTreeMap::<&str, (&str, &Path)>::new();
    let mut reported = BTreeSet::new();
    for object in facts {
        for (role, uid) in &object.uids {
            match roles.get(uid.as_str()) {
                Some((existing, path)) if existing != role && reported.insert(uid.as_str()) => {
                    violations.push(format!(
                        "UID {uid} is both a {existing} ({}) and a {role} ({}); UIDs identify one entity (PS3.5 {EDITION} 9)",
                        path.display(),
                        object.path.display()
                    ));
                }
                Some(_) => {}
                None => {
                    roles.insert(uid, (role, &object.path));
                }
            }
        }
    }
    let mut studies = BTreeMap::<&str, &str>::new();
    for object in facts {
        let (Some(series), Some(study)) = (&object.series, &object.study) else {
            continue;
        };
        if let Some(existing) = studies.insert(series, study) {
            if existing != study {
                violations.push(format!(
                    "Series {series} belongs to Studies {existing} and {study} (PS3.3 {EDITION} C.7.3.1)"
                ));
            }
        }
    }
}

/// C.7.6.16.1: Instance Number is shared by a Concatenation and otherwise
/// distinct within a Series.
fn instance_numbers(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let mut owners = BTreeMap::<(&str, i64), (Option<&str>, &Path)>::new();
    for object in facts {
        let (Some(series), Some(number)) = (&object.series, object.instance_number) else {
            continue;
        };
        let owner = object
            .concatenation
            .as_ref()
            .map(|concatenation| concatenation.uid.as_str());
        match owners.get(&(series.as_str(), number)) {
            Some((existing, path)) if owner.is_none() || existing != &owner => violations.push(format!(
                "Instance Number {number} is used by {} and {} in Series {series}; it is shared only within a Concatenation (PS3.3 {EDITION} C.7.6.16.1)",
                path.display(),
                object.path.display()
            )),
            Some(_) => {}
            None => {
                owners.insert((series, number), (owner, &object.path));
            }
        }
    }
}

/// C.7.6.16.2.2.4 and C.7.6.16.1: the SOP Instances of a Concatenation agree
/// on everything but the per-instance Attributes, and their numbers increase
/// in frame order. A supplied subset need not contain the first Instance.
fn concatenations(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let mut groups = BTreeMap::<&str, Vec<(&SeriesFacts, &Concatenation)>>::new();
    for object in facts {
        if let Some(concatenation) = &object.concatenation {
            groups
                .entry(&concatenation.uid)
                .or_default()
                .push((object, concatenation));
            if concatenation.total.is_some_and(|total| total <= 1) {
                violations.push(format!(
                    "{}: In-concatenation Total Number shall be greater than one (PS3.3 {EDITION} C.7.6.16.1)",
                    object.path.display()
                ));
            }
        }
    }
    for (uid, members) in groups {
        let (first, reference) = members[0];
        for (object, concatenation) in &members[1..] {
            for (differs, what) in [
                (
                    concatenation.dimension_index != reference.dimension_index,
                    "Dimension Index Sequence",
                ),
                (
                    concatenation.shared_groups != reference.shared_groups,
                    "Shared Functional Groups Sequence",
                ),
                (
                    concatenation.per_frame_structure != reference.per_frame_structure,
                    "Per-Frame Functional Groups Item structure",
                ),
                (
                    concatenation.others != reference.others,
                    "Attributes outside the per-instance ones",
                ),
                (
                    concatenation.source != reference.source,
                    "SOP Instance UID of Concatenation Source",
                ),
            ] {
                if differs {
                    violations.push(format!(
                        "Concatenation {uid}: {what} differs between {} and {} (PS3.3 {EDITION} C.7.6.16.2.2.4)",
                        first.path.display(),
                        object.path.display()
                    ));
                }
            }
        }
        for (object, concatenation) in &members {
            if concatenation.number.is_some_and(|number| number < 1)
                || concatenation
                    .number
                    .zip(concatenation.total)
                    .is_some_and(|(number, total)| number > total)
            {
                violations.push(format!(
                    "Concatenation {uid}: {} has In-concatenation Number {:?} outside 1..{:?} (PS3.3 {EDITION} C.7.6.16.1)",
                    object.path.display(),
                    concatenation.number,
                    concatenation.total
                ));
            }
            if concatenation.number == Some(1)
                && concatenation
                    .frame_offset
                    .is_some_and(|frame_offset| frame_offset != 0)
            {
                violations.push(format!(
                    "Concatenation {uid}: {} is In-concatenation Number 1 but has Concatenation Frame Offset Number {:?} instead of 0 (PS3.3 {EDITION} C.7.6.16.1)",
                    object.path.display(),
                    concatenation.frame_offset
                ));
            }
            if concatenation
                .number
                .zip(concatenation.frame_offset)
                .and_then(|(number, frame_offset)| {
                    number.checked_sub(1).map(|minimum| (frame_offset, minimum))
                })
                .is_some_and(|(frame_offset, minimum)| frame_offset < minimum)
            {
                violations.push(format!(
                    "Concatenation {uid}: {} has In-concatenation Number {:?} but Concatenation Frame Offset Number {:?}; each earlier member has at least one Frame (PS3.3 {EDITION} C.7.6.16.1)",
                    object.path.display(),
                    concatenation.number,
                    concatenation.frame_offset
                ));
            }
        }
        let mut ordered: Vec<_> = members
            .iter()
            .filter_map(|(object, concatenation)| {
                Some((*object, concatenation.number?, concatenation.frame_offset?))
            })
            .collect();
        ordered.sort_by_key(|(_, _, frame_offset)| *frame_offset);
        for pair in ordered.windows(2) {
            let (before, before_number, before_offset) = pair[0];
            let (after, after_number, after_offset) = pair[1];
            if after_number <= before_number || after_offset <= before_offset {
                violations.push(format!(
                    "Concatenation {uid}: In-concatenation Numbers and Concatenation Frame Offset Numbers are not strictly increasing between {} and {} (PS3.3 {EDITION} C.7.6.16.1)",
                    before.path.display(),
                    after.path.display()
                ));
                continue;
            }
            let Some(frames) = before.number_of_frames.filter(|frames| *frames > 0) else {
                continue;
            };
            let Some(after_known_frames) = before_offset.checked_add(frames) else {
                violations.push(format!(
                    "Concatenation {uid}: {} has Number of Frames {frames}, which cannot be added to Concatenation Frame Offset Number {before_offset} (PS3.3 {EDITION} C.7.6.16.1)",
                    before.path.display()
                ));
                continue;
            };
            let Some(number_gap) = after_number.checked_sub(before_number) else {
                continue;
            };
            if number_gap == 1 {
                if after_offset != after_known_frames {
                    violations.push(format!(
                        "Concatenation {uid}: {} has Concatenation Frame Offset Number {after_offset}, inconsistent with adjacent member {} (PS3.3 {EDITION} C.7.6.16.1)",
                        after.path.display(),
                        before.path.display()
                    ));
                }
            } else {
                let Some(minimum_offset) = number_gap
                    .checked_sub(1)
                    .and_then(|missing_members| after_known_frames.checked_add(missing_members))
                else {
                    violations.push(format!(
                        "Concatenation {uid}: the Frame ranges implied by {} and {} overflow (PS3.3 {EDITION} C.7.6.16.1)",
                        before.path.display(),
                        after.path.display()
                    ));
                    continue;
                };
                if after_offset < minimum_offset {
                    violations.push(format!(
                        "Concatenation {uid}: {} has Concatenation Frame Offset Number {after_offset}, which overlaps the Frames of {} or the intervening members (PS3.3 {EDITION} C.7.6.16.1)",
                        after.path.display(),
                        before.path.display()
                    ));
                }
            }
        }
    }
}

/// C.7.6.17.2: Instances sharing a Dimension Organization UID index the same
/// Attributes with it.
fn dimension_organizations(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let mut seen = BTreeMap::<&str, (BTreeSet<&str>, &Path)>::new();
    for object in facts {
        let mut mine = BTreeMap::<&str, BTreeSet<&str>>::new();
        for (uid, pointers) in &object.dimensions {
            mine.entry(uid).or_default().insert(pointers);
        }
        for (uid, pointers) in mine {
            match seen.get(uid) {
                Some((existing, path)) if existing != &pointers => violations.push(format!(
                    "Dimension Organization UID {uid} indexes different Attributes in {} and {}; shared indices shall have the same meaning (PS3.3 {EDITION} C.7.6.17.2)",
                    path.display(),
                    object.path.display()
                )),
                Some(_) => {}
                None => {
                    seen.insert(uid, (pointers, &object.path));
                }
            }
        }
    }
}

/// C.7.6.16.2.5.1: a localizer referenced for planning shares this image's
/// Frame of Reference.
fn localizers(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let frames: BTreeMap<&str, Option<&str>> = facts
        .iter()
        .filter_map(|object| {
            Some((
                object.sop_instance.as_deref()?,
                object.frame_of_reference.as_deref(),
            ))
        })
        .collect();
    for object in facts {
        for localizer in &object.localizers {
            if let Some(frame) = frames.get(localizer.as_str()) {
                if *frame != object.frame_of_reference.as_deref() {
                    violations.push(format!(
                        "{} references localizer {localizer} with a different Frame of Reference UID (PS3.3 {EDITION} C.7.6.16.2.5.1)",
                        object.path.display()
                    ));
                }
            }
        }
    }
}

/// C.7.6.22.1.2: within a Study, a Specimen Identifier and its issuer name one
/// specimen, so one Specimen UID.
fn specimen_identifiers(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let mut specimens = BTreeMap::<(&str, &(String, String)), (&str, &Path)>::new();
    for object in facts {
        let Some(study) = &object.study else {
            continue;
        };
        for (identifier, uid) in &object.specimens {
            match specimens.get(&(study.as_str(), identifier)) {
                Some((existing, path)) if existing != uid => violations.push(format!(
                    "Specimen Identifier {} names Specimen UIDs {existing} ({}) and {uid} ({}) in Study {study} (PS3.3 {EDITION} C.7.6.22.1.2)",
                    identifier.0,
                    path.display(),
                    object.path.display()
                )),
                Some(_) => {}
                None => {
                    specimens.insert((study, identifier), (uid, &object.path));
                }
            }
        }
    }
}

/// Table C.12-9: an Instance extracted from one with a Frame Extraction
/// Sequence holds all of the parent's Items and a new one.
fn frame_extractions(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let instances: BTreeMap<&str, &SeriesFacts> = facts
        .iter()
        .filter_map(|object| Some((object.sop_instance.as_deref()?, object)))
        .collect();
    for object in facts {
        let sources: BTreeSet<&str> = object
            .extractions
            .iter()
            .filter_map(|(source, _)| source.as_deref())
            .collect();
        for parent in sources
            .into_iter()
            .filter_map(|source| instances.get(source))
            .filter(|parent| !parent.extractions.is_empty() && !std::ptr::eq(**parent, object))
        {
            let carried = parent
                .extractions
                .iter()
                .all(|(_, item)| object.extractions.iter().any(|(_, mine)| mine == item));
            if !carried || object.extractions.len() != parent.extractions.len() + 1 {
                violations.push(format!(
                    "{} was extracted from {}, so its Frame Extraction Sequence shall hold all {} Items of the parent's and one new Item, but has {} Items{} (PS3.3 {EDITION} Table C.12-9)",
                    object.path.display(),
                    parent.path.display(),
                    parent.extractions.len(),
                    object.extractions.len(),
                    if carried { "" } else { " without all of the parent's" }
                ));
            }
        }
    }
}

/// C.7.1.4.1.1.1: the same non-human organisms imaged together in a different
/// arrangement are a different group, with a different Patient ID.
fn arrangements(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let mut seen =
        BTreeMap::<(&str, &BTreeSet<String>), (&BTreeSet<(String, Vec<i64>)>, &Path)>::new();
    for object in facts {
        let Some(arrangement) = &object.arrangement else {
            continue;
        };
        let key = (arrangement.group.as_str(), &arrangement.members);
        match seen.get(&key) {
            Some((positions, path)) if *positions != &arrangement.positions => violations.push(format!(
                "Patient ID {} names a group arranged differently in {} and {}; a different arrangement of the same organisms uses a different Patient ID (PS3.3 {EDITION} C.7.1.4.1.1.1)",
                arrangement.group,
                path.display(),
                object.path.display()
            )),
            Some(_) => {}
            None => {
                seen.insert(key, (&arrangement.positions, &object.path));
            }
        }
    }
}

/// C.7.1.4.1.1: Patient Module Attributes present for a group of non-human
/// organisms imaged together are those the entire group shares, so none
/// differs from the same Attribute of a member imaged alone in the set.
fn group_attributes(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let individuals: BTreeMap<&str, &SeriesFacts> = facts
        .iter()
        .filter(|object| object.arrangement.is_none())
        .filter_map(|object| Some((object.patient_id.as_deref()?, object)))
        .collect();
    for object in facts {
        let Some(arrangement) = &object.arrangement else {
            continue;
        };
        let differing = arrangement.members.iter().find_map(|member| {
            let individual = individuals.get(member.as_str())?;
            object.patient.iter().find_map(|(index, value)| {
                individual
                    .patient
                    .iter()
                    .find(|(other, own)| other == index && own != value)
                    .map(|(_, own)| (member, *index, value, own, &individual.path))
            })
        });
        if let Some((member, index, value, own, path)) = differing {
            violations.push(format!(
                "{} images the group {} with {} \"{value}\", but member {member} is \"{own}\" in {}; the Patient Module Attributes of a group are those its entire group shares (PS3.3 {EDITION} C.7.1.4.1.1)",
                object.path.display(),
                arrangement.group,
                GROUP_SHARED[index].1,
                path.display()
            ));
        }
    }
}

/// C.7.6.1.1.5: Lossy Image Compression is not reset once 01, and a lossy
/// compressed version of an image is a new SOP Instance, so two files of one
/// SOP Instance agree on it.
fn lossy_versions(facts: &[SeriesFacts], violations: &mut Vec<String>) {
    let mut seen = BTreeMap::<&str, (&str, &Path)>::new();
    for object in facts {
        let (Some(uid), Some(lossy)) = (&object.sop_instance, &object.lossy) else {
            continue;
        };
        match seen.get(uid.as_str()) {
            Some((existing, path)) if *existing != lossy.as_str() => violations.push(format!(
                "SOP Instance {uid} has Lossy Image Compression (0028,2110) {existing} in {} and {lossy} in {}; once 01 it is not reset, and a lossy compressed version of an image is a new SOP Instance (PS3.3 {EDITION} C.7.6.1.1.5)",
                path.display(),
                object.path.display()
            )),
            Some(_) => {}
            None => {
                seen.insert(uid, (lossy, &object.path));
            }
        }
    }
}
