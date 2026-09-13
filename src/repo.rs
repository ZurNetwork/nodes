//! Where nodes live on disk: root discovery, file discovery, reading and writing.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::schema::{NODE_FILE, Node, NodePath};

/// Directories never descended into, whatever `.chartignore` says.
const ALWAYS_SKIPPED: [&str; 4] = [".git", ".jj", "node_modules", "target"];
/// The chart's ignore file: one directory name per line, gitignore-style, `#` comments.
const IGNORE_FILE: &str = ".chartignore";

/// A charted repository, anchored at the directory holding the root node.
#[derive(Clone, Debug)]
pub struct Repo {
    root: PathBuf,
}

/// A node read from disk. `location` is where the file sits — the truth `check` holds `path` against.
#[derive(Clone, Debug)]
pub struct LoadedNode {
    pub file: PathBuf,
    pub location: NodePath,
    pub node: Node,
}

/// A `NODE.json` that could not be read or does not fit the schema.
#[derive(Debug)]
pub struct Unreadable {
    pub location: NodePath,
    pub error: Error,
}

impl Repo {
    /// The repository whose root node sits in `root`.
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// Walks up from `start` to the nearest directory whose `NODE.json` declares `path: "."`.
    pub fn discover(start: &Path) -> Result<Self, Error> {
        let mut candidate = Some(start);
        while let Some(dir) = candidate {
            if Self::holds_root_node(dir) {
                return Ok(Self::at(dir.to_path_buf()));
            }
            candidate = dir.parent();
        }
        Err(Error::NoRoot {
            start: start.to_path_buf(),
        })
    }

    fn holds_root_node(dir: &Path) -> bool {
        let Ok(text) = fs::read_to_string(dir.join(NODE_FILE)) else {
            return false;
        };
        Node::parse(&text).is_ok_and(|node| node.path.is_root())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The `NODE.json` of the node at `path`.
    pub fn node_file(&self, path: &NodePath) -> PathBuf {
        path.to_dir(&self.root).join(NODE_FILE)
    }

    /// A file as messages print it: relative to the root when it lies beneath it.
    pub fn display(&self, file: &Path) -> String {
        file.strip_prefix(&self.root)
            .unwrap_or(file)
            .display()
            .to_string()
    }

    /// A typed path as a node path (see [`NodePath::normalize`]).
    pub fn node_path(&self, input: &str) -> Result<NodePath, Error> {
        NodePath::normalize(input, &self.root).map_err(|reason| Error::InvalidPath {
            input: input.to_owned(),
            reason: reason.to_string(),
        })
    }

    /// The node path of the directory holding `file`.
    pub fn location_of(&self, file: &Path) -> Result<NodePath, Error> {
        let text = file.to_str().ok_or_else(|| Error::InvalidPath {
            input: file.display().to_string(),
            reason: "not UTF-8".to_owned(),
        })?;
        let dir = self.node_path(text)?;
        Ok(dir)
    }

    /// Every `NODE.json` beneath the root, skipping ignored directories.
    pub fn node_files(&self) -> Result<Vec<PathBuf>, Error> {
        let skipped = self.skipped_names()?;
        let mut found = Vec::new();
        collect_node_files(&self.root, &skipped, &mut found)?;
        Ok(found)
    }

    fn skipped_names(&self) -> Result<Vec<String>, Error> {
        let mut names: Vec<String> = ALWAYS_SKIPPED.map(str::to_owned).to_vec();
        let ignore_file = self.root.join(IGNORE_FILE);
        if !ignore_file.exists() {
            return Ok(names);
        }
        let text =
            fs::read_to_string(&ignore_file).map_err(|source| Error::io(&ignore_file, source))?;
        let listed = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.trim_matches('/').to_owned());
        names.extend(listed);
        Ok(names)
    }

    /// Reads one node file.
    pub fn read(&self, file: &Path) -> Result<LoadedNode, Error> {
        let text = fs::read_to_string(file).map_err(|source| Error::io(file, source))?;
        let node = Node::parse(&text).map_err(|source| Error::Schema {
            file: file.to_path_buf(),
            source,
        })?;
        let location = self.location_of(file)?;
        let loaded = LoadedNode {
            file: file.to_path_buf(),
            location,
            node,
        };
        Ok(loaded)
    }

    /// Reads every node; the first unreadable file stops the run.
    pub fn read_all(&self) -> Result<Vec<LoadedNode>, Error> {
        self.node_files()?
            .iter()
            .map(|file| self.read(file))
            .collect()
    }

    /// Reads every node; an unreadable file is reported alongside the rest instead of stopping the run.
    pub fn read_all_lenient(&self) -> Result<(Vec<LoadedNode>, Vec<Unreadable>), Error> {
        let mut loaded = Vec::new();
        let mut unreadable = Vec::new();
        for file in self.node_files()? {
            match self.read(&file) {
                Ok(node) => loaded.push(node),
                Err(error) => {
                    let location = self.location_of(&file)?;
                    unreadable.push(Unreadable { location, error });
                }
            }
        }
        Ok((loaded, unreadable))
    }

    /// Writes the node in canonical form; `true` when the file's bytes changed.
    pub fn write(&self, file: &Path, node: &Node) -> Result<bool, Error> {
        let canonical = node.canonical_json();
        let current = fs::read_to_string(file).ok();
        if current.as_deref() == Some(canonical.as_str()) {
            return Ok(false);
        }
        fs::write(file, canonical).map_err(|source| Error::io(file, source))?;
        Ok(true)
    }
}

fn collect_node_files(
    dir: &Path,
    skipped: &[String],
    found: &mut Vec<PathBuf>,
) -> Result<(), Error> {
    let listing = fs::read_dir(dir).map_err(|source| Error::io(dir, source))?;
    let mut entries: Vec<fs::DirEntry> = listing
        .collect::<Result<_, _>>()
        .map_err(|source| Error::io(dir, source))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let kind = entry
            .file_type()
            .map_err(|source| Error::io(entry.path(), source))?;
        if kind.is_dir() {
            let name = name.to_string_lossy();
            if skipped.iter().any(|skip| *skip == name) {
                continue;
            }
            collect_node_files(&entry.path(), skipped, found)?;
        } else if name == NODE_FILE {
            found.push(entry.path());
        }
    }
    Ok(())
}
