//! PS3.5 7.5.3: which Sequences of the IOD define Specific Character Set
//! (0008,0005) for their Items.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use dicom_core::{Tag, VR};

use super::engine::{flatten, Definition};
use super::model::{Node, Table};
use super::scope::raw_tag;
use super::tables::{FUNCTIONAL_GROUPS, MODULES};
use super::values::entry;
use super::MAX_NESTING;

const SPECIFIC_CHARACTER_SET: u32 = 0x0008_0005;

/// Whether an Item of `sequence` may include Specific Character Set: `None`
/// when no table of the IOD defines the Sequence. A Sequence whose Items the
/// tables leave open (e.g. Modified Attributes Sequence) admits it.
pub(in crate::validation) fn item_admits_specific_character_set(sequence: Tag) -> Option<bool> {
    static ADMITS: OnceLock<BTreeMap<u32, bool>> = OnceLock::new();
    ADMITS
        .get_or_init(|| {
            let mut admits = BTreeMap::new();
            let tables = MODULES
                .iter()
                .map(|module| module.table)
                .chain(FUNCTIONAL_GROUPS.iter().map(|group| group.table));
            for table in tables {
                collect(table.nodes, table, &mut admits, 0);
            }
            admits
        })
        .get(&raw_tag(sequence))
        .copied()
}

fn collect(
    nodes: &'static [Node],
    table: &'static Table,
    admits: &mut BTreeMap<u32, bool>,
    depth: usize,
) {
    let mut rows: Vec<Definition> = Vec::new();
    flatten(nodes, table, &mut rows);
    for row in rows {
        let attribute = row.attribute;
        let is_sequence = entry(super::scope::tag_of(attribute.tag))
            .is_some_and(|entry| entry.vr.contains(&VR::SQ));
        if !is_sequence {
            continue;
        }
        let open = attribute.children.is_empty();
        let mut children = Vec::new();
        flatten(attribute.children, row.table, &mut children);
        let defines = children
            .iter()
            .any(|child| child.attribute.tag == SPECIFIC_CHARACTER_SET);
        *admits.entry(attribute.tag).or_insert(false) |= open || defines;
        if depth < MAX_NESTING {
            collect(attribute.children, row.table, admits, depth + 1);
        }
    }
}
