//! PS3.15 E.1.1 over a set of objects: "It is the responsibility of the
//! de-identifier to ensure the consistency of dummy values ... if multiple
//! related SOP Instances are protected", so the de-identified instances of
//! one pyramid, series or study still agree on what identifies the entities
//! above them. And a SOP Instance UID that a non-de-identified object in the
//! set also carries was provably not replaced (Table E.1-1 action U), unless
//! the Retain UIDs Option is declared. The DICOMDIR of a File-set holding
//! de-identified Files is held to E.1.1 by `directory`.

mod directory;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::{report, Declaration};
use crate::validation::iod::deidentification::ConfidentialityOption;
use crate::validation::iod::{items, string_value, Family};
use crate::validation::wsi_conformance::iod_of;
use crate::validation::{ValidationCheck, ValidationOptions};

/// The Attributes each related set of de-identified instances shares: the
/// key naming the entity, then what its instances agree on (E.1.1: "all
/// Attributes of every entity about the Instance level should remain
/// consistent for all Instances protected").
const ENTITIES: &[(Field, &[Field])] = &[
    (
        Field::Tag(tags::PYRAMID_UID, "Pyramid UID (0008,0019)"),
        &[
            Field::Tag(
                tags::FRAME_OF_REFERENCE_UID,
                "Frame of Reference UID (0020,0052)",
            ),
            Field::Tag(
                tags::CONTAINER_IDENTIFIER,
                "Container Identifier (0040,0512)",
            ),
            Field::SpecimenUids,
            Field::Tag(tags::STUDY_INSTANCE_UID, "Study Instance UID (0020,000D)"),
            Field::Tag(tags::PATIENT_ID, "Patient ID (0010,0020)"),
        ],
    ),
    (
        Field::Tag(tags::SERIES_INSTANCE_UID, "Series Instance UID (0020,000E)"),
        &[
            Field::Tag(tags::SERIES_NUMBER, "Series Number (0020,0011)"),
            Field::Tag(tags::STUDY_INSTANCE_UID, "Study Instance UID (0020,000D)"),
            Field::Tag(tags::PATIENT_ID, "Patient ID (0010,0020)"),
        ],
    ),
    (
        Field::Tag(tags::STUDY_INSTANCE_UID, "Study Instance UID (0020,000D)"),
        &[
            Field::Tag(tags::STUDY_ID, "Study ID (0020,0010)"),
            Field::Tag(tags::PATIENT_ID, "Patient ID (0010,0020)"),
            Field::Tag(tags::PATIENT_NAME, "Patient's Name (0010,0010)"),
        ],
    ),
];

#[derive(Clone, Copy, Debug)]
enum Field {
    Tag(Tag, &'static str),
    /// Every Specimen UID (0040,0554) of the Specimen Description Sequence.
    SpecimenUids,
}

impl Field {
    fn name(self) -> &'static str {
        match self {
            Self::Tag(_, name) => name,
            Self::SpecimenUids => "the Specimen UIDs (0040,0554)",
        }
    }

    fn value(self, object: &InMemDicomObject) -> Option<String> {
        match self {
            Self::Tag(tag, _) => string_value(object, tag, 1),
            Self::SpecimenUids => {
                let mut uids: Vec<String> = items(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE)
                    .iter()
                    .filter_map(|specimen| string_value(specimen, tags::SPECIMEN_UID, 1))
                    .collect();
                uids.sort();
                (!uids.is_empty()).then(|| uids.join("\\"))
            }
        }
    }
}

/// What the set check compares, gathered from each object of a
/// pathology-slide IOD as it is read.
#[derive(Debug, Default)]
pub(in crate::validation) struct DeidentificationCorpus {
    instances: Vec<Facts>,
    /// The File-set roots discovery found, and the bounds their DICOMDIRs
    /// are read within; none for objects read without a File-set (DICOM
    /// JSON).
    file_sets: Option<(Vec<PathBuf>, ValidationOptions)>,
}

#[derive(Debug)]
struct Facts {
    path: PathBuf,
    /// Patient Identity Removed YES or a De-identification Method named.
    claimed: bool,
    /// The Retain UIDs Option may apply: declared, or the Options are named
    /// by text alone.
    retains_uids: bool,
    sop_instance_uid: Option<String>,
    /// Each `ENTITIES` key, then its compared Fields, in order.
    entities: Vec<(Option<String>, Vec<Option<String>>)>,
    /// What the object declares, which the records describing it are held to.
    declaration: Declaration,
    /// Of a de-identified object, each of `directory::RECORD_KEYS` as it
    /// holds it; empty otherwise.
    record_keys: Vec<Option<String>>,
}

impl DeidentificationCorpus {
    /// Records what the set check needs of `object`, read from `path`; an
    /// object of no pathology-slide IOD adds nothing.
    pub(in crate::validation) fn observe(&mut self, path: &Path, object: &DefaultDicomObject) {
        if iod_of(object).is_none() {
            return;
        }
        let declaration = Declaration::of(object);
        let entities = ENTITIES
            .iter()
            .map(|(key, fields)| {
                (
                    key.value(object),
                    fields.iter().map(|field| field.value(object)).collect(),
                )
            })
            .collect();
        let record_keys = if declaration.claimed() {
            directory::record_keys(object)
        } else {
            Vec::new()
        };
        self.instances.push(Facts {
            path: path.to_path_buf(),
            claimed: declaration.claimed(),
            retains_uids: !declaration.options_known()
                || declaration.has(ConfidentialityOption::RetainUIDs),
            sop_instance_uid: string_value(object, tags::SOP_INSTANCE_UID, 1),
            entities,
            declaration,
            record_keys,
        });
    }

