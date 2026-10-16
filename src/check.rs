//! `nodes check`: every rule a charted tree must satisfy, reported as findings.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Error;
use crate::refindex::RefIndex;
use crate::repo::{Era, LoadedNode, Repo};
use crate::schema::{NodePath, PageId, REFS_FILE, Ref};
use crate::tree::NodeTree;
use crate::treerefs::TreeRefs;

/// How bad a finding is: errors fail the check, warnings do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One rule violation at one node.
#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub path: NodePath,
    pub message: String,
}

/// The outcome of a check run.
#[derive(Debug, Serialize)]
pub struct Report {
    pub nodes: usize,
    /// The trees mounted beneath this one; only their root nodes were read.
    pub mounts: usize,
    /// The refs checked, wherever the tree keeps them.
    pub refs: usize,
    /// Whether a node tree was here. Without one — a local-only tree's fresh clone, CI — only
    /// the tree's `NODE.refs.json` was checked.
    pub tree: bool,
    pub errors: usize,
    pub warnings: usize,
    pub findings: Vec<Finding>,
}

/// What `check` resolves refs against, when anything.
#[derive(Debug, Default)]
pub struct CheckOptions {
    /// A text file listing the citable page ids; without it the ref rules are skipped.
    pub ref_index: Option<PathBuf>,
    /// The word that marks an index entry superseded.
    pub superseded_marker: String,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.errors == 0
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => f.write_str("error"),
            Self::Warning => f.write_str("warning"),
        }
    }
}

/// Runs every rule over the repository's own nodes. A mounted tree is another tree's business:
/// only its root node is read, and its refs are never held against this tree's index. A checkout
/// that holds the tree's `NODE.refs.json` but no nodes — a local-only tree's node files are not
/// committed — gets its refs checked alone.
pub fn check(repo: &Repo, options: &CheckOptions) -> Result<Report, Error> {
    repo.note_if_unmigrated();
    let ref_index = options
        .ref_index
        .as_deref()
        .map(|file| RefIndex::load(file, &options.superseded_marker))
        .transpose()?;
    let mut findings: Vec<Finding> = Vec::new();
    let refs = match repo.era() {
        Era::Current => repo.load_refs().unwrap_or_else(|unreadable| {
            findings.push(error_at(&NodePath::root(), unreadable.to_string()));
            TreeRefs::default()
        }),
        Era::Legacy => TreeRefs::default(),
    };
    let found = repo.walk()?;
    let holds_refs_alone =
        found.node_files.is_empty() && repo.era() == Era::Current && repo.refs_file().is_file();
    if holds_refs_alone {
        let report = check_refs_alone(repo, &refs, ref_index.as_ref(), findings);
        return Ok(report);
    }
    let reached: BTreeSet<NodePath> = found
        .node_files
        .iter()
        .chain(&found.mounts)
        .map(|reached| repo.location_of(reached))
        .collect::<Result<_, _>>()?;
    let (loaded, unreadable) = repo.read_lenient(&found.node_files, &refs)?;
    let broken = unreadable.into_iter().map(|broken| Finding {
        severity: Severity::Error,
        path: broken.location,
        message: broken.error.to_string(),
    });
    findings.extend(broken);
    let tree = NodeTree::from_nodes(loaded);
    if tree.root().is_none() {
        let message = format!(
            "no root node: a {} whose path is \".\" must sit at the root",
            repo.era().node_file()
        );
        findings.push(error_at(&NodePath::root(), message));
    }
    for node in tree.iter() {
        check_path(node, &mut findings);
        check_fs_children(repo, &reached, node, &mut findings);
        check_listed_by_ancestor(&tree, &node.location, &mut findings);
        check_refs(
            &node.location,
            &node.node.refs,
            ref_index.as_ref(),
            &mut findings,
        );
    }
    for (path, cited) in refs.iter() {
        if tree.get(path).is_some() {
            continue;
        }
        check_refs_path(repo, path, &mut findings);
        check_refs(path, cited, ref_index.as_ref(), &mut findings);
    }
    for mount_dir in &found.mounts {
        check_mount(repo, &tree, mount_dir, &mut findings)?;
    }
    let cited = tree.iter().map(|node| node.node.refs.len()).sum::<usize>()
        + refs
            .iter()
            .filter(|(path, _)| tree.get(path).is_none())
            .map(|(_, cited)| cited.len())
            .sum::<usize>();
    let report = report(tree.len(), found.mounts.len(), cited, true, findings);
    Ok(report)
}

