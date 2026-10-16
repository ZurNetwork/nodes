//! A tree's refs: one committed `NODE.refs.json` at its root, every node's citations in it.
//!
//! Most of a node is derived from the directory it charts, so a local-only tree can keep its files
//! out of version control and re-chart them. Its refs are authored — which design page governs
//! what — so they live here, committed whatever the tree does with its node files: versioned,
//! reviewed with the code, and checkable (`nodes check --ref-index`) in a checkout that holds no
//! node tree at all, such as a local-only tree's CI.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::schema::{NodePath, Ref};

/// Every node's refs, keyed by the node's path. A path that cites nothing has no entry; a tree that
/// cites nothing has no file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TreeRefs(BTreeMap<NodePath, Vec<Ref>>);

/// Hand-written for one rule a derived map lacks: a path listed twice is refused, never collapsed
/// into its last entry. "Keep both sides" of a merge conflict leaves exactly that, and a silent
/// collapse would drop refs while `check` passed.
impl<'de> Deserialize<'de> for TreeRefs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(TreeRefsVisitor)
    }
}

struct TreeRefsVisitor;

impl<'de> Visitor<'de> for TreeRefsVisitor {
    type Value = TreeRefs;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object of node path → refs")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<TreeRefs, A::Error> {
        let mut paths: BTreeMap<NodePath, Vec<Ref>> = BTreeMap::new();
        while let Some((path, refs)) = entries.next_entry::<NodePath, Vec<Ref>>()? {
            if paths.contains_key(&path) {
                let listed_twice = de::Error::custom(format!(
                    "`{path}` is listed more than once; merge its refs into one entry"
                ));
                return Err(listed_twice);
            }
            paths.insert(path, refs);
        }
        Ok(TreeRefs(paths))
    }
}

impl TreeRefs {
    /// Parses a `NODE.refs.json`: an object of node path → refs, each ref `{page, title, governs}`
    /// and nothing else.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The refs of the node at `path`; none when it cites nothing.
    pub fn of(&self, path: &NodePath) -> &[Ref] {
        self.0.get(path).map_or(&[], Vec::as_slice)
    }

    /// Replaces the refs of the node at `path`, kept in canonical order (by `page`); with none,
    /// the entry goes.
    pub fn set(&mut self, path: NodePath, mut refs: Vec<Ref>) {
        if refs.is_empty() {
            self.0.remove(&path);
            return;
        }
        refs.sort_by_key(|reference| reference.page);
        self.0.insert(path, refs);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many refs the tree holds, across every path.
    pub fn count(&self) -> usize {
        self.0.values().map(Vec::len).sum()
    }

    /// Every path with its refs, in tree order.
    pub fn iter(&self) -> impl Iterator<Item = (&NodePath, &[Ref])> {
        self.0.iter().map(|(path, refs)| (path, refs.as_slice()))
    }

    /// The canonical file text: paths in tree order, each path's refs by `page`, two-space indent,
    /// one trailing newline.
    pub fn canonical_json(&self) -> String {
        let mut canonical = self.clone();
        for refs in canonical.0.values_mut() {
            refs.sort_by_key(|reference| reference.page);
        }
        let mut text =
            serde_json::to_string_pretty(&canonical).expect("a refs file always serializes");
        text.push('\n');
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::PageId;

    fn cited(page: u64, title: &str) -> Ref {
        Ref {
            page: PageId::from(page),
            title: title.to_owned(),
            governs: "here".to_owned(),
        }
    }

    #[test]
    fn the_canonical_file_orders_paths_by_tree_and_refs_by_page() {
        let mut all = TreeRefs::default();
        let backend_dash: NodePath = "backend-x".parse().expect("valid");
        let backend_crates: NodePath = "backend/crates".parse().expect("valid");
        all.set(backend_dash, vec![cited(1, "One")]);
        all.set(
            backend_crates.clone(),
            vec![cited(9, "Nine"), cited(2, "Two")],
        );
        let text = all.canonical_json();
        let crates_at = text.find("backend/crates").expect("listed");
        let dash_at = text.find("backend-x").expect("listed");
        assert!(
            crates_at < dash_at,
            "a directory's subtree sorts before its siblings: {text}"
        );
        assert!(text.find("\"Two\"") < text.find("\"Nine\""), "{text}");
        let reread = TreeRefs::parse(&text).expect("canonical text parses");
        assert_eq!(reread.canonical_json(), text, "a fixed point");
        assert_eq!(reread.count(), 3);
        all.set(backend_crates.clone(), Vec::new());
        assert_eq!(all.of(&backend_crates), [], "no refs, no entry");
    }

    #[test]
    fn a_malformed_refs_file_is_refused() {
        let unknown_key = r#"{"src": [{"page": 1, "title": "t", "governs": "g", "extra": 1}]}"#;
        assert!(TreeRefs::parse(unknown_key).is_err());
        let bad_path = r#"{"/abs": []}"#;
        assert!(TreeRefs::parse(bad_path).is_err());
        let listed_twice = r#"{
  "backend": [{"page": 1, "title": "One", "governs": "a"}],
  "backend": [{"page": 2, "title": "Two", "governs": "b"}]
}"#;
        let listed_twice_error = TreeRefs::parse(listed_twice).expect_err("never collapsed");
        assert!(
            listed_twice_error
                .to_string()
                .contains("`backend` is listed more than once"),
            "{listed_twice_error}"
        );
    }
}
