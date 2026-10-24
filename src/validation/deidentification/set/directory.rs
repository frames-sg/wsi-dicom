//! PS3.15 E.1.1 over the DICOMDIR of a File-set: "If a DICOMDIR File is
//! required, it shall either be created from the de-identified DICOM Files
//! it references, or an existing DICOMDIR File shall be de-identifed
//! according to this Profile", and "Any existing non-de-identified DICOMDIR
//! File shall be removed from the File-set".
//!
//! A record describes the File it references and, as a PATIENT, STUDY or
//! SERIES record above it, the Files of the records below it. A record
//! describing de-identified Files is held to what neither DICOMDIR E.1.1
//! admits could hold:
//! - a Value that Table E.1-1, or under the Clean Structured Content Option
//!   Table E.3.4-1, removes under the Options of every File it describes, at
//!   any depth;
//! - a Record Key that PS3.3 F.5 takes from the File and the Profile
//!   replaces, other than every File it describes carries: a DICOMDIR
//!   created from the de-identified Files carries their Keys, and so does
//!   one de-identified itself, as F.5 still binds it.
//!
//! What only the DICOMDIR before de-identification could show (whether a
//! Value it holds is a dummy) is not claimed. The records are read through
//! `file_set::listing`, within the File-set check's bounds; a DICOMDIR it
//! cannot list is named as an advisory, never passed over.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::super::actions::{candidates, check_record, RecordAt};
use super::{Declaration, Facts};
use crate::validation::file_set::listing::{directory_records, DirectoryRecord};
use crate::validation::iod::deidentification::{confidentiality_attribute, DeidentificationAction};
use crate::validation::iod::{element_label, string_value, Family};
use crate::validation::ValidationOptions;

/// PS3.10 8.6: the reserved File ID of the DICOMDIR File.
const DICOMDIR: &str = "DICOMDIR";

/// The Record Keys PS3.3 F.5 takes from the referenced File, as the
/// File-set check compares them, that Table E.1-1 replaces.
pub(super) const RECORD_KEYS: [Tag; 4] = [
    tags::PATIENT_NAME,
    tags::PATIENT_ID,
    tags::STUDY_INSTANCE_UID,
    tags::SERIES_INSTANCE_UID,
];

/// Each of [`RECORD_KEYS`] as `object` holds it.
pub(super) fn record_keys(object: &InMemDicomObject) -> Vec<Option<String>> {
    RECORD_KEYS
        .iter()
        .map(|tag| string_value(object, *tag, 1))
        .collect()
}

/// Holds the DICOMDIR of each File-set root to E.1.1 where its records
/// describe one of the de-identified Files `protected`.
pub(super) fn check_file_sets(
    roots: &[PathBuf],
    options: &ValidationOptions,
    protected: &[&Facts],
    violations: &mut Family,
    advisories: &mut Vec<(PathBuf, String)>,
) {
    if roots.is_empty() {
        return;
    }
    let files = by_canonical_path(protected, advisories);
    for root in roots {
        let dicomdir = root.join(DICOMDIR);
        match directory_records(root, options) {
            Ok(records) => check_records(&dicomdir, &records, &files, violations),
            Err(reason) => advisories.push((
                dicomdir,
                format!(
                    "its records cannot be read, so it is not held to PS3.15 E.1.1 for the de-identified Files it references: {reason}"
                ),
            )),
        }
    }
}

/// The de-identified Files by canonical path, as Referenced File IDs
/// resolve. A File that cannot be resolved is named as an advisory, since
/// no record can be matched to it.
fn by_canonical_path<'a>(
    protected: &[&'a Facts],
    advisories: &mut Vec<(PathBuf, String)>,
) -> BTreeMap<PathBuf, &'a Facts> {
    let mut files = BTreeMap::new();
    for facts in protected {
        match facts.path.canonicalize() {
            Ok(path) => {
                files.insert(path, *facts);
            }
            Err(err) => advisories.push((
                facts.path.clone(),
                format!(
                    "it cannot be resolved ({err}), so no DICOMDIR record that references it is held to PS3.15 E.1.1"
                ),
            )),
        }
    }
    files
}

/// Holds each record of the DICOMDIR at `dicomdir` that describes a File
/// of `files` to E.1.1.
fn check_records(
    dicomdir: &Path,
    records: &[DirectoryRecord],
    files: &BTreeMap<PathBuf, &Facts>,
    violations: &mut Family,
) {
    let mut described: Vec<Vec<&Facts>> = vec![Vec::new(); records.len()];
    for (index, record) in records.iter().enumerate() {
        let Some(facts) = record
            .referenced_file
            .as_ref()
            .and_then(|path| files.get(path))
            .copied()
        else {
            continue;
        };
        for at in record
            .ancestry
            .iter()
            .copied()
            .chain(std::iter::once(index))
        {
            if let Some(entry) = described.get_mut(at) {
                entry.push(facts);
            }
        }
    }
    for (record, files) in records.iter().zip(&described) {
        if files.is_empty() {
            continue;
        }
        let at = RecordAt {
            dicomdir,
            record_type: &record.record_type,
            offset: record.offset,
        };
        let declaration = covering(files.iter().map(|facts| &facts.declaration));
        check_record(&record.attributes, at, &declaration, violations);
        check_keys(record, at, files, &declaration, violations);
    }
}

/// What a record describing the Files of `declarations` is held to: the
/// union of their Options. An Option only ever gives an action K or C, so
/// the union relaxes each Table E.1-1 and E.3.4-1 action at least as much
/// as any one declaration does, and what breaks it breaks each. When a File
/// names its Options by text alone, they are unknown.
fn covering<'a>(declarations: impl IntoIterator<Item = &'a Declaration>) -> Declaration {
    let mut covering = Declaration {
        basic: true,
        ..Declaration::default()
    };
    for declaration in declarations {
        if !declaration.options_known() {
            return Declaration::default();
        }
        covering.options.extend(declaration.options.iter().copied());
    }
    covering
}

/// The [`RECORD_KEYS`] of `record` that the Profile replaces under
/// `declaration`, each held to the Values the de-identified `files` it
/// describes carry. An empty Key holds nothing to compare.
fn check_keys(
    record: &DirectoryRecord,
    at: RecordAt<'_>,
    files: &[&Facts],
    declaration: &Declaration,
    violations: &mut Family,
) {
    for (key, tag) in RECORD_KEYS.into_iter().enumerate() {
        let Some(value) = string_value(&record.attributes, tag, 1) else {
            continue;
        };
        let replaced = confidentiality_attribute(tag).is_some_and(|row| {
            !candidates(row.basic, row.options, declaration).contains(&DeidentificationAction::K)
        });
        let carried = files.iter().any(|facts| {
            facts.record_keys.get(key).and_then(Option::as_deref) == Some(value.as_str())
        });
        if !replaced || carried {
            continue;
        }
        violations.push(|| {
            format!(
                "{at} > {} is not the Value {}; a DICOMDIR created from the de-identified Files it references, or itself de-identified, carries their Record Keys (PS3.15 E.1.1, PS3.3 F.5), and Table E.1-1 replaces this one",
                element_label("", tag),
                carriers(files)
            )
        });
    }
}

/// "the de-identified File <path> carries", or how many of them, naming
/// the first.
fn carriers(files: &[&Facts]) -> String {
    match files {
        [only] => format!("the de-identified File {} carries", only.path.display()),
        [first, ..] => format!(
            "any of the {} de-identified Files it describes carries, such as {}",
            files.len(),
            first.path.display()
        ),
        [] => "any de-identified File carries".to_string(),
    }
}
