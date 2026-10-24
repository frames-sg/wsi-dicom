//! The Directory Entities of PS3.3 F.3.2.2 and F.2.1: every offset points at
//! the Item tag of a Directory Record, each Directory Entity is a chain that
//! ends, each record belongs to one Directory Entity reached from the Root
//! Directory Entity, and each record sits under a parent of a type Table
//! F.4-1 allows.

use std::collections::BTreeMap;

use dicom_core::Tag;

use super::records::{Directory, Record};
use crate::validation::iod::file_set::{directory_record, root_directory_records};
use crate::validation::iod::{Family, MAX_NESTING};

const FIRST_ROOT_RECORD: Tag = Tag(0x0004, 0x1200);
const LAST_ROOT_RECORD: Tag = Tag(0x0004, 0x1202);
const CONSISTENCY_FLAG: Tag = Tag(0x0004, 0x1212);
const NEXT_RECORD: Tag = Tag(0x0004, 0x1400);
const IN_USE_FLAG: Tag = Tag(0x0004, 0x1410);
const LOWER_ENTITY: Tag = Tag(0x0004, 0x1420);

/// Table F.4-1 gives SURFACE SCAN a row of its own, and F.5 its keys, but
/// leaves it out of the types a SERIES record's lower-level Directory Entity
/// may include; it is accepted there.
const SURFACE_SCAN: &str = "SURFACE SCAN";

/// Where each Directory Record sits: its parent record, if it was reached.
#[derive(Debug, Default)]
pub(super) struct Tree {
    /// `Some(None)` for a record of the Root Directory Entity,
    /// `Some(Some(parent))` below `parent`, `None` when unreachable.
    pub(super) parent: Vec<Option<Option<usize>>>,
}

impl Tree {
    /// `index` and its ancestors, nearest first.
    pub(super) fn lineage(&self, index: usize) -> Vec<usize> {
        let mut lineage = vec![index];
        let mut current = index;
        while let Some(Some(parent)) = self.parent.get(current).copied().flatten() {
            if lineage.len() > self.parent.len() {
                break;
            }
            lineage.push(parent);
            current = parent;
        }
        lineage
    }
}

/// Checks the File-set Consistency Flag and every offset, chain and
/// parent-child relationship of `directory`; returns where each record sits.
pub(super) fn check_structure(directory: &Directory, family: &mut Family) -> Tree {
    check_consistency_flag(directory, family);
    let records = &directory.records;
    let index: BTreeMap<u64, usize> = records
        .iter()
        .enumerate()
        .map(|(position, record)| (record.offset, position))
        .collect();
    let mut tree = Tree {
        parent: vec![None; records.len()],
    };
    if !directory.has_records {
        return tree;
    }
    let first = root_offset(directory, FIRST_ROOT_RECORD, &index, family);
    let last = root_offset(directory, LAST_ROOT_RECORD, &index, family);
    for record in records {
        check_record(record, &index, family);
    }
    let mut walk = Walk {
        records,
        index: &index,
        tree: &mut tree,
        family,
    };
    let root_last = walk.entities(first);
    if let (Some(expected), Some(found)) = (last, root_last) {
        if expected != records[found].offset {
            family.push(|| {
                format!(
                    "Offset of the Last Directory Record of the Root Directory Entity (0004,1202) \
                     is {expected}, but the Root Directory Entity's chain ends at the record at \
                     byte {} (PS3.3 F.3.2.2)",
                    records[found].offset
                )
            });
        }
    }
    report_unreachable(records, &tree, family);
    check_unique_keys(records, family);
    tree
}

/// PS3.3 F.5.1, F.5.2 and F.5.3: within a File-set, a Patient ID, Study
/// Instance UID or Series Instance UID appears in one record of its type.
fn check_unique_keys(records: &[Record], family: &mut Family) {
    for (record_type, tag, name, section) in [
        (
            "PATIENT",
            Tag(0x0010, 0x0020),
            "Patient ID (0010,0020)",
            "F.5.1",
        ),
        (
            "STUDY",
            Tag(0x0020, 0x000D),
            "Study Instance UID (0020,000D)",
            "F.5.2",
        ),
        (
            "SERIES",
            Tag(0x0020, 0x000E),
            "Series Instance UID (0020,000E)",
            "F.5.3",
        ),
    ] {
        let mut first: BTreeMap<String, u64> = BTreeMap::new();
        for record in records
            .iter()
            .filter(|record| record.record_type() == record_type)
        {
            let Some(value) = record.get(tag).map(|value| value.text()) else {
                continue;
            };
            if value.is_empty() {
                continue;
            }
            if let Some(earlier) = first.get(&value) {
                family.push(|| {
                    format!(
                        "{name} \"{}\" of the {record_type} record at byte {} is also that of \
                         the {record_type} record at byte {earlier}, but it shall have a unique \
                         Value in the File-set (PS3.3 {section})",
                        value.escape_default(),
                        record.offset
                    )
                });
            } else {
                first.insert(value, record.offset);
            }
        }
    }
}

