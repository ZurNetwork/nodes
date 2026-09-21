//! Where nodes live on disk: root discovery, file discovery, reading and writing.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Error;
use crate::ignores::IgnoreRules;
use crate::schema::{NODE_FILE, Node, NodePath};

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
    /// Whether this node is the root of a tree mounted beneath the one being read.
    pub is_mount: bool,
}

/// What a walk of the tree found: its own node files, and the mounts it stopped at.
#[derive(Debug, Default)]
pub struct Discovered {
    /// Every `NODE.json` that belongs to this tree.
    pub node_files: Vec<PathBuf>,
    /// Directories beneath the root that hold a root node of their own: other trees, mounted
    /// here. The walk does not descend into them.
    pub mounts: Vec<PathBuf>,
}

/// The one field that makes a `NODE.json` a root; the rest of the file need not fit the schema.
#[derive(Debug, Deserialize)]
struct DeclaredPath {
    path: NodePath,
}

impl LoadedNode {
    /// This node as seen from a tree that mounts its own at `mount_point`: `location` and `path`
    /// are rebased onto that tree's root, and the mounted tree's root node is marked as the mount.
    pub fn mounted_at(self, mount_point: &NodePath) -> Self {
        let is_mount = self.is_mount || self.location.is_root();
        let location = self.location.mounted_at(mount_point);
        let path = self.node.path.mounted_at(mount_point);
        let node = Node { path, ..self.node };
        Self {
            file: self.file,
            location,
            node,
            is_mount,
        }
    }
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

    /// Whether `dir` holds a root node: a `NODE.json` declaring `path: "."`. Only `path` is read,
    /// so a root that does not fit the schema is still a root — and is reported as one.
    fn holds_root_node(dir: &Path) -> bool {
        let Ok(text) = fs::read_to_string(dir.join(NODE_FILE)) else {
            return false;
        };
        serde_json::from_str::<DeclaredPath>(&text).is_ok_and(|declared| declared.path.is_root())
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

    /// Walks the tree: every `NODE.json` beneath the root that belongs to it, and the mounts the
    /// walk stopped at. Ignored directories are skipped (see [`IgnoreRules`]).
    pub fn walk(&self) -> Result<Discovered, Error> {
        let mut rules = IgnoreRules::default();
        let mut found = Discovered::default();
        self.collect(&self.root, &mut rules, &mut found)?;
        Ok(found)
    }

    /// Every `NODE.json` of this tree — never a mounted tree's.
    pub fn node_files(&self) -> Result<Vec<PathBuf>, Error> {
        let found = self.walk()?;
        Ok(found.node_files)
    }

    fn collect(
        &self,
        dir: &Path,
        rules: &mut IgnoreRules,
        found: &mut Discovered,
    ) -> Result<(), Error> {
        let listing = fs::read_dir(dir).map_err(|source| Error::io(dir, source))?;
        let mut entries: Vec<fs::DirEntry> = listing
            .collect::<Result<_, _>>()
            .map_err(|source| Error::io(dir, source))?;
        entries.sort_by_key(fs::DirEntry::file_name);
        let holds_node = entries.iter().any(|entry| entry.file_name() == NODE_FILE);
        let is_mount = holds_node && dir != self.root && Self::holds_root_node(dir);
        if is_mount {
            found.mounts.push(dir.to_path_buf());
            return Ok(());
        }
        rules.enter(dir)?;
        for entry in entries {
            let kind = entry
                .file_type()
                .map_err(|source| Error::io(entry.path(), source))?;
            if kind.is_dir() {
                if rules.skips(&entry.path()) {
                    continue;
                }
                self.collect(&entry.path(), rules, found)?;
            } else if entry.file_name() == NODE_FILE {
                found.node_files.push(entry.path());
            }
        }
        rules.leave();
        Ok(())
    }

    /// The tree mounted at `mount_point`, as a repository of its own.
    pub fn mounted_tree(&self, mount_point: &NodePath) -> Self {
        Self::at(mount_point.to_dir(&self.root))
    }

    /// The mount `path` belongs to: the deepest directory at or above it, strictly beneath the
    /// root, that holds a root node. `None` for a path of this tree.
    pub fn mount_holding(&self, path: &NodePath) -> Option<NodePath> {
        std::iter::successors(Some(path.clone()), NodePath::parent)
            .take_while(|location| !location.is_root())
            .find(|location| Self::holds_root_node(&location.to_dir(&self.root)))
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
            is_mount: false,
        };
        Ok(loaded)
    }

    /// Reads every node a reader sees: this tree's own, then each mounted tree's — mounts within
    /// mounts included — rebased onto this root (see [`LoadedNode::mounted_at`]). A mounted tree
    /// is read exactly as it reads from its own root. The first unreadable file stops the run.
    pub fn read_across_mounts(&self) -> Result<Vec<LoadedNode>, Error> {
        let found = self.walk()?;
        let mut nodes: Vec<LoadedNode> = found
            .node_files
            .iter()
            .map(|file| self.read(file))
            .collect::<Result<_, _>>()?;
        for mount_dir in &found.mounts {
            let mount_point = self.location_of(mount_dir)?;
            let mounted_nodes = self.mounted_tree(&mount_point).read_across_mounts()?;
            let rebased = mounted_nodes
                .into_iter()
                .map(|node| node.mounted_at(&mount_point));
            nodes.extend(rebased);
        }
        Ok(nodes)
    }

    /// Reads the given node files; an unreadable one is reported alongside the rest instead of
    /// stopping the run.
    pub fn read_lenient(
        &self,
        files: &[PathBuf],
    ) -> Result<(Vec<LoadedNode>, Vec<Unreadable>), Error> {
        let mut loaded = Vec::new();
        let mut unreadable = Vec::new();
        for file in files {
            match self.read(file) {
                Ok(node) => loaded.push(node),
                Err(error) => {
                    let location = self.location_of(file)?;
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
