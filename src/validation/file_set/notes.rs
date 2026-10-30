//! What the File-set check leaves unvalidated, each said once in its
//! message as an advisory, never as a failure: records of a Directory
//! Record Type outside the pathology-slide scope, and Content Sequences not
//! compared with a document.

use std::collections::BTreeMap;

use super::records::{Record, DIRECTORY_RECORD_TYPE};
use crate::validation::disclosure::shown;
use crate::validation::iod::file_set::directory_record;

/// The Directory Record Types of the pathology-slide objects and their
/// hierarchy, whose records the check validates in full.
const VALIDATED_TYPES: &[&str] = &[
    "PATIENT",
    "STUDY",
    "SERIES",
    "IMAGE",
    "SR DOCUMENT",
    "KEY OBJECT DOC",
    "ANNOTATION",
];

/// Advisories, each with the number of times it was noted.
#[derive(Debug, Default)]
pub(super) struct Notes {
    lines: BTreeMap<String, usize>,
}

impl Notes {
    pub(super) fn note(&mut self, line: String) {
        *self.lines.entry(line).or_insert(0) += 1;
    }

    /// Notes once each Directory Record Type of `records` the check does
    /// not validate in full, with its record count.
    pub(super) fn record_types(&mut self, records: &[Record]) {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for record in records {
            let record_type = record.record_type();
            if !VALIDATED_TYPES.contains(&record_type.as_str()) {
                *counts.entry(record_type).or_insert(0) += 1;
            }
        }
        for (record_type, count) in counts {
            let why = match (record_type.as_str(), directory_record(&record_type)) {
                ("PRIVATE", _) => "its private keys and the objects it references are defined \
                                   only by its Private Record UID (PS3.3 F.6.1)"
                    .to_string(),
                (_, Some(_)) => "it references objects outside the pathology-slide scope, so \
                                 which SOP Class it references (PS3.3 F.5) and the referenced \
                                 objects are not checked"
                    .to_string(),
                (_, None) => "it is not a Directory Record Type of PS3.3 Table F.4-1".to_string(),
            };
            self.lines.insert(
                format!(
                    "{count} record{} of Directory Record Type {} not validated: {why}",
                    if count == 1 { "" } else { "s" },
                    shown(DIRECTORY_RECORD_TYPE, &record_type)
                ),
                1,
            );
        }
    }

    /// The advisories as a message suffix, or "" when there are none.
    pub(super) fn suffix(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let shown: Vec<String> = self
            .lines
            .iter()
            .map(|(line, count)| match count {
                1 => line.clone(),
                count => format!("{line} ({count} times)"),
            })
            .collect();
        format!("; advisory, not a failure: {}", shown.join("; "))
    }
}
