//! Where nodes live on disk: root discovery, file discovery, reading and writing.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Error;
use crate::ignores::IgnoreRules;
use crate::render::note;
use crate::schema::{
    LEGACY_NODE_FILE, NODE_FILE, NODE_FILES_IGNORED, Node, NodePath, REFS_FILE, Ref,
};
use crate::treerefs::TreeRefs;

/// The `.gitignore` lines that already keep every node file out, whichever way they are spelled.
const NODE_FILES_IGNORED_SPELLINGS: [&str; 3] =
    [NODE_FILES_IGNORED, "*.inf.json", "/**/*.inf.json"];

/// Which release's files a tree keeps its nodes in, judged by its root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Era {
    /// `NODE.inf.json` nodes, their refs in `NODE.refs.json`: v0.7 on.
    Current,
    /// `NODE.json` nodes that hold their own refs: a tree from before v0.7. It is read as it is,
    /// and only `migrate` writes it.
    Legacy,
}

impl Era {
    /// The era of the tree rooted at `root`: legacy while the root holds a `NODE.json`, beside a
    /// `NODE.inf.json` or not — until `migrate` has run, which of the two is current is not the
    /// tool's to guess.
    fn of(root: &Path) -> Self {
        if root.join(LEGACY_NODE_FILE).is_file() {
            Self::Legacy
        } else {
            Self::Current
        }
    }

    /// The file a node of this era lives in.
    pub const fn node_file(self) -> &'static str {
        match self {
            Self::Current => NODE_FILE,
            Self::Legacy => LEGACY_NODE_FILE,
        }
    }
}

/// A charted repository, anchored at the directory holding the root node (or, in a checkout with
/// no node tree, the tree's `NODE.refs.json`).
#[derive(Clone, Debug)]
pub struct Repo {
    root: PathBuf,
    era: Era,
    /// Where discovery started, when the tree was found above it.
    found_above: Option<PathBuf>,
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
    /// Every node file that belongs to this tree.
    pub node_files: Vec<PathBuf>,
    /// Directories beneath the root that hold a tree of their own: other trees, mounted here. The
    /// walk does not descend into them.
    pub mounts: Vec<PathBuf>,
    /// Node files of the other era the walk passed by — a `NODE.json` left behind in a tree that
    /// keeps `NODE.inf.json` files, say.
    pub passed_over: Vec<PathBuf>,
    /// The content files a walk over content kept; none for a walk over the chart alone.
    files: Vec<PathBuf>,
    /// Whether a walk over content reached the directory it was asked about.
    reached_start: bool,
}

/// How far a walk reaches beyond the chart itself.
#[derive(Clone, Copy, Debug)]
enum Reach<'a> {
    /// Node files only: what every command over the chart needs.
    Nodes,
    /// Also the content files at or beneath this directory. The walk is narrowed to the way
    /// there, so the ignore files above it still speak and nothing beside it is listed.
    FilesWithin(&'a Path),
}

impl Reach<'_> {
    /// Whether the walk has business in `dir`.
    fn enters(self, dir: &Path) -> bool {
        match self {
            Self::Nodes => true,
            Self::FilesWithin(start) => dir.starts_with(start) || start.starts_with(dir),
        }
    }

    /// Whether `file` is content the walk keeps. A chart file never is.
    fn keeps(self, file: &Path) -> bool {
        match self {
            Self::Nodes => false,
            Self::FilesWithin(start) => file.starts_with(start),
        }
    }

    fn starts_at(self, dir: &Path) -> bool {
        matches!(self, Self::FilesWithin(start) if start == dir)
    }
}

/// One walk's scope: how far beyond the chart it reaches, and which file names are nodes.
#[derive(Clone, Copy, Debug)]
struct Walk<'a> {
    reach: Reach<'a>,
    names: NodeNames,
}

/// The file names a walk takes for a node.
#[derive(Clone, Copy, Debug)]
enum NodeNames {
    /// The one file a tree of this era keeps its nodes in: what every read walks.
    Of(Era),
    /// Both — what `migrate` walks.
    Every,
}

