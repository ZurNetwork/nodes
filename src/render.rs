//! Plain-text renderings of nodes and trees.

use std::fmt::Write as _;

use crate::repo::LoadedNode;
use crate::schema::{Category, Field, FsEntry, Node, NodeType, Ref};
use crate::tree::NodeTree;

/// The whole node, every field labelled.
pub fn node_text(node: &Node) -> String {
    let mut text = format!("{}  (charted {})\n", node.path, node.charted);
    let _ = writeln!(text, "short: {}", node.short);
    let _ = writeln!(text, "is: {}", node.is);
    let _ = writeln!(text, "type: {}", node.node_type);
    let ranked_categories = category_words(node).join(", ");
    let _ = writeln!(text, "categories: {ranked_categories}");
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
        Field::Categories => category_words(node),
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

/// Both closed vocabularies: a heading each, then `word  meaning`, one line per term.
pub fn vocabulary_text() -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "type — what a directory broadly holds: the one kind that fits it best"
    );
    for node_type in NodeType::ALL {
        let _ = writeln!(text, "  {node_type}  {}", node_type.meaning());
    }
    let _ = writeln!(
        text,
        "\ncategories — what a directory specifically holds, ranked from most to least fitting"
    );
    for category in Category::ALL {
        let _ = writeln!(text, "  {category}  {}", category.meaning());
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
/// a mounted tree is tagged `[mount]`.
pub fn tree_text(tree: &NodeTree) -> String {
    let mut text = String::new();
    let Some(root) = tree.root() else {
        return text;
    };
    let _ = writeln!(text, "{}  {}", root.location, root.node.short);
    push_children(&mut text, tree, root, "");
    text
}

fn push_children(text: &mut String, tree: &NodeTree, parent: &LoadedNode, prefix: &str) {
    let children = tree.children(&parent.location);
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
            "{prefix}{connector}{name}{mount_tag}  {}",
            child.node.short
        );
        let deeper = if last { "    " } else { "│   " };
        let child_prefix = format!("{prefix}{deeper}");
        push_children(text, tree, child, &child_prefix);
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

/// The node's categories as words, best fit first.
fn category_words(node: &Node) -> Vec<String> {
    node.categories
        .ranked()
        .iter()
        .map(ToString::to_string)
        .collect()
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