/// What a checkout with no node tree can check: its `NODE.refs.json` — every path a directory
/// here, every page cited once per path, and, given an index, every page in it.
fn check_refs_alone(
    repo: &Repo,
    refs: &TreeRefs,
    index: Option<&RefIndex>,
    mut findings: Vec<Finding>,
) -> Report {
    for (path, cited) in refs.iter() {
        if !path.to_dir(repo.root()).is_dir() {
            let message =
                format!("{REFS_FILE} cites pages for `{path}`, which is not a directory here");
            findings.push(error_at(path, message));
        }
        check_refs(path, cited, index, &mut findings);
    }
    report(0, 0, refs.count(), false, findings)
}

fn report(nodes: usize, mounts: usize, refs: usize, tree: bool, findings: Vec<Finding>) -> Report {
    let errors = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    Report {
        nodes,
        mounts,
        refs,
        tree,
        errors,
        warnings: findings.len() - errors,
        findings,
    }
}

/// Refs kept for a path no node charts: an error when the path is no directory here, a warning
/// otherwise (chart the directory, or move the refs to the node that should carry them).
fn check_refs_path(repo: &Repo, path: &NodePath, findings: &mut Vec<Finding>) {
    let finding = if path.to_dir(repo.root()).is_dir() {
        let message = format!("{REFS_FILE} cites pages for `{path}`, where no node is charted");
        warning_at(path, message)
    } else {
        let message =
            format!("{REFS_FILE} cites pages for `{path}`, which is not a directory here");
        error_at(path, message)
    };
    findings.push(finding);
}

/// `path` says where the file sits.
fn check_path(node: &LoadedNode, findings: &mut Vec<Finding>) {
    if node.node.path == node.location {
        return;
    }
    let message = format!(
        "path is `{}` but the file sits at `{}`",
        node.node.path, node.location
    );
    findings.push(error(node, message));
}

/// Every `fs` entry flagged `node: true` has a `NODE.inf.json` at that path beneath this node — a nested
/// name such as `src/account/` is allowed, so a pass-through directory need not carry a node of its
/// own — and the walk reaches it: a node that is hidden, ignored or inside a mount is no node of
/// this tree, whatever `fs` says.
fn check_fs_children(
    repo: &Repo,
    reached: &BTreeSet<NodePath>,
    node: &LoadedNode,
    findings: &mut Vec<Finding>,
) {
    for entry in node.node.fs.iter().filter(|entry| entry.node) {
        let Ok(child) = node.location.join(&entry.name) else {
            let message = format!(
                "fs entry `{}` says node: true but is not a directory path",
                entry.name
            );
            findings.push(error(node, message));
            continue;
        };
        if reached.contains(&child) {
            continue;
        }
        let mount_above = repo.mount_holding(&child).filter(|mount| *mount != child);
        let child_dir = child.to_dir(repo.root());
        let left_behind = repo.era() == Era::Current && Repo::is_unmigrated_dir(&child_dir);
        let why_not = if left_behind {
            format!(
                "`{child}` holds a NODE.json from before v0.7 — run `nodes migrate` (in a mounted tree, from its root)"
            )
        } else if !repo.node_file(&child).is_file() {
            format!("`{child}` has no {}", repo.era().node_file())
        } else if let Some(mount) = mount_above {
            format!("`{child}` belongs to the tree mounted at `{mount}`")
        } else {
            format!(
                "`{child}` is hidden or ignored — re-include it with a `!` line in .chartignore, or say node: false"
            )
        };
        let message = format!("fs entry `{}` says node: true but {why_not}", entry.name);
        findings.push(error(node, message));
    }
}