impl NodeNames {
    fn takes(self, name: &OsStr) -> bool {
        match self {
            Self::Of(era) => name == era.node_file(),
            Self::Every => is_node_file_name(name),
        }
    }
}

/// Whether `name` is a node file of either era.
fn is_node_file_name(name: &OsStr) -> bool {
    name == NODE_FILE || name == LEGACY_NODE_FILE
}

/// Whether `name` belongs to the chart rather than to what it charts: a node file of either era,
/// or a tree's refs. A chart file is never content.
fn is_chart_file_name(name: &OsStr) -> bool {
    is_node_file_name(name) || name == REFS_FILE
}

/// The content beneath a directory, across mounts: every regular file that is not a chart file
/// and that no ignore rule passes by, sorted — and the directories that hold a node, so each file
/// can be given to the node that owns it.
#[derive(Debug, Default)]
pub struct Content {
    pub files: Vec<PathBuf>,
    node_dirs: BTreeSet<PathBuf>,
    /// Whether the walk reached the directory it was asked about; an ignored one is never reached.
    pub reached: bool,
}

impl Content {
    /// The directory of the node that owns `file`: the deepest one above it holding a node.
    pub fn owner_of(&self, file: &Path) -> Option<&Path> {
        file.ancestors()
            .skip(1)
            .find_map(|dir| self.node_dirs.get(dir))
            .map(PathBuf::as_path)
    }

    /// Every directory the walk found a node in.
    pub fn node_dirs(&self) -> impl Iterator<Item = &Path> {
        self.node_dirs.iter().map(PathBuf::as_path)
    }
}

/// The one field that makes a node file a root; the rest of the file need not fit the schema.
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

/// A node file that could not be read or does not fit the schema.
#[derive(Debug)]
pub struct Unreadable {
    pub location: NodePath,
    pub error: Error,
}

/// What a write changed: the node file, the tree's `NODE.refs.json`, or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub node: bool,
    pub refs: bool,
}

/// One ref as a reader sees it: the node path citing it, and the ref.
pub type Citation = (NodePath, Ref);

impl Repo {
    /// The tree rooted at `root`, in whichever era its root says.
    pub fn at(root: PathBuf) -> Self {
        let era = Era::of(&root);
        Self {
            root,
            era,
            found_above: None,
        }
    }

    /// Walks up from `start` to the nearest directory that roots a tree (see
    /// [`Repo::holds_root`]). A root from before v0.7 (a `NODE.json`) is found too: it is read as
    /// it is, and `migrate` brings it up to date.
    pub fn discover(start: &Path) -> Result<Self, Error> {
        let root = start
            .ancestors()
            .find(|dir| Self::holds_root(dir))
            .ok_or_else(|| Error::NoRoot {
                start: start.to_path_buf(),
            })?;
        let found_above = (root != start).then(|| start.to_path_buf());
        let repo = Self {
            found_above,
            ..Self::at(root.to_path_buf())
        };
        Ok(repo)
    }

    /// Where discovery started, when the tree was found above it.
    pub fn found_above(&self) -> Option<&Path> {
        self.found_above.as_deref()
    }

    /// Whether `dir` is a checkout's top level: it holds `.git` (a directory, or a worktree's
    /// file) or `.jj`.
    pub fn is_checkout_top(dir: &Path) -> bool {
        dir.join(".git").exists() || dir.join(".jj").exists()
    }

    /// Whether `dir` roots a tree: it holds a node file of either era declaring `path: "."`, or
    /// the tree's `NODE.refs.json` — all a checkout with no node tree has. Only `path` is read, so
    /// a root that does not fit the schema is still a root — and is reported as one.
    fn holds_root(dir: &Path) -> bool {
        dir.join(REFS_FILE).is_file() || Self::holds_root_node(dir)
    }

    /// Whether `dir` holds a root node: a node file of either era declaring `path: "."`.
    pub fn holds_root_node(dir: &Path) -> bool {
        let declares_root = |node_file: &str| {
            let Ok(text) = fs::read_to_string(dir.join(node_file)) else {
                return false;
            };
            serde_json::from_str::<DeclaredPath>(&text)
                .is_ok_and(|declared| declared.path.is_root())
        };
        declares_root(NODE_FILE) || declares_root(LEGACY_NODE_FILE)
    }

