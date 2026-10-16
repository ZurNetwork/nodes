//! The one error type every command returns, rendered as `nodes: <message>` on stderr.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::schema::{Field, NodePath, PageId};

/// Everything that can stop a command.
#[derive(Debug)]
pub enum Error {
    /// No ancestor of the starting directory roots a tree: no root node file (one whose `path` is
    /// `.`), no `NODE.refs.json`.
    NoRoot { start: PathBuf },
    /// A write to a tree from before v0.7 (its root holds a `NODE.json`): reads work, writes wait
    /// for `migrate`. `found_above` is where discovery started, when the tree was found above it.
    Unmigrated {
        root: PathBuf,
        found_above: Option<PathBuf>,
    },
    /// A directory holds a node twice, in a `NODE.json` and a `NODE.inf.json` that say different
    /// things: `migrate` will not choose between them.
    TwoNodeFiles { dir: PathBuf },
    /// `migrate` or `init` was pointed at a directory that holds no tree to migrate.
    NoTree { root: PathBuf },
    /// The tree's refs are here but its nodes are not: a fresh clone, a worktree, CI.
    NoNodeTree { root: PathBuf },
    /// A write would follow a symbolic link to wherever it points.
    Symlink { path: PathBuf },
    /// `init` in a directory that holds a node of a tree above it.
    NotARoot { dir: PathBuf, path: NodePath },
    /// A file or directory could not be read, written or listed.
    Io { path: PathBuf, source: io::Error },
    /// A node file does not fit the schema.
    Schema {
        file: PathBuf,
        source: serde_json::Error,
    },
    /// A node file an older release wrote — `refs` inside, ranked `categories` — that only
    /// `migrate` brings up to date.
    NeedsMigrating {
        file: PathBuf,
        source: serde_json::Error,
    },
    /// A ref edit with no node tree and no `NODE.refs.json` here, in a directory that is no
    /// checkout's top level: a refs file there would start a tree of its own.
    RefsOutsideTree { root: PathBuf },
    /// No node is charted at this path.
    UnknownNode(NodePath),
    /// A typed path could not be turned into a node path.
    InvalidPath { input: String, reason: String },
    /// A command argument failed to parse.
    InvalidArgument {
        what: &'static str,
        input: String,
        reason: String,
    },
    /// A value handed to `set` does not fit the field.
    InvalidValue {
        field: Field,
        source: serde_json::Error,
    },
    /// `set path` is refused: a node's path is derived from where its file sits.
    PathIsDerived,
    /// `rm-ref` on a page the node does not cite.
    RefNotCited { path: NodePath, page: PageId },
    /// A write aimed at a node of a mounted tree: writes never cross a mount.
    InsideMount {
        path: NodePath,
        mount: NodePath,
        mount_dir: PathBuf,
    },
    /// A directory's ignore files could not be compiled into rules.
    IgnoreRules { dir: PathBuf, source: ignore::Error },
}

impl Error {
    /// An I/O failure at `path`.
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRoot { start } => write!(
                f,
                "no tree in {} or any parent (no root NODE.inf.json, no NODE.refs.json); a tree lives in the checkout where it was charted: pass --root <that checkout>, or start one with `nodes init`",
                start.display()
            ),
            Self::Unmigrated { root, found_above } => {
                write!(
                    f,
                    "{} holds a tree from before v0.7 (NODE.json files): it is read as it is, but writes wait for `nodes migrate` there",
                    root.display()
                )?;
                match found_above {
                    Some(start) => write!(
                        f,
                        " (found above {}; to work on another tree, pass --root)",
                        start.display()
                    ),
                    None => Ok(()),
                }
            }
            Self::TwoNodeFiles { dir } => write!(
                f,
                "{} holds both NODE.json and NODE.inf.json, and they differ; delete the one that is out of date (after checking out an older commit, that is NODE.json), then migrate",
                dir.display()
            ),
            Self::NoTree { root } => {
                write!(f, "{} holds no tree: nothing to migrate", root.display())
            }
            Self::NoNodeTree { root } => write!(
                f,
                "{} holds the tree's NODE.refs.json but no nodes: node files are never committed, so a fresh clone, a worktree or CI has none; chart it here, or pass --root <the checkout that holds the tree>",
                root.display()
            ),
            Self::NotARoot { dir, path } => write!(
                f,
                "{} holds the node `{path}` of a tree above it, so it is no root; `init` makes a root in a directory that holds no node",
                dir.display()
            ),
            Self::Symlink { path } => write!(
                f,
                "{} is a symbolic link; nodes writes no file through one",
                path.display()
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Schema { file, source } => write!(f, "{}: {source}", file.display()),
            Self::NeedsMigrating { file, source } => write!(
                f,
                "{}: {source} — an older release wrote it; `nodes migrate` brings it up to date (refs move into NODE.refs.json)",
                file.display()
            ),
            Self::RefsOutsideTree { root } => write!(
                f,
                "{} holds no tree and is no checkout's top level, so a NODE.refs.json here would start a tree of its own; pass --root <the checkout's top level>",
                root.display()
            ),
            Self::UnknownNode(path) => write!(f, "no node charted at `{path}`"),
            Self::InvalidPath { input, reason } => write!(f, "`{input}`: {reason}"),
            Self::InvalidArgument {
                what,
                input,
                reason,
            } => write!(f, "{what} `{input}`: {reason}"),
            Self::InvalidValue { field, source } => {
                write!(f, "value does not fit field `{field}`: {source}")
            }
            Self::PathIsDerived => {
                write!(
                    f,
                    "`path` is derived from the file's location and cannot be set"
                )
            }
            Self::RefNotCited { path, page } => {
                write!(f, "`{path}` does not cite page {page}")
            }
            Self::InsideMount {
                path,
                mount,
                mount_dir,
            } => write!(
                f,
                "`{path}` belongs to the tree mounted at `{mount}`; writes do not cross a mount — run this with --root {}",
                mount_dir.display()
            ),
            Self::IgnoreRules { dir, source } => {
                write!(f, "{}: ignore rules: {source}", dir.display())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Schema { source, .. }
            | Self::NeedsMigrating { source, .. }
            | Self::InvalidValue { source, .. } => Some(source),
            Self::IgnoreRules { source, .. } => Some(source),
            _ => None,
        }
    }
}