/// A mount's root node fits the schema, and its parent lists it like any other child node. Nothing
/// beneath the mount's root is read.
fn check_mount(
    repo: &Repo,
    tree: &NodeTree,
    mount_dir: &Path,
    findings: &mut Vec<Finding>,
) -> Result<(), Error> {
    let mount_point = repo.location_of(mount_dir)?;
    let mounted = repo.mounted_tree(&mount_point);
    if Repo::holds_root_node(mount_dir) {
        let root_file = mounted.node_file(&NodePath::root());
        if let Err(unreadable) = mounted.read(&root_file) {
            let finding = error_at(&mount_point, unreadable.to_string());
            findings.push(finding);
        }
    } else if !Repo::is_checkout_top(mount_dir) {
        // Refs alone are a tree of their own only in a checkout of their own — a fresh clone
        // mounted here. Anywhere else they split this tree.
        let message = format!(
            "holds a {REFS_FILE} but no root node, so it splits this tree in two: move its refs into the {REFS_FILE} at the root"
        );
        findings.push(error_at(&mount_point, message));
    }
    check_listed_by_ancestor(tree, &mount_point, findings);
    Ok(())
}

/// A node — or a mount — directly beneath another node is listed in that node's `fs` with `node: true`.
fn check_listed_by_ancestor(tree: &NodeTree, location: &NodePath, findings: &mut Vec<Finding>) {
    let Some(ancestor) = tree.nearest_ancestor(location) else {
        return;
    };
    let Some(below) = ancestor.location.relative(location) else {
        return;
    };
    let is_direct_child = !below.contains('/');
    if !is_direct_child {
        return;
    }
    let listing = ancestor
        .node
        .fs
        .iter()
        .find(|entry| entry.name.trim_end_matches('/') == below);
    let message = match listing {
        Some(entry) if entry.node => return,
        Some(_) => format!(
            "listed in `{}`'s fs with node: false, but it has a node of its own",
            ancestor.location
        ),
        None => format!("not listed in `{}`'s fs", ancestor.location),
    };
    findings.push(error_at(location, message));
}

/// The refs of `path` cite each page once, and — given an index — only pages it lists; a
/// superseded entry warns.
fn check_refs(
    path: &NodePath,
    refs: &[Ref],
    index: Option<&RefIndex>,
    findings: &mut Vec<Finding>,
) {
    let mut seen: Vec<PageId> = Vec::new();
    for reference in refs {
        if seen.contains(&reference.page) {
            let message = format!("page {} is cited more than once", reference.page);
            findings.push(error_at(path, message));
        }
        seen.push(reference.page);
        let Some(index) = index else {
            continue;
        };
        match index.get(reference.page) {
            None => {
                let message = format!(
                    "ref {} ({}) is not in the ref index",
                    reference.page, reference.title
                );
                findings.push(error_at(path, message));
            }
            Some(entry) if entry.superseded => {
                let message = format!(
                    "ref {} ({}) is marked superseded in the ref index",
                    reference.page, reference.title
                );
                findings.push(warning_at(path, message));
            }
            Some(_) => {}
        }
    }
}

fn error(node: &LoadedNode, message: String) -> Finding {
    error_at(&node.location, message)
}

fn error_at(path: &NodePath, message: String) -> Finding {
    Finding {
        severity: Severity::Error,
        path: path.clone(),
        message,
    }
}

fn warning_at(path: &NodePath, message: String) -> Finding {
    Finding {
        severity: Severity::Warning,
        path: path.clone(),
        message,
    }
}
