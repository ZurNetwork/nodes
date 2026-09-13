//! The loaded node tree: lookup by path, ancestor chains, children and citations.

use std::collections::BTreeMap;

use crate::repo::LoadedNode;
use crate::schema::{NodePath, PageId, Ref};

/// Every loaded node keyed by its location, in tree order (a directory before everything beneath it).
#[derive(Debug, Default)]
pub struct NodeTree {
    nodes: BTreeMap<NodePath, LoadedNode>,
}

impl NodeTree {
    pub fn from_nodes(nodes: Vec<LoadedNode>) -> Self {
        let keyed = nodes
            .into_iter()
            .map(|node| (node.location.clone(), node))
            .collect();
        Self { nodes: keyed }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn get(&self, path: &NodePath) -> Option<&LoadedNode> {
        self.nodes.get(path)
    }

    pub fn root(&self) -> Option<&LoadedNode> {
        self.get(&NodePath::root())
    }

    /// Every node, top down.
    pub fn iter(&self) -> impl Iterator<Item = &LoadedNode> {
        self.nodes.values()
    }

    /// Root → … → the nearest node at or above `target`: what a reader loads to understand a path.
    pub fn chain(&self, target: &NodePath) -> Vec<&LoadedNode> {
        self.iter()
            .filter(|node| node.location.relative(target).is_some())
            .collect()
    }

    /// The closest node strictly above `path`.
    pub fn nearest_ancestor(&self, path: &NodePath) -> Option<&LoadedNode> {
        self.iter()
            .filter(|node| node.location.is_ancestor_of(path))
            .last()
    }

    /// The nodes whose nearest ancestor is `parent`.
    pub fn children(&self, parent: &NodePath) -> Vec<&LoadedNode> {
        self.iter()
            .filter(|node| parent.is_ancestor_of(&node.location))
            .filter(|node| {
                self.nearest_ancestor(&node.location)
                    .is_some_and(|ancestor| ancestor.location == *parent)
            })
            .collect()
    }

    /// Every node citing `page`, with the ref that cites it.
    pub fn citing(&self, page: PageId) -> Vec<(&LoadedNode, &Ref)> {
        self.iter()
            .filter_map(|node| node.node.reference(page).map(|reference| (node, reference)))
            .collect()
    }
}