    pub fn era(&self) -> Era {
        self.era
    }

    /// Whether this is a tree from before v0.7: read as it is, written only by `migrate`.
    pub fn is_unmigrated(&self) -> bool {
        self.era == Era::Legacy
    }

    /// Whether the tree rooted at `dir` is from before v0.7 (see [`Repo::is_unmigrated`]).
    pub fn is_unmigrated_dir(dir: &Path) -> bool {
        Era::of(dir) == Era::Legacy
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The node file of the node at `path`, in this tree's era.
    pub fn node_file(&self, path: &NodePath) -> PathBuf {
        path.to_dir(&self.root).join(self.era.node_file())
    }

    /// The tree's `NODE.refs.json`.
    pub fn refs_file(&self) -> PathBuf {
        self.root.join(REFS_FILE)
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

    /// The node path of a directory, or of the directory holding a node file of either era.
    pub fn location_of(&self, path: &Path) -> Result<NodePath, Error> {
        let is_node_file = path.file_name().is_some_and(is_node_file_name);
        let dir = match path.parent() {
            Some(parent) if is_node_file => parent,
            _ => path,
        };
        let text = dir.to_str().ok_or_else(|| Error::InvalidPath {
            input: dir.display().to_string(),
            reason: "not UTF-8".to_owned(),
        })?;
        self.node_path(text)
    }

    /// Walks the tree: every node file of its era beneath the root, and the mounts the walk
    /// stopped at. Ignored directories are skipped (see [`IgnoreRules`]).
    pub fn walk(&self) -> Result<Discovered, Error> {
        let walk = Walk {
            reach: Reach::Nodes,
            names: NodeNames::Of(self.era),
        };
        self.walked(walk)
    }

    fn walked(&self, walk: Walk<'_>) -> Result<Discovered, Error> {
        let mut rules = IgnoreRules::default();
        let mut found = Discovered::default();
        self.collect(&self.root, walk, &mut rules, &mut found)?;
        Ok(found)
    }

    /// The content at or beneath `start` (see [`Content`]): this tree's, then each mounted tree's
    /// — mounts within mounts included. A mounted tree is walked exactly as from its own root,
    /// under its own ignore files. Paths stay absolute; the caller names them from its own root.
    pub fn content_across_mounts(&self, start: &Path) -> Result<Content, Error> {
        let walk = Walk {
            reach: Reach::FilesWithin(start),
            names: NodeNames::Of(self.era),
        };
        let found = self.walked(walk)?;
        let node_dirs = found
            .node_files
            .iter()
            .filter_map(|file| file.parent())
            .map(Path::to_path_buf)
            .collect();
        let mut content = Content {
            files: found.files,
            node_dirs,
            reached: found.reached_start,
        };
        for mount_dir in found.mounts {
            let mounted = Self::at(mount_dir).content_across_mounts(start)?;
            content.files.extend(mounted.files);
            content.node_dirs.extend(mounted.node_dirs);
            content.reached |= mounted.reached;
        }
        content.files.sort();
        Ok(content)
    }

    /// Every node file of this tree, in its era — never a mounted tree's.
    pub fn node_files(&self) -> Result<Vec<PathBuf>, Error> {
        let found = self.walk()?;
        Ok(found.node_files)
    }

    /// What `migrate` walks: every node file of this tree, `NODE.inf.json` or `NODE.json`, and the
    /// mounts — trees of either era — it does not cross.
    pub fn walk_every_era(&self) -> Result<Discovered, Error> {
        let walk = Walk {
            reach: Reach::Nodes,
            names: NodeNames::Every,
        };
        self.walked(walk)
    }

    fn collect(
        &self,
        dir: &Path,
        walk: Walk<'_>,
        rules: &mut IgnoreRules,
        found: &mut Discovered,
    ) -> Result<(), Error> {
        let listing = fs::read_dir(dir).map_err(|source| Error::io(dir, source))?;
        let mut entries: Vec<fs::DirEntry> = listing
            .collect::<Result<_, _>>()
            .map_err(|source| Error::io(dir, source))?;
        entries.sort_by_key(fs::DirEntry::file_name);
        let holds_chart_file = entries
            .iter()
            .any(|entry| is_chart_file_name(&entry.file_name()));
        let is_mount = holds_chart_file && dir != self.root && Self::holds_root(dir);
        if is_mount {
            found.mounts.push(dir.to_path_buf());
            return Ok(());
        }
        if walk.reach.starts_at(dir) {
            found.reached_start = true;
        }
        rules.enter(dir)?;
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name();
            let kind = entry
                .file_type()
                .map_err(|source| Error::io(&path, source))?;
            if kind.is_dir() {
                if rules.skips(&path) || !walk.reach.enters(&path) {
                    continue;
                }
                self.collect(&path, walk, rules, found)?;
            } else if walk.names.takes(&name) {
                // The chart is found whatever the ignore files say about files.
                found.node_files.push(path);
            } else if is_node_file_name(&name) {
                found.passed_over.push(path);
            } else if is_chart_file_name(&name) {
                // The tree's refs: the chart, not content.
            } else if kind.is_file() && walk.reach.keeps(&path) && !rules.skips_file(&path) {
                found.files.push(path);
            }
        }
        rules.leave();
        Ok(())
    }

    /// Keeps every node file out of version control: appends [`NODE_FILES_IGNORED`] to the
    /// root's `.gitignore` — creating it if need be, in the line endings it already uses — unless
    /// a line already says it. A `.gitignore` that re-includes node files (`!…`) is left as it is,
    /// with a note: the appended line would silently override it. `true` when the file changed.
    pub fn ignore_node_files(&self) -> Result<bool, Error> {
        let file = self.root.join(".gitignore");
        let current = read_if_present(&file)?.unwrap_or_default();
        let patterns: Vec<&str> = current.lines().map(str::trim).collect();
        let ignores_node_files = |pattern: &str| {
            NODE_FILES_IGNORED_SPELLINGS.contains(&pattern) || pattern.ends_with(NODE_FILE)
        };
        let re_includes = patterns
            .iter()
            .filter_map(|pattern| pattern.strip_prefix('!'))
            .any(ignores_node_files);
        if re_includes {
            note(&format!(
                "{}: re-includes node files (a `!` line), so `{NODE_FILES_IGNORED}` was not added",
                file.display()
            ));
            return Ok(false);
        }
        let already_ignored = patterns
            .iter()
            .any(|pattern| NODE_FILES_IGNORED_SPELLINGS.contains(pattern));
        if already_ignored {
            return Ok(false);
        }
        let newline = if current.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut updated = current;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push_str(newline);
        }
        updated.push_str(NODE_FILES_IGNORED);
        updated.push_str(newline);
        write_if_changed(&file, &updated)
    }