    /// Records the File-set roots discovery found (directories holding a
    /// DICOMDIR, PS3.10 8.6), whose records the set check holds to PS3.15
    /// E.1.1 where they describe de-identified Files, reading each DICOMDIR
    /// within `options`.
    pub(in crate::validation) fn observe_file_sets(
        &mut self,
        roots: &[PathBuf],
        options: &ValidationOptions,
    ) {
        self.file_sets = Some((roots.to_vec(), options.clone()));
    }
}

/// The de-identification check of the set `corpus` observed, with its
/// advisories by file; none when no observed object claims de-identification.
pub(in crate::validation) fn run_deidentification_set_check(
    corpus: &DeidentificationCorpus,
) -> Option<(ValidationCheck, Vec<(PathBuf, String)>)> {
    let protected: Vec<&Facts> = corpus
        .instances
        .iter()
        .filter(|facts| facts.claimed)
        .collect();
    if protected.is_empty() {
        return None;
    }
    let mut violations = Family::default();
    inconsistent_dummies(&protected, &mut violations);
    retained_instance_uids(corpus, &mut violations);
    let mut advisories = partly_protected(corpus);
    if let Some((roots, options)) = &corpus.file_sets {
        directory::check_file_sets(roots, options, &protected, &mut violations, &mut advisories);
    }
    // The set check names no one file.
    let check = report(
        Path::new(""),
        &violations,
        advisories
            .iter()
            .map(|(path, advisory)| format!("{}: {advisory}", path.display())),
    );
    Some((check, advisories))
}

/// E.1.1: the de-identified instances of one entity disagree on a Value
/// the entity's instances share.
fn inconsistent_dummies(protected: &[&Facts], violations: &mut Family) {
    for (index, (key, fields)) in ENTITIES.iter().enumerate() {
        let mut first = BTreeMap::<&str, &Facts>::new();
        for facts in protected {
            let Some(uid) = facts.entities[index].0.as_deref() else {
                continue;
            };
            let Some(earlier) = first.get(uid) else {
                first.insert(uid, facts);
                continue;
            };
            for (field_index, field) in fields.iter().enumerate() {
                let here = &facts.entities[index].1[field_index];
                let there = &earlier.entities[index].1[field_index];
                if here != there {
                    violations.push(|| {
                        format!(
                            "E.1.1: the de-identified instances of {} {uid} disagree on {}: {} in {} and {} in {}; the de-identifier keeps dummy values consistent across related instances",
                            key.name(),
                            field.name(),
                            quoted(there),
                            earlier.path.display(),
                            quoted(here),
                            facts.path.display()
                        )
                    });
                }
            }
        }
    }
}

fn quoted(value: &Option<String>) -> String {
    value
        .as_deref()
        .map_or_else(|| "none".to_string(), |value| format!("\"{value}\""))
}

/// E.1.1 and Table E.1-1 action U: a de-identified object whose SOP
/// Instance UID a non-de-identified object of the set also carries kept
/// the original UID, since one UID cannot name two Data Sets.
fn retained_instance_uids(corpus: &DeidentificationCorpus, violations: &mut Family) {
    let originals: BTreeMap<&str, &Path> = corpus
        .instances
        .iter()
        .filter(|facts| !facts.claimed)
        .filter_map(|facts| Some((facts.sop_instance_uid.as_deref()?, facts.path.as_path())))
        .collect();
    let retained = corpus
        .instances
        .iter()
        .filter(|facts| facts.claimed && !facts.retains_uids)
        .filter_map(|facts| {
            let uid = facts.sop_instance_uid.as_deref()?;
            Some((facts, uid, *originals.get(uid)?))
        });
    for (facts, uid, original) in retained {
        violations.push(|| {
            format!(
                "E.1.1 and Table E.1-1: SOP Instance UID (0008,0018) {uid} of de-identified {} is the UID of non-de-identified {}, so action U did not replace it and the Retain UIDs Option is not declared",
                facts.path.display(),
                original.display()
            )
        });
    }
}

/// A pyramid whose instances are partly de-identified: the instances left
/// unprotected identify the slide the protected ones show.
fn partly_protected(corpus: &DeidentificationCorpus) -> Vec<(PathBuf, String)> {
    let mut pyramids = BTreeMap::<&str, (bool, bool)>::new();
    for facts in &corpus.instances {
        if let Some(uid) = facts.entities[0].0.as_deref() {
            let entry = pyramids.entry(uid).or_default();
            if facts.claimed {
                entry.0 = true;
            } else {
                entry.1 = true;
            }
        }
    }
    corpus
        .instances
        .iter()
        .filter(|facts| !facts.claimed)
        .filter_map(|facts| {
            let uid = facts.entities[0].0.as_deref()?;
            (pyramids.get(uid) == Some(&(true, true))).then(|| {
                (
                    facts.path.clone(),
                    format!(
                        "Pyramid UID (0008,0019) {uid} has de-identified instances, but this one claims no de-identification (PS3.15 E.1.1 protects related instances together)"
                    ),
                )
            })
        })
        .collect()
}
