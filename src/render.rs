//! Plain-text renderings of nodes and trees.

use std::fmt::Write as _;

use crate::repo::LoadedNode;
use crate::schema::{Field, FsEntry, Node, Ref};
use crate::tree::NodeTree;

/// The whole node, every field labelled.
pub fn node_text(node: &Node) -> String {
    let mut text = format!("{}  (charted {})\n", node.path, node.charted);
    let _ = writeln!(text, "is: {}", node.is);
    push_list(&mut text, "conventions", node.conventions.iter().cloned());
    push_list(&mut text, "entry_points", node.entry_points.iter().cloned());
    push_list(&mut text, "fs", node.fs.iter().map(fs_line));
    push_list(&mut text, "refs", node.refs.iter().map(ref_line));
    push_list(&mut text, "notes", node.notes.iter().cloned());
    text
}

/// One field, as bare lines.
pub fn field_text(node: &Node, field: Field) -> String {
    let lines: Vec<String> = match field {
        Field::Path => vec![node.path.to_string()],
        Field::Charted => vec![node.charted.to_string()],
        Field::Is => vec![node.is.clone()],
        Field::Conventions => node.conventions.clone(),
        Field::EntryPoints => node.entry_points.clone(),
        Field::Fs => node.fs.iter().map(fs_line).collect(),
        Field::Refs => node.refs.iter().map(ref_line).collect(),
        Field::Notes => node.notes.clone(),
    };
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

/// `path  is`, one line per node.
pub fn ls_text(tree: &NodeTree) -> String {
    let mut text = String::new();
    for node in tree.iter() {
        let _ = writeln!(text, "{}  {}", node.location, node.node.is);
    }
    text
}

/// The tree from the root, two spaces of indent per level: `path  is`.
pub fn tree_text(tree: &NodeTree) -> String {
    let mut text = String::new();
    if let Some(root) = tree.root() {
        push_subtree(&mut text, tree, root, 0);
    }
    text
}

fn push_subtree(text: &mut String, tree: &NodeTree, node: &LoadedNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let _ = writeln!(text, "{indent}{}  {}", node.location, node.node.is);
    for child in tree.children(&node.location) {
        push_subtree(text, tree, child, depth + 1);
    }
}

fn push_list(text: &mut String, label: &str, items: impl Iterator<Item = String>) {
    let mut any = false;
    let mut body = String::new();
    for item in items {
        any = true;
        let _ = writeln!(body, "  - {item}");
    }
    if any {
        let _ = writeln!(text, "{label}:");
        text.push_str(&body);
    } else {
        let _ = writeln!(text, "{label}: (none)");
    }
}

fn fs_line(entry: &FsEntry) -> String {
    let marker = if entry.node { "  [node]" } else { "" };
    format!("{}  {}{marker}", entry.name, entry.role)
}

fn ref_line(reference: &Ref) -> String {
    format!(
        "{}  {} — {}",
        reference.page, reference.title, reference.governs
    )
}