    /// The tree mounted at `mount_point`, as a repository of its own.
    pub fn mounted_tree(&self, mount_point: &NodePath) -> Self {
        Self::at(mount_point.to_dir(&self.root))
    }

    /// The mount `path` belongs to: the deepest directory at or above it, strictly beneath the
    /// root, that roots a tree. `None` for a path of this tree.
    pub fn mount_holding(&self, path: &NodePath) -> Option<NodePath> {
        std::iter::successors(Some(path.clone()), NodePath::parent)
            .take_while(|location| !location.is_root())
            .find(|location| Self::holds_root(&location.to_dir(&self.root)))
    }

    /// The tree's refs, as its `NODE.refs.json` holds them; none when it has no such file.
    pub fn load_refs(&self) -> Result<TreeRefs, Error> {
        let file = self.refs_file();
        let Some(text) = read_if_present(&file)? else {
            return Ok(TreeRefs::default());
        };
        TreeRefs::parse(&text).map_err(|source| Error::Schema { file, source })
    }

    /// Reads one node file, its refs merged in.
    pub fn read(&self, file: &Path) -> Result<LoadedNode, Error> {
        let refs = self.refs_for_reading()?;
        self.read_with(file, &refs)
    }

    /// The refs a read merges into this tree's nodes: its `NODE.refs.json`; none for a tree from
    /// before v0.7, whose node files hold their own.
    fn refs_for_reading(&self) -> Result<TreeRefs, Error> {
        match self.era {
            Era::Current => self.load_refs(),
            Era::Legacy => Ok(TreeRefs::default()),
        }
    }

