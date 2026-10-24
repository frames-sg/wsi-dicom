//! The Content Tree of a document (PS3.3 C.17.3): the root Content Item, the
//! Data Set itself, and every Content Item its Content Sequences nest, read
//! once in breadth-first order and bounded in depth and count. A by-reference
//! Content Item's Referenced Content Item Identifier (0040,DB73) is resolved
//! along the by-value relationship path it names (Table C.17-6).

use std::fmt;
use std::ops::Range;

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::Findings;
use crate::validation::iod::{items as sequence_items, string_value, MAX_NESTING};

/// Content Sequence nesting walked below the root.
pub(super) const MAX_DEPTH: usize = MAX_NESTING;
/// Content Items read; a larger tree is examined in part.
pub(super) const MAX_ITEMS: usize = 200_000;

pub(super) struct Node<'a> {
    pub(super) object: &'a InMemDicomObject,
    pub(super) parent: Option<usize>,
    /// 1-based position in the parent's Content Sequence; 1 for the root.
    pub(super) position: u32,
    pub(super) depth: usize,
    /// The Items of its Content Sequence, contiguous in breadth-first order.
    pub(super) children: Range<usize>,
    pub(super) value_type: Option<String>,
    pub(super) relationship: Option<String>,
    /// The Values of Referenced Content Item Identifier, for a by-reference
    /// Content Item.
    pub(super) reference: Option<Vec<i64>>,
}

pub(super) struct Tree<'a> {
    pub(super) nodes: Vec<Node<'a>>,
    /// Content Items not read: below `MAX_DEPTH` or beyond `MAX_ITEMS`.
    pub(super) unread: usize,
}

impl<'a> Node<'a> {
    fn new(
        object: &'a InMemDicomObject,
        parent: Option<usize>,
        position: u32,
        depth: usize,
    ) -> Self {
        let reference = object
            .get(tags::REFERENCED_CONTENT_ITEM_IDENTIFIER)
            .map(|element| element.to_multi_int::<i64>().unwrap_or_default());
        Self {
            object,
            parent,
            position,
            depth,
            children: 0..0,
            value_type: string_value(object, tags::VALUE_TYPE, 1),
            relationship: string_value(object, tags::RELATIONSHIP_TYPE, 1),
            reference,
        }
    }

    pub(super) fn value_type(&self) -> Option<&str> {
        self.value_type.as_deref()
    }

    pub(super) fn relationship(&self) -> Option<&str> {
        self.relationship.as_deref()
    }

    pub(super) fn is_by_reference(&self) -> bool {
        self.reference.is_some()
    }
}

impl<'a> Tree<'a> {
    pub(super) fn of(root: &'a InMemDicomObject) -> Self {
        let mut nodes = vec![Node::new(root, None, 1, 0)];
        let mut unread = 0;
        let mut next = 0;
        while next < nodes.len() {
            let (object, depth) = (nodes[next].object, nodes[next].depth);
            let items = sequence_items(object, tags::CONTENT_SEQUENCE);
            let start = nodes.len();
            if depth >= MAX_DEPTH || start + items.len() > MAX_ITEMS {
                unread += items.len();
            } else {
                for (index, item) in items.iter().enumerate() {
                    let position = u32::try_from(index + 1).unwrap_or(u32::MAX);
                    nodes.push(Node::new(item, Some(next), position, depth + 1));
                }
            }
            nodes[next].children = start..nodes.len();
            next += 1;
        }
        Self { nodes, unread }
    }

    /// Says what of the tree the limits left unread.
    pub(super) fn note_limits(&self, findings: &mut Findings) {
        if self.unread > 0 {
            findings.unexamined(format!(
                "{} Content Item{} nested more than {MAX_DEPTH} levels deep or beyond the first {MAX_ITEMS}, and their descendants",
                self.unread,
                if self.unread == 1 { "" } else { "s" }
            ));
        }
    }

    pub(super) fn root(&self) -> &Node<'a> {
        &self.nodes[0]
    }

    /// The Content Item a by-reference Item's identifier names: the root for
    /// a single Value of 1, then each 1-based position along the by-value
    /// path. `None` when it names no Item of the tree as read.
    pub(super) fn resolve(&self, identifier: &[i64]) -> Option<usize> {
        let (first, rest) = identifier.split_first()?;
        if *first != 1 {
            return None;
        }
        let mut node = 0;
        for position in rest {
            let offset = usize::try_from(position.checked_sub(1)?).ok()?;
            let children = &self.nodes[node].children;
            let index = children.start.checked_add(offset)?;
            if index >= children.end {
                return None;
            }
            node = index;
        }
        Some(node)
    }

    /// The Content Item `index` stands for: itself, or the Target its
    /// Referenced Content Item Identifier names.
    pub(super) fn target(&self, index: usize) -> Option<usize> {
        match &self.nodes[index].reference {
            Some(identifier) => self.resolve(identifier),
            None => Some(index),
        }
    }

    /// The Value Type of the Content Item `index` stands for.
    pub(super) fn target_value_type(&self, index: usize) -> Option<&str> {
        self.target(index)
            .and_then(|target| self.nodes[target].value_type())
    }

    pub(super) fn children(&self, index: usize) -> impl Iterator<Item = usize> {
        self.nodes[index].children.clone()
    }

    /// Whether `ancestor` is `index` or one of its by-value ancestors.
    pub(super) fn is_ancestor(&self, ancestor: usize, index: usize) -> bool {
        let mut current = Some(index);
        while let Some(node) = current {
            if node == ancestor {
                return true;
            }
            current = self.nodes[node].parent;
        }
        false
    }

    /// A lazily formatted locator of Content Item `index`.
    pub(super) fn at(&self, index: usize) -> At<'_, 'a> {
        At { tree: self, index }
    }

    /// The Referenced Content Item Identifier that names Content Item
    /// `index`, e.g. "1.3.2".
    pub(super) fn identifier(&self, index: usize) -> String {
        let mut positions = Vec::new();
        let mut current = Some(index);
        while let Some(node) = current {
            positions.push(self.nodes[node].position.to_string());
            current = self.nodes[node].parent;
        }
        positions.reverse();
        positions.join(".")
    }
}

/// "Content Sequence[3] > Content Sequence[1]", or "the root Content Item".
#[derive(Clone, Copy)]
pub(super) struct At<'t, 'a> {
    tree: &'t Tree<'a>,
    index: usize,
}

impl fmt::Display for At<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.index == 0 {
            return formatter.write_str("the root Content Item");
        }
        let mut positions = Vec::new();
        let mut current = self.index;
        while let Some(parent) = self.tree.nodes[current].parent {
            positions.push(self.tree.nodes[current].position);
            current = parent;
        }
        let path = positions
            .iter()
            .rev()
            .map(|position| format!("Content Sequence[{position}]"))
            .collect::<Vec<_>>()
            .join(" > ");
        formatter.write_str(&path)
    }
}
