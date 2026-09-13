//! `nodes check`: every rule a charted tree must satisfy, reported as findings.

use std::path::PathBuf;

use serde::Serialize;

use crate::error::Error;
use crate::refindex::RefIndex;
use crate::repo::{LoadedNode, Repo};
use crate::schema::{NodePath, PageId};
use crate::tree::NodeTree;

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

/// Runs every rule over the repository.
pub fn check(repo: &Repo, options: &CheckOptions) -> Result<Report, Error> {
    let (loaded, unreadable) = repo.read_all_lenient()?;
    let mut findings: Vec<Finding> = unreadable
        .into_iter()
        .map(|broken| Finding {
            severity: Severity::Error,
            path: broken.location,
            message: broken.error.to_string(),
        })
        .collect();
    let tree = NodeTree::from_nodes(loaded);
    if tree.root().is_none() {
        findings.push(Finding {
            severity: Severity::Error,
            path: NodePath::root(),
            message: "no root node: a NODE.json whose path is \".\" must sit at the root"
                .to_owned(),
        });
    }
    let ref_index = options
        .ref_index
        .as_deref()
        .map(|file| RefIndex::load(file, &options.superseded_marker))
        .transpose()?;
    for node in tree.iter() {
        check_path(node, &mut findings);
        check_fs_children(repo, node, &mut findings);
        check_listed_by_ancestor(&tree, node, &mut findings);
        check_refs(node, ref_index.as_ref(), &mut findings);
    }
    let errors = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    let report = Report {
        nodes: tree.len(),
        errors,
        warnings: findings.len() - errors,
        findings,
    };
    Ok(report)
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

/// Every `fs` entry flagged `node: true` has a `NODE.json` at that path beneath this node — a nested
/// name such as `src/account/` is allowed, so a pass-through directory need not carry a node of its own.
fn check_fs_children(repo: &Repo, node: &LoadedNode, findings: &mut Vec<Finding>) {
    for entry in node.node.fs.iter().filter(|entry| entry.node) {
        let Ok(child) = node.location.join(&entry.name) else {
            let message = format!(
                "fs entry `{}` says node: true but is not a directory path",
                entry.name
            );
            findings.push(error(node, message));
            continue;
        };
        if repo.node_file(&child).is_file() {
            continue;
        }
        let message = format!(
            "fs entry `{}` says node: true but `{}` has no NODE.json",
            entry.name, child
        );
        findings.push(error(node, message));
    }
}

/// A node directly beneath another node is listed in that node's `fs` with `node: true`.
fn check_listed_by_ancestor(tree: &NodeTree, node: &LoadedNode, findings: &mut Vec<Finding>) {
    let Some(ancestor) = tree.nearest_ancestor(&node.location) else {
        return;
    };
    let Some(below) = ancestor.location.relative(&node.location) else {
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
            "listed in `{}`'s fs with node: false, but it has a NODE.json",
            ancestor.location
        ),
        None => format!("not listed in `{}`'s fs", ancestor.location),
    };
    findings.push(error(node, message));
}

/// Refs cite each page once, and — given an index — only pages it lists; a superseded entry warns.
fn check_refs(node: &LoadedNode, index: Option<&RefIndex>, findings: &mut Vec<Finding>) {
    let mut seen: Vec<PageId> = Vec::new();
    for reference in &node.node.refs {
        if seen.contains(&reference.page) {
            let message = format!("page {} is cited more than once", reference.page);
            findings.push(error(node, message));
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
                findings.push(error(node, message));
            }
            Some(entry) if entry.superseded => {
                let message = format!(
                    "ref {} ({}) is marked superseded in the ref index",
                    reference.page, reference.title
                );
                findings.push(warning(node, message));
            }
            Some(_) => {}
        }
    }
}

fn error(node: &LoadedNode, message: String) -> Finding {
    Finding {
        severity: Severity::Error,
        path: node.location.clone(),
        message,
    }
}

fn warning(node: &LoadedNode, message: String) -> Finding {
    Finding {
        severity: Severity::Warning,
        path: node.location.clone(),
        message,
    }
}