    /// Reads one node file, merging in its refs from `refs` — or, from before v0.7, from the file.
    fn read_with(&self, file: &Path, refs: &TreeRefs) -> Result<LoadedNode, Error> {
        let text = fs::read_to_string(file).map_err(|source| Error::io(file, source))?;
        let location = self.location_of(file)?;
        let parsed = match self.era {
            Era::Current => Node::parse(&text).map(|node| Node {
                refs: refs.of(&location).to_vec(),
                ..node
            }),
            Era::Legacy => Node::parse_migrating(&text).map(|migrating| migrating.node),
        };
        let node = parsed.map_err(|source| {
            // A file an older release wrote — `refs` inside, ranked `categories` — is `migrate`'s.
            let older_release = Node::parse_migrating(&text).is_ok();
            let file = file.to_path_buf();
            if older_release {
                Error::NeedsMigrating { file, source }
            } else {
                Error::Schema { file, source }
            }
        })?;
        let loaded = LoadedNode {
            file: file.to_path_buf(),
            location,
            node,
            is_mount: false,
        };
        Ok(loaded)
    }

    /// Says on stderr that this tree is from before v0.7 — read as it is — when it is.
    pub fn note_if_unmigrated(&self) {
        if self.is_unmigrated() {
            let found_above = self
                .found_above
                .as_ref()
                .map(|start| {
                    format!(
                        " (found above {}; to read another tree, pass --root)",
                        start.display()
                    )
                })
                .unwrap_or_default();
            note(&format!(
                "{}: a tree from before v0.7 (NODE.json files), read as it is{found_above}; run `nodes migrate` there before any write",
                self.root.display()
            ));
        }
    }

