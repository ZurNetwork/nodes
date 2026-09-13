//! The one error type every command returns, rendered as `nodes: <message>` on stderr.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::schema::{Field, NodePath, PageId};

/// Everything that can stop a command.
#[derive(Debug)]
pub enum Error {
    /// No ancestor of the starting directory holds a root `NODE.json` (one whose `path` is `.`).
    NoRoot { start: PathBuf },
    /// A file or directory could not be read, written or listed.
    Io { path: PathBuf, source: io::Error },
    /// A `NODE.json` does not fit the schema.
    Schema {
        file: PathBuf,
        source: serde_json::Error,
    },
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
                "no root NODE.json (one whose path is \".\") in {} or any parent; pass --root",
                start.display()
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Schema { file, source } => write!(f, "{}: {source}", file.display()),
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
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Schema { source, .. } | Self::InvalidValue { source, .. } => Some(source),
            _ => None,
        }
    }
}
