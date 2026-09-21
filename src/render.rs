//! Plain-text renderings of nodes and trees.

use std::fmt::Write as _;

use crate::repo::LoadedNode;
use crate::schema::{Field, FsEntry, Node, NodeType, Ref};
use crate::tree::{NodeTree, Pruned};

/// The whole node, every field labelled.
pub fn node_text(node: &Node) -> String {
    let mut text = format!("{}  (charted {})\n", node.path, node.charted);
    let _ = writeln!(text, "short: {}", node.short);
    let _ = writeln!(text, "is: {}", node.is);
    let _ = writeln!(text, "type: {}", node.node_type);
    let authored_tags = tag_words(node).join(", ");
    let _ = writeln!(text, "tags: {authored_tags}");
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
        Field::Short => vec![node.short.clone()],
        Field::Is => vec![node.is.clone()],
        Field::Type => vec![node.node_type.to_string()],
        Field::Tags => tag_words(node),
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

/// The closed vocabulary: a heading, then `word  meaning`, one line per term.
pub fn vocabulary_text() -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "type — what a directory broadly holds: the one kind that fits it best"
    );
    for node_type in NodeType::ALL {
        let _ = writeln!(text, "  {node_type}  {}", node_type.meaning());
    }
    text
}

/// `path  short`, one line per node.
pub fn ls_text(nodes: &[&LoadedNode]) -> String {
    let mut text = String::new();
    for node in nodes {
        let _ = writeln!(text, "{}  {}", node.location, node.node.short);
    }
    text
}

/// The tree from the root, drawn with box connectors; each line names the node relative to its
/// parent node (so a node two directories down reads `api/v1`) and gives its `short`. The root of
/// a mounted tree is tagged `[mount]`. Only what `pruned` draws appears, and an ancestor that
/// merely leads to a match is drawn bare — no `short` — so the matches stand out.
pub fn tree_text(tree: &NodeTree, pruned: &Pruned) -> String {
    let mut text = String::new();
    let drawn_root = tree.root().filter(|root| pruned.draws(&root.location));
    let Some(root) = drawn_root else {
        return text;
    };
    let _ = writeln!(text, "{}{}", root.location, short_tail(root, pruned));
    push_children(&mut text, tree, pruned, root, "");
    text
}

fn push_children(
    text: &mut String,
    tree: &NodeTree,
    pruned: &Pruned,
    parent: &LoadedNode,
    prefix: &str,
) {
    let children: Vec<&LoadedNode> = tree
        .children(&parent.location)
        .into_iter()
        .filter(|child| pruned.draws(&child.location))
        .collect();
    let count = children.len();
    for (index, child) in children.into_iter().enumerate() {
        let last = index + 1 == count;
        let connector = if last { "└── " } else { "├── " };
        let name = parent
            .location
            .relative(&child.location)
            .unwrap_or(child.location.as_str());
        let mount_tag = if child.is_mount { "  [mount]" } else { "" };
        let _ = writeln!(
            text,
            "{prefix}{connector}{name}{mount_tag}{}",
            short_tail(child, pruned)
        );
        let deeper = if last { "    " } else { "│   " };
        let child_prefix = format!("{prefix}{deeper}");
        push_children(text, tree, pruned, child, &child_prefix);
    }
}

/// What follows a node's name on its line: its `short` for a match, nothing for a bare ancestor.
fn short_tail(node: &LoadedNode, pruned: &Pruned) -> String {
    if pruned.matches(&node.location) {
        format!("  {}", node.node.short)
    } else {
        String::new()
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

/// The node's tags as words, in the author's order.
fn tag_words(node: &Node) -> Vec<String> {
    node.tags
        .authored()
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// What follows a name that carries its own `NODE.json`.
pub const NODE_MARKER: &str = "  [node]";

fn fs_line(entry: &FsEntry) -> String {
    let marker = if entry.node { NODE_MARKER } else { "" };
    format!("{}  {}{marker}", entry.name, entry.role)
}

fn ref_line(reference: &Ref) -> String {
    format!(
        "{}  {} — {}",
        reference.page, reference.title, reference.governs
    )
}