    /// Reads every node a reader sees: this tree's own, then each mounted tree's — mounts within
    /// mounts included — rebased onto this root (see [`LoadedNode::mounted_at`]). A mounted tree
    /// is read exactly as it reads from its own root, in its own era. A node file of the wrong era
    /// is passed over with a note. The first unreadable file stops the run.
    pub fn read_across_mounts(&self) -> Result<Vec<LoadedNode>, Error> {
        self.note_if_unmigrated();
        let found = self.walk()?;
        for file in &found.passed_over {
            note(&format!(
                "passed over {}: this tree keeps its nodes in {} files — `nodes migrate` settles it",
                file.display(),
                self.era.node_file()
            ));
        }
        let refs = self.refs_for_reading()?;
        let mut nodes: Vec<LoadedNode> = found
            .node_files
            .iter()
            .map(|file| self.read_with(file, &refs))
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

    /// Every ref a reader sees, with the node path citing it, in tree order: this tree's — from
    /// its `NODE.refs.json`, or from a tree from before v0.7, its node files — then each mounted
    /// tree's, rebased. A checkout with no node tree, only the refs, answers too.
    pub fn citations_across_mounts(&self) -> Result<Vec<Citation>, Error> {
        self.note_if_unmigrated();
        let found = self.walk()?;
        let mut citations: Vec<Citation> = Vec::new();
        match self.era {
            Era::Current => {
                let refs = self.load_refs()?;
                for (path, cited) in refs.iter() {
                    let these = cited
                        .iter()
                        .map(|reference| (path.clone(), reference.clone()));
                    citations.extend(these);
                }
            }
            Era::Legacy => {
                for file in &found.node_files {
                    let loaded = self.read_with(file, &TreeRefs::default())?;
                    let location = loaded.location;
                    let these = loaded
                        .node
                        .refs
                        .into_iter()
                        .map(|reference| (location.clone(), reference));
                    citations.extend(these);
                }
            }
        }
        for mount_dir in &found.mounts {
            let mount_point = self.location_of(mount_dir)?;
            let mounted = self.mounted_tree(&mount_point).citations_across_mounts()?;
            let rebased = mounted
                .into_iter()
                .map(|(path, reference)| (path.mounted_at(&mount_point), reference));
            citations.extend(rebased);
        }
        // A stable sort: a node's refs keep their order.
        citations.sort_by(|(left, _), (right, _)| left.cmp(right));
        Ok(citations)
    }

    /// Reads the given node files, merging in `refs`; an unreadable one is reported alongside the
    /// rest instead of stopping the run.
    pub fn read_lenient(
        &self,
        files: &[PathBuf],
        refs: &TreeRefs,
    ) -> Result<(Vec<LoadedNode>, Vec<Unreadable>), Error> {
        let mut loaded = Vec::new();
        let mut unreadable = Vec::new();
        for file in files {
            match self.read_with(file, refs) {
                Ok(node) => loaded.push(node),
                Err(error) => {
                    let location = self.location_of(file)?;
                    unreadable.push(Unreadable { location, error });
                }
            }
        }
        Ok((loaded, unreadable))
    }

    /// Writes the node in canonical form: the node file without its refs, and its refs into the
    /// tree's `NODE.refs.json`. A tree from before v0.7 is refused: `migrate` comes first.
    pub fn write(&self, file: &Path, node: &Node) -> Result<Written, Error> {
        if self.is_unmigrated() {
            let unmigrated = Error::Unmigrated {
                root: self.root.clone(),
                found_above: self.found_above.clone(),
            };
            return Err(unmigrated);
        }
        let node_changed = self.write_node_file(file, node)?;
        let location = self.location_of(file)?;
        let mut refs = self.load_refs()?;
        refs.set(location, node.refs.clone());
        let refs_changed = self.write_refs(&refs)?;
        let written = Written {
            node: node_changed,
            refs: refs_changed,
        };
        Ok(written)
    }

    /// Writes a node file in canonical form, without its refs; `true` when its bytes changed.
    pub fn write_node_file(&self, file: &Path, node: &Node) -> Result<bool, Error> {
        write_if_changed(file, &node.canonical_json())
    }

    /// Writes a node file of a tree from before v0.7 in place, refs and all: the one write such a
    /// tree takes (`classify`, readying its oldest files for `migrate`).
    pub fn write_legacy_node_file(&self, file: &Path, node: &Node) -> Result<bool, Error> {
        write_if_changed(file, &node.canonical_json_with_refs())
    }

    /// Writes the tree's `NODE.refs.json` in canonical form — or removes it, once the tree cites
    /// nothing. `true` when the file changed.
    pub fn write_refs(&self, refs: &TreeRefs) -> Result<bool, Error> {
        let file = self.refs_file();
        if !refs.is_empty() {
            return write_if_changed(&file, &refs.canonical_json());
        }
        refuse_symlink(&file)?;
        match fs::remove_file(&file) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(Error::io(&file, source)),
        }
    }
}

/// A file's text, or `None` when there is no such file.
fn read_if_present(file: &Path) -> Result<Option<String>, Error> {
    match fs::read_to_string(file) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::io(file, source)),
    }
}

/// Writes `text` to `file` unless it already holds it; `true` when it did. The text goes to a
/// hidden file beside it first, then is renamed into place, so a concurrent reader sees the old
/// file or the new one, never half of one. A symlink is refused rather than followed.
fn write_if_changed(file: &Path, text: &str) -> Result<bool, Error> {
    refuse_symlink(file)?;
    let current = read_if_present(file)?;
    if current.as_deref() == Some(text) {
        return Ok(false);
    }
    let name = file
        .file_name()
        .map(OsStr::to_string_lossy)
        .unwrap_or_default();
    let temporary = file.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    fs::write(&temporary, text).map_err(|source| Error::io(&temporary, source))?;
    fs::rename(&temporary, file).map_err(|source| {
        let _ = fs::remove_file(&temporary);
        Error::io(file, source)
    })?;
    Ok(true)
}

/// `Symlink` when `file` is a symbolic link: a write would land wherever it points.
fn refuse_symlink(file: &Path) -> Result<(), Error> {
    let is_symlink = fs::symlink_metadata(file).is_ok_and(|metadata| metadata.is_symlink());
    if is_symlink {
        let symlink = Error::Symlink {
            path: file.to_path_buf(),
        };
        return Err(symlink);
    }
    Ok(())
}
