//! Bounded violation collection and check construction.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::engine::Definition;
use super::tables::ConditionId;
use crate::validation::{ValidationCheck, ValidationStatus};

/// Violations shown per check; the rest are counted.
const MAX_SHOWN: usize = 20;
/// Longest condition excerpt quoted in a message.
const MAX_CONDITION_EXCERPT: usize = 100;

#[derive(Debug, Default)]
pub(super) struct Family {
    shown: Vec<String>,
    total: usize,
    undetermined: BTreeMap<ConditionId, usize>,
}

impl Family {
    /// Counts a violation; `message` is formatted only while it will be shown.
    pub(super) fn push(&mut self, message: impl FnOnce() -> String) {
        self.total += 1;
        if self.shown.len() < MAX_SHOWN {
            self.shown.push(message());
        }
    }

    /// Moves `other`'s violations and undeterminable counts into this Family.
    pub(super) fn absorb(&mut self, other: Family) {
        let hidden = other.total.saturating_sub(other.shown.len());
        for message in other.shown {
            self.push(|| message);
        }
        self.total += hidden;
        for (condition, count) in other.undetermined {
            *self.undetermined.entry(condition).or_insert(0) += count;
        }
    }

    pub(super) fn undetermined(&mut self, condition: ConditionId) {
        *self.undetermined.entry(condition).or_insert(0) += 1;
    }

    fn message(&self, citation: &str) -> String {
        let mut message = if self.total == 0 {
            format!("{citation}: conformant")
        } else {
            let mut text = format!(
                "{citation}: {} violation{}: {}",
                self.total,
                if self.total == 1 { "" } else { "s" },
                self.shown.join("; ")
            );
            if self.total > self.shown.len() {
                text.push_str(&format!("; and {} more", self.total - self.shown.len()));
            }
            text
        };
        if !self.undetermined.is_empty() {
            let evaluations: usize = self.undetermined.values().sum();
            message.push_str(&format!(
                "; {evaluations} condition evaluation{} undeterminable from the object ({} distinct condition{}, not failed)",
                if evaluations == 1 { "" } else { "s" },
                self.undetermined.len(),
                if self.undetermined.len() == 1 { "" } else { "s" },
            ));
        }
        message
    }

    pub(super) fn check(&self, path: &Path, name: &str, citation: &str) -> ValidationCheck {
        ValidationCheck {
            execution: None,
            name: name.to_string(),
            path: (!path.as_os_str().is_empty()).then(|| path.to_path_buf()),
            status: if self.total == 0 {
                ValidationStatus::Passed
            } else {
                ValidationStatus::Failed
            },
            command: Vec::new(),
            message: self.message(citation),
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

/// Findings of one SOP Instance, one Family per check.
#[derive(Debug, Default)]
pub(super) struct Findings {
    pub(super) modules: Family,
    pub(super) presence: Family,
    pub(super) enumerated: Family,
    pub(super) vr_vm: Family,
    pub(super) value_format: Family,
    pub(super) sequence_items: Family,
    pub(super) file_meta: Family,
    pub(super) optional_absent: OptionalAbsent,
}

/// Optional modules, functional groups and rows the object does not encode.
/// A row is recorded once however many Items lack it.
#[derive(Debug, Default)]
pub(super) struct OptionalAbsent {
    groups: BTreeSet<String>,
    rows: BTreeMap<(usize, usize), Definition>,
}

impl OptionalAbsent {
    pub(super) fn group(&mut self, label: String) {
        self.groups.insert(label);
    }

    pub(super) fn row(&mut self, definition: Definition) {
        let key = (
            std::ptr::from_ref(definition.attribute) as usize,
            std::ptr::from_ref(definition.table) as usize,
        );
        self.rows.entry(key).or_insert(definition);
    }

    pub(super) fn describe(&self) -> Vec<String> {
        let mut rows = self
            .rows
            .values()
            .map(|definition| {
                format!(
                    "{} {}: {}",
                    definition.attribute.name,
                    tag_text(definition.attribute.tag),
                    definition.citation()
                )
            })
            .collect::<Vec<_>>();
        rows.sort();
        self.groups.iter().cloned().chain(rows).collect()
    }
}

/// "(gggg,eeee)".
pub(super) fn tag_text(tag: u32) -> String {
    format!("({:04X},{:04X})", tag >> 16, tag & 0xFFFF)
}

/// `: "<condition text>"` for a message about an unmet condition, or nothing.
pub(super) fn condition_suffix(condition: Option<ConditionId>) -> String {
    condition
        .map(|condition| format!(": \"{}\"", excerpt(condition.text(), MAX_CONDITION_EXCERPT)))
        .unwrap_or_default()
}

/// The first `limit` characters of `text`, marked when cut.
pub(super) fn excerpt(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        format!("{}...", text.chars().take(limit).collect::<String>())
    }
}
