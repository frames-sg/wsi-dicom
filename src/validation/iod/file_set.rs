//! PS3.3 Annex F Basic Directory IOD as data, for the rule that holds a
//! File-set and its DICOMDIR to PS3.3 Annex F and PS3.10 Chapter 8.
//!
//! The IOD checks hold a DICOMDIR (Media Storage Directory Storage) to the
//! generated Basic Directory IOD tables, each Directory Record to the Record
//! Selection Keys its Directory Record Type (0004,1430) selects; what spans
//! records and files (offsets, the Table F.4-1 hierarchy, referenced Files)
//! is the File-set rule's.

use super::engine::flatten;
use super::model::Table;
use super::tables::{BASIC_DIRECTORY, DIRECTORY_RECORDS, ROOT_DIRECTORY_RECORDS};

/// One top-level Record Selection Key of a Directory Record Type (PS3.3 F.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::validation) struct RecordKey {
    pub(in crate::validation) tag: u32,
    pub(in crate::validation) name: &'static str,
    /// "1", "1C", "2", "2C" or "3".
    pub(in crate::validation) kind: &'static str,
}

/// A PS3.3 Table F.4-1 Directory Record Type.
#[derive(Debug)]
pub(in crate::validation) struct DirectoryRecord {
    /// Its Directory Record Type (0004,1430) Value, e.g. "PATIENT".
    pub(in crate::validation) record_type: &'static str,
    /// The PS3.3 section defining it, e.g. "F.5.1".
    pub(in crate::validation) section: &'static str,
    /// Its Record Selection Keys; `None` for the F.6.1 Private Directory Record.
    pub(super) keys: Option<&'static Table>,
    /// The Directory Record Types its lower-level Directory Entity may include.
    pub(in crate::validation) lower_level: &'static [&'static str],
    /// Whether that Entity may also include any of the Types as privately
    /// defined (a PRIVATE record, Table F.4-1).
    pub(in crate::validation) any_private_lower_level: bool,
}

/// The Media Storage Directory Storage SOP Class UID of a DICOMDIR.
pub(in crate::validation) fn basic_directory_sop_class() -> &'static str {
    BASIC_DIRECTORY.sop_classes[0].uid
}

/// The PS3.3 section of the Basic Directory IOD, "F.3".
pub(in crate::validation) fn basic_directory_section() -> &'static str {
    BASIC_DIRECTORY.section
}

/// Every PS3.3 Table F.4-1 Directory Record Type, in its order.
pub(in crate::validation) fn directory_records() -> &'static [DirectoryRecord] {
    DIRECTORY_RECORDS
}

/// The Directory Record a Directory Record Type (0004,1430) Value names.
pub(in crate::validation) fn directory_record(
    record_type: &str,
) -> Option<&'static DirectoryRecord> {
    DIRECTORY_RECORDS
        .iter()
        .find(|record| record.record_type == record_type)
}

/// The Directory Record Types the Root Directory Entity may include (Table
/// F.4-1).
pub(in crate::validation) fn root_directory_records() -> &'static [&'static str] {
    ROOT_DIRECTORY_RECORDS
}

impl DirectoryRecord {
    /// Whether a Directory Record of `record_type` may be in the lower-level
    /// Directory Entity this record references (Table F.4-1).
    pub(in crate::validation) fn admits_lower_level(&self, record_type: &str) -> bool {
        self.lower_level.contains(&record_type)
    }

    /// Its top-level Record Selection Keys, the Macros its key table
    /// includes expanded; empty for the F.6.1 Private Directory Record. The
    /// IOD checks hold each record to them, conditions and Items included.
    pub(in crate::validation) fn selection_keys(&self) -> Vec<RecordKey> {
        let Some(table) = self.keys else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        flatten(table.nodes, table, &mut rows);
        rows.iter()
            .map(|row| RecordKey {
                tag: row.attribute.tag,
                name: row.attribute.name,
                kind: row.attribute.kind.label(),
            })
            .collect()
    }

    /// The PS3.3 table of its Record Selection Keys, e.g. "F.5-1"; `None`
    /// for the F.6.1 Private Directory Record, whose keys are private.
    pub(in crate::validation) fn keys_table(&self) -> Option<&'static str> {
        self.keys.map(|table| table.label)
    }
}
