//! PS3.5 7.5.3: which Sequences of the IODs (the pathology-slide IODs and the
//! Basic Directory IOD) define Specific Character Set (0008,0005) for their
//! Items.

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use dicom_core::{Tag, VR};

use super::engine::{dynamic_tables, flatten_level, Definition};
use super::model::{Node, Table};
use super::scope::raw_tag;
use super::tables::{BASIC_DIRECTORY, IODS};
use super::values::entry;
use super::MAX_NESTING;

const SPECIFIC_CHARACTER_SET: u32 = 0x0008_0005;

/// Whether an Item of `sequence` may include Specific Character Set: `None`
/// when no table of the IODs defines the Sequence. A Sequence whose Items the
/// tables leave open (e.g. Modified Attributes Sequence), or that any of the
/// IODs lets define it, admits it; so does one whose Items include a table
/// that defines it on their Values (the Record Selection Keys of a Directory
/// Record).
pub(in crate::validation) fn item_admits_specific_character_set(sequence: Tag) -> Option<bool> {
    static ADMITS: OnceLock<BTreeMap<u32, bool>> = OnceLock::new();
    ADMITS
        .get_or_init(|| {
            let mut admits = BTreeMap::new();
            let mut visited = HashSet::new();
            let tables = IODS
                .iter()
                .chain(std::iter::once(&BASIC_DIRECTORY))
                .flat_map(|iod| {
                    iod.modules
                        .iter()
                        .map(|module| module.table)
                        .chain(iod.functional_groups.iter().map(|group| group.table))
                });
            for table in tables {
                collect(table.nodes, table, &mut admits, &mut visited, 0);
            }
            admits
        })
        .get(&raw_tag(sequence))
        .copied()
}

/// The rows of one nesting level, the rows of the tables its Data Sets may
/// include by their own Values among them.
fn level_rows(nodes: &'static [Node], table: &'static Table) -> Vec<Definition> {
    let mut rows = Vec::new();
    let mut dynamic = Vec::new();
    flatten_level(nodes, table, &mut rows, &mut dynamic);
    let mut seen = HashSet::new();
    while let Some(next) = dynamic.pop() {
        for included in dynamic_tables(next) {
            if seen.insert(std::ptr::from_ref(included) as usize) {
                flatten_level(included.nodes, included, &mut rows, &mut dynamic);
            }
        }
    }
    rows
}

fn collect(
    nodes: &'static [Node],
    table: &'static Table,
    admits: &mut BTreeMap<u32, bool>,
    visited: &mut HashSet<usize>,
    depth: usize,
) {
    // Rows are static: a level reached again (a recursive Content Sequence)
    // adds nothing.
    if !visited.insert(nodes.as_ptr() as usize) {
        return;
    }
    for row in level_rows(nodes, table) {
        let attribute = row.attribute;
        let is_sequence = entry(super::scope::tag_of(attribute.tag))
            .is_some_and(|entry| entry.vr.contains(&VR::SQ));
        if !is_sequence {
            continue;
        }
        let open = attribute.children.is_empty();
        let defines = level_rows(attribute.children, row.table)
            .iter()
            .any(|child| child.attribute.tag == SPECIFIC_CHARACTER_SET);
        *admits.entry(attribute.tag).or_insert(false) |= open || defines;
        if depth < MAX_NESTING {
            collect(attribute.children, row.table, admits, visited, depth + 1);
        }
    }
}