fn check_consistency_flag(directory: &Directory, family: &mut Family) {
    match directory
        .data_set
        .get(&CONSISTENCY_FLAG)
        .map(|value| value.unsigned())
    {
        Some(Some(0)) => {}
        Some(Some(flag)) => family.push(|| {
            format!(
                "File-set Consistency Flag (0004,1212) is {flag:04X}H, but its only Enumerated \
                 Value is 0000H (PS3.3 F.3.2.2)"
            )
        }),
        Some(None) => family.push(|| {
            "File-set Consistency Flag (0004,1212) has no US Value (PS3.3 F.3.2.2)".to_string()
        }),
        None => family.push(|| {
            "File-set Consistency Flag (0004,1212), Type 1, is absent (PS3.3 F.3.2.2)".to_string()
        }),
    }
}

/// The offset `tag` holds, when it is non-zero and names a record; a zero
/// offset is an empty Root Directory Entity.
fn root_offset(
    directory: &Directory,
    tag: Tag,
    index: &BTreeMap<u64, usize>,
    family: &mut Family,
) -> Option<u64> {
    let name = offset_name(tag);
    let Some(value) = directory.data_set.get(&tag) else {
        family.push(|| format!("{name}, Type 1, is absent (PS3.3 F.3.2.2)"));
        return None;
    };
    let Some(offset) = value.unsigned() else {
        family.push(|| format!("{name} has no UL Value (PS3.3 F.3.2.2)"));
        return None;
    };
    let offset = u64::from(offset);
    if offset == 0 {
        return None;
    }
    if !index.contains_key(&offset) {
        family.push(|| not_an_item(&name, offset));
        return None;
    }
    Some(offset)
}

/// Checks one record's own offsets and its Record In-use Flag.
fn check_record(record: &Record, index: &BTreeMap<u64, usize>, family: &mut Family) {
    for tag in [NEXT_RECORD, LOWER_ENTITY] {
        let name = format!(
            "{} of the record at byte {}",
            offset_name(tag),
            record.offset
        );
        match record.get(tag).map(|value| value.unsigned()) {
            None => family.push(|| format!("{name}, Type 1, is absent (PS3.3 F.3.2.2)")),
            Some(None) => family.push(|| format!("{name} has no UL Value (PS3.3 F.3.2.2)")),
            Some(Some(0)) => {}
            Some(Some(offset)) if index.contains_key(&u64::from(offset)) => {}
            Some(Some(offset)) => family.push(|| not_an_item(&name, u64::from(offset))),
        }
    }
    if record.get(IN_USE_FLAG).and_then(|value| value.unsigned()) == Some(0) {
        family.push(|| {
            format!(
                "Record In-use Flag (0004,1410) of the record at byte {} is 0000H, a Value that \
                 shall never be present (PS3.3 F.3.2.2)",
                record.offset
            )
        });
    }
}

fn offset_name(tag: Tag) -> String {
    let name = match tag {
        FIRST_ROOT_RECORD => "Offset of the First Directory Record of the Root Directory Entity",
        LAST_ROOT_RECORD => "Offset of the Last Directory Record of the Root Directory Entity",
        NEXT_RECORD => "Offset of the Next Directory Record",
        _ => "Offset of Referenced Lower-Level Directory Entity",
    };
    format!("{name} ({:04X},{:04X})", tag.group(), tag.element())
}

fn not_an_item(name: &str, offset: u64) -> String {
    format!(
        "{name} is {offset}, which is not the first byte of an Item of the Directory Record \
         Sequence (PS3.3 F.3.2.2)"
    )
}

fn offset_of(record: &Record, tag: Tag) -> u64 {
    record
        .get(tag)
        .and_then(|value| value.unsigned())
        .map_or(0, u64::from)
}

struct Walk<'a> {
    records: &'a [Record],
    index: &'a BTreeMap<u64, usize>,
    tree: &'a mut Tree,
    family: &'a mut Family,
}

impl Walk<'_> {
    /// Walks the Root Directory Entity starting at `first` and every
    /// Directory Entity below it; returns the last record of the root chain.
    fn entities(&mut self, first: Option<u64>) -> Option<usize> {
        let mut pending = vec![(first?, None, 0usize)];
        let mut root_last = None;
        while let Some((start, parent, depth)) = pending.pop() {
            let last = self.chain(start, parent, depth, &mut pending);
            if parent.is_none() {
                root_last = last;
            }
        }
        root_last
    }

    /// Follows one Directory Entity's chain from `start`, queueing the
    /// lower-level Entities its records reference.
    fn chain(
        &mut self,
        start: u64,
        parent: Option<usize>,
        depth: usize,
        pending: &mut Vec<(u64, Option<usize>, usize)>,
    ) -> Option<usize> {
        let mut current = start;
        let mut last = None;
        while current != 0 {
            let Some(&position) = self.index.get(&current) else {
                break;
            };
            if self.tree.parent[position].is_some() {
                self.family.push(|| {
                    format!(
                        "the record at byte {current} is reached a second time: the Directory \
                         Entities form a cycle or share a record, but each record belongs to a \
                         single Directory Entity referenced by at most one higher-level record \
                         (PS3.3 F.2.1)"
                    )
                });
                break;
            }
            self.tree.parent[position] = Some(parent);
            let record = &self.records[position];
            self.check_parent(record, parent);
            let lower = offset_of(record, LOWER_ENTITY);
            if lower != 0 {
                self.descend(record, lower, position, depth, pending);
            }
            last = Some(position);
            current = offset_of(record, NEXT_RECORD);
        }
        last
    }

    fn descend(
        &mut self,
        record: &Record,
        lower: u64,
        position: usize,
        depth: usize,
        pending: &mut Vec<(u64, Option<usize>, usize)>,
    ) {
        if depth + 1 >= MAX_NESTING {
            self.family.push(|| {
                format!(
                    "the record at byte {} references a Directory Entity more than \
                     {MAX_NESTING} levels below the Root Directory Entity; it is not followed",
                    record.offset
                )
            });
            return;
        }
        pending.push((lower, Some(position), depth + 1));
    }

    /// Table F.4-1: whether the record's type may sit below its parent's.
    fn check_parent(&mut self, record: &Record, parent: Option<usize>) {
        let child = record.record_type();
        let parent_type = parent.map(|index| self.records[index].record_type());
        if allowed_below(parent_type.as_deref(), &child) {
            return;
        }
        let place = parent_type.map_or_else(
            || "the Root Directory Entity".to_string(),
            |kind| format!("a {kind} record"),
        );
        self.family.push(|| {
            format!(
                "the record at byte {} of Directory Record Type \"{child}\" sits below {place}, \
                 which Table F.4-1 does not allow (PS3.3 F.4)",
                record.offset
            )
        });
    }
}

/// PS3.3 Table F.4-1, from the generated Basic Directory tables. A parent
/// whose own Directory Record Type is not in Table F.4-1 admits any child:
/// the IOD checks report its type, and nothing is known of its children.
fn allowed_below(parent: Option<&str>, child: &str) -> bool {
    let Some(parent) = parent else {
        return root_directory_records().contains(&child);
    };
    let Some(record) = directory_record(parent) else {
        return true;
    };
    record.admits_lower_level(child)
        || record.any_private_lower_level
        || (parent == "SERIES" && child == SURFACE_SCAN)
}

fn report_unreachable(records: &[Record], tree: &Tree, family: &mut Family) {
    for (record, parent) in records.iter().zip(&tree.parent) {
        if parent.is_none() {
            family.push(|| {
                format!(
                    "the record at byte {} (Directory Record Type \"{}\") is not reached from \
                     the Root Directory Entity, so it belongs to no Directory Entity (PS3.3 \
                     F.2.1, F.3.2.2)",
                    record.offset,
                    record.record_type()
                )
            });
        }
    }
}
