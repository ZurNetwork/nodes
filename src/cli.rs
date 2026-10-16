//! The command line: argument parsing and one function per command.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write as _};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};

use crate::check::{self, CheckOptions};
use crate::date::ModifiedTime;
use crate::error::Error;
use crate::fuzzy::{self, Score};
use crate::grep::{self, Needle, Searched};
use crate::refindex::DEFAULT_SUPERSEDED_MARKER;
use crate::render;
use crate::render::note;
use crate::repo::{Citation, Content, LoadedNode, Repo};
use crate::schema::{
    ChartedDate, Field, LEGACY_NODE_FILE, MigratingNode, NODE_FILE, Node, NodePath, NodeType,
    PageId, REFS_FILE, Ref, Tag, Tags,
};
use crate::tag::MalformedTag;
use crate::tree::{NodeTree, Pruned};
use crate::treerefs::TreeRefs;

/// NODE.inf.json normalizer and lookup.
#[derive(Debug, Parser)]
#[command(name = "nodes", version, about)]
pub struct Cli {
    /// Emit JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    /// Repository root (default: the nearest ancestor of the current directory holding a root NODE.inf.json;
    /// for `init`, the current directory).
    #[arg(long, global = true, value_name = "DIR")]
    pub root: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(flatten)]
    InTree(TreeCommand),
    /// Make this directory (or --root) a tree's root: add `**/*.inf.json` to its `.gitignore`
    /// and, given a type and categories, seed its root NODE.inf.json. Safe to run again: an
    /// existing node is left as it is.
    Init(InitArgs),
    /// The closed vocabulary of `type`, each term with its meaning.
    Vocabulary,
}

/// The commands that work on a tree, found from the current directory or given by --root.
#[derive(Debug, Subcommand)]
pub enum TreeCommand {
    /// The whole node tree, drawn: each node's name (relative to its parent node) + `short`;
    /// a mounted tree's root is tagged `[mount]`. Filtered, it keeps the matches and the
    /// ancestors that lead to them (drawn bare).
    Tree(Selection),
    /// Every node, one line each: path + `short`.
    Ls(Selection),
    /// Root → … → node: the nodes a reader loads to understand a path.
    Chain {
        /// A repo-relative directory (or a file in it).
        path: String,
    },
    /// One node, or one field of it.
    Get {
        path: String,
        /// path, charted, short, is, type, tags, conventions, entry_points, fs, refs or notes.
        field: Option<Field>,
    },
    /// Every node citing a page — read from NODE.refs.json, so a checkout with no node tree answers too.
    Refs { page: PageId },
    /// Case-insensitive search over `short`, `is`, `type`, `tags`, `fs[].role`, `conventions`, `notes`, `refs[].title` and `refs[].governs`.
    Find { term: String },
    /// Case-insensitive full-text search over what the tree holds — `find` searches the chart,
    /// `grep` the text files it charts: path (the owning node), file, line and the line's text.
    /// Ignored, hidden and binary files are passed by; the chart's own files are `find`'s.
    Grep {
        term: Needle,
        /// Stop after this many hits (default: every hit).
        #[arg(long, value_name = "N")]
        limit: Option<Limit>,
    },
    /// The files modified most recently, newest first: modification time (UTC) + file.
    Recent {
        /// A repo-relative directory (default: the root).
        path: Option<String>,
        /// How many files to list.
        #[arg(long, value_name = "N", default_value = "10")]
        limit: Limit,
    },
    /// The nodes and files a partial or slightly-off name most likely means, best first.
    Resolve {
        query: String,
        /// How many candidates to list.
        #[arg(long, value_name = "N", default_value = "10")]
        limit: Limit,
    },
    /// Normalize NODE.inf.json files in place (default: every one in the repository, and its
    /// NODE.refs.json).
    Fmt {
        /// NODE.inf.json files or their directories, repo-relative or absolute.
        files: Vec<PathBuf>,
    },
    /// Validate every node; exit 1 on any error. With no node tree here (a fresh clone, CI), the
    /// tree's NODE.refs.json is checked alone.
    Check(CheckArgs),
    /// Replace one field with a JSON value (a value that is not JSON is taken as a string), then normalize.
    Set {
        path: String,
        field: Field,
        value: String,
    },
    /// Add a ref, or replace the one citing the same page (kept in NODE.refs.json).
    AddRef {
        path: String,
        page: PageId,
        title: String,
        governs: String,
    },
    /// Remove the ref citing a page.
    RmRef { path: String, page: PageId },
    /// Set `type` and the `category:` tags together — also on a file written before they existed,
    /// and the one write a tree from before v0.7 takes, so its oldest files can be migrated.
    /// The node's other tags are kept, after the categories.
    Classify {
        path: String,
        /// The one broad kind that fits best (see `nodes vocabulary`).
        node_type: NodeType,
        /// What the directory specifically holds, most fitting first: `ui` is tagged `category:ui`.
        #[arg(required = true, value_name = "CATEGORY", value_parser = category_tag)]
        categories: Vec<Tag>,
    },
    /// Bring a tree from an older release up to date: every `NODE.json` (up to v0.6) becomes a
    /// `NODE.inf.json`, every node's refs move into NODE.refs.json, ranked `categories` (up to
    /// v0.5) become `category:` tags, and the root's `.gitignore` gains `**/*.inf.json`. Also
    /// normalizes, like `fmt`; a second run changes nothing, and completes an interrupted one.
    Migrate {
        /// Leave `.gitignore` alone: for a repository that is not yours (keep node files out
        /// through its exclude file instead), or a tree outside any repository.
        #[arg(long)]
        no_gitignore: bool,
    },
    /// Set `charted` to today (UTC), or to --date.
    Touch {
        path: String,
        #[arg(long, value_name = "YYYY-MM-DD")]
        date: Option<ChartedDate>,
    },
}

impl TreeCommand {
    /// Whether a tree from before v0.7 refuses the command: every write but `migrate`, and
    /// `classify`, which readies its oldest files for `migrate`.
    fn waits_for_migrate(&self) -> bool {
        matches!(
            self,
            Self::Fmt { .. }
                | Self::Set { .. }
                | Self::AddRef { .. }
                | Self::RmRef { .. }
                | Self::Touch { .. }
        )
    }
}

/// What `init` is told: whether to leave `.gitignore` alone, and the root node to seed, if any.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Leave `.gitignore` alone: for a repository that is not yours (keep node files out through
    /// its exclude file instead), or a tree outside any repository.
    #[arg(long)]
    pub no_gitignore: bool,
    /// The root node's type (see `nodes vocabulary`); without it, no node is seeded.
    #[arg(requires = "categories")]
    pub node_type: Option<NodeType>,
    /// What the root specifically holds, most fitting first: `project` is tagged `category:project`.
    #[arg(value_name = "CATEGORY", value_parser = category_tag)]
    pub categories: Vec<Tag>,
}

/// Which nodes a listing keeps: by type, by tags, both, or — with neither — all.
#[derive(Debug, Args)]
pub struct Selection {
    /// Only nodes carrying this tag (`word` or `namespace:word`); repeat it to require several.
    #[arg(long = "tag", value_name = "TAG")]
    pub tags: Vec<Tag>,
    /// Only nodes carrying `category:CATEGORY` — short for `--tag category:CATEGORY`. `ls` lists
    /// them best fit first (by the category's rank among each node's categories).
    #[arg(long, value_name = "CATEGORY", value_parser = category_tag)]
    pub category: Option<Tag>,
    /// Only nodes of this type.
    #[arg(long = "type", value_name = "TYPE")]
    pub node_type: Option<NodeType>,
}

impl Selection {
    /// Every tag a node must carry, `--category` first.
    fn selected_tags(&self) -> impl Iterator<Item = &Tag> {
        self.category.iter().chain(&self.tags)
    }

    fn is_everything(&self) -> bool {
        self.node_type.is_none() && self.selected_tags().next().is_none()
    }

    fn admits(&self, node: &Node) -> bool {
        let type_fits = self
            .node_type
            .is_none_or(|node_type| node.node_type == node_type);
        let tags_fit = self.selected_tags().all(|tag| node.tags.carries(tag));
        type_fits && tags_fit
    }

    /// The category `ls` ranks by: the first one selected, however it was spelled.
    fn ranking_category(&self) -> Option<&Tag> {
        self.selected_tags().find(|tag| tag.is_category())
    }
}

/// A category word as the tag it stands for: `ui` is `category:ui`.
fn category_tag(word: &str) -> Result<Tag, MalformedTag> {
    Tag::category(word)
}

/// How many entries a listing is cut to: a whole number from 1 up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit(NonZeroUsize);

/// An argument that is not a [`Limit`].
#[derive(Debug, PartialEq, Eq)]
pub struct NotALimit(String);

impl Limit {
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

impl FromStr for Limit {
    type Err = NotALimit;

    fn from_str(text: &str) -> Result<Self, NotALimit> {
        text.parse::<NonZeroUsize>()
            .map(Self)
            .map_err(|_| NotALimit(text.to_owned()))
    }
}

impl fmt::Display for NotALimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a limit: a whole number from 1 up", self.0)
    }
}

impl std::error::Error for NotALimit {}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// A text file listing the citable page ids (a line's first run of digits is its page);
    /// without it the ref rules are skipped.
    #[arg(long, value_name = "FILE")]
    pub ref_index: Option<PathBuf>,
    /// The word marking an index entry superseded (its refs warn).
    #[arg(long, value_name = "TEXT", default_value = DEFAULT_SUPERSEDED_MARKER)]
    pub superseded_marker: String,
}

/// How a command speaks: text (the default) or JSON.
#[derive(Clone, Copy, Debug)]
struct Output {
    json: bool,
}

impl Output {
    fn emit(self, value: &impl Serialize, text: &str) -> Result<ExitCode, Error> {
        if self.json {
            let mut rendered =
                serde_json::to_string_pretty(value).expect("output values serialize");
            rendered.push('\n');
            print(&rendered)?;
        } else {
            print(text)?;
        }
        Ok(ExitCode::SUCCESS)
    }

    /// After a write: silent in text mode, the updated node in JSON mode.
    fn written(self, node: &Node) -> Result<ExitCode, Error> {
        if self.json {
            let canonical = node.clone().canonical();
            let mut rendered =
                serde_json::to_string_pretty(&canonical).expect("a Node always serializes");
            rendered.push('\n');
            print(&rendered)?;
        }
        Ok(ExitCode::SUCCESS)
    }
}

/// Writes to stdout. A reader that went away (`nodes tree | head`) closes the pipe; that ends the
/// output quietly instead of failing the run.
fn print(text: &str) -> Result<(), Error> {
    let mut stdout = io::stdout().lock();
    let written = stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush());
    match written {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(source) => Err(Error::io("<stdout>", source)),
    }
}

/// Runs one command; the exit code is `1` for a failed `check`, `0` otherwise.
pub fn run(cli: Cli) -> Result<ExitCode, Error> {
    let output = Output { json: cli.json };
    match cli.command {
        // The vocabularies belong to the tool, not to a tree: printing them needs no root.
        Command::Vocabulary => vocabulary(output),
        // `init` makes a root: it starts where it is told, never at an ancestor's tree.
        Command::Init(args) => {
            let dir = given_or_current_dir(cli.root)?;
            init(&Repo::at(dir), args, output)
        }
        Command::InTree(command) => run_in_tree(cli.root, command, output),
    }
}

fn run_in_tree(
    root: Option<PathBuf>,
    command: TreeCommand,
    output: Output,
) -> Result<ExitCode, Error> {
    let (repo, start) = open_repo(root)?;
    if command.waits_for_migrate() && repo.is_unmigrated() {
        let found_above = start.filter(|start| start != repo.root());
        let unmigrated = Error::Unmigrated {
            root: repo.root().to_path_buf(),
            found_above,
        };
        return Err(unmigrated);
    }
    match command {
        TreeCommand::Classify {
            path,
            node_type,
            categories,
        } => classify(&repo, &path, node_type, &categories, output),
        TreeCommand::Migrate { no_gitignore } => migrate(&repo, no_gitignore, output),
        TreeCommand::Tree(selection) => tree(&repo, &selection, output),
        TreeCommand::Ls(selection) => ls(&repo, &selection, output),
        TreeCommand::Chain { path } => chain(&repo, &path, output),
        TreeCommand::Get { path, field } => get(&repo, &path, field, output),
        TreeCommand::Refs { page } => refs(&repo, page, output),
        TreeCommand::Find { term } => find(&repo, &term, output),
        TreeCommand::Grep { term, limit } => grep(&repo, &term, limit, output),
        TreeCommand::Recent { path, limit } => recent(&repo, path.as_deref(), limit, output),
        TreeCommand::Resolve { query, limit } => resolve(&repo, &query, limit, output),
        TreeCommand::Fmt { files } => fmt(&repo, &files, output),
        TreeCommand::Check(args) => check(&repo, args, output),
        TreeCommand::Set { path, field, value } => set(&repo, &path, field, &value, output),
        TreeCommand::AddRef {
            path,
            page,
            title,
            governs,
        } => add_ref(&repo, &path, page, title, governs, output),
        TreeCommand::RmRef { path, page } => rm_ref(&repo, &path, page, output),
        TreeCommand::Touch { path, date } => touch(&repo, &path, date, output),
    }
}

fn current_dir() -> Result<PathBuf, Error> {
    std::env::current_dir().map_err(|source| Error::io(".", source))
}

fn canonical_dir(dir: &Path) -> Result<PathBuf, Error> {
    dir.canonicalize().map_err(|source| Error::io(dir, source))
}

/// The directory `init` makes a root: `--root` when given, otherwise the current directory.
fn given_or_current_dir(root: Option<PathBuf>) -> Result<PathBuf, Error> {
    let dir = match root {
        Some(root) => root,
        None => current_dir()?,
    };
    canonical_dir(&dir)
}

/// The tree a command works on — `--root`, or the nearest one above the current directory — and
/// where discovery started, when it did.
fn open_repo(root: Option<PathBuf>) -> Result<(Repo, Option<PathBuf>), Error> {
    let Some(root) = root else {
        let cwd = current_dir()?;
        let repo = Repo::discover(&cwd)?;
        return Ok((repo, Some(cwd)));
    };
    let repo = Repo::at(canonical_dir(&root)?);
    Ok((repo, None))
}

/// Why a checkout holds no node here: `NoNodeTree` when it holds only the tree's refs (a fresh
/// clone, a worktree, CI), otherwise `UnknownNode`.
fn missing_node(repo: &Repo, path: NodePath) -> Error {
    let holds_refs_alone =
        repo.refs_file().is_file() && !repo.node_file(&NodePath::root()).is_file();
    if holds_refs_alone {
        return Error::NoNodeTree {
            root: repo.root().to_path_buf(),
        };
    }
    Error::UnknownNode(path)
}

/// The tree a reader sees: this repository's nodes and every mounted tree's.
fn load_tree(repo: &Repo) -> Result<NodeTree, Error> {
    let nodes = repo.read_across_mounts()?;
    if nodes.is_empty() {
        return Err(missing_node(repo, NodePath::root()));
    }
    Ok(NodeTree::from_nodes(nodes))
}

/// The node at a typed path — this tree's or a mounted tree's — or `UnknownNode`.
fn read_node(repo: &Repo, input: &str) -> Result<LoadedNode, Error> {
    let path = repo.node_path(input)?;
    let Some(mount_point) = repo.mount_holding(&path) else {
        let file = repo.node_file(&path);
        if !file.is_file() {
            return Err(missing_node(repo, path));
        }
        repo.note_if_unmigrated();
        return repo.read(&file);
    };
    // A mounted tree keeps its nodes in its own era's files.
    let mounted = repo.mounted_tree(&mount_point);
    let below = mount_point
        .relative(&path)
        .filter(|below| !below.is_empty())
        .map_or_else(|| Ok(NodePath::root()), NodePath::try_from)
        .map_err(|reason| Error::InvalidPath {
            input: input.to_owned(),
            reason: reason.to_string(),
        })?;
    let file = mounted.node_file(&below);
    if !file.is_file() {
        return Err(Error::UnknownNode(path));
    }
    mounted.note_if_unmigrated();
    let mounted_node = mounted.read(&file)?;
    Ok(mounted_node.mounted_at(&mount_point))
}

/// The node at a typed path, for a write: a node of a mounted tree is refused.
fn read_own_node(repo: &Repo, input: &str) -> Result<LoadedNode, Error> {
    let file = own_node_file(repo, input)?;
    repo.read(&file)
}

/// The `NODE.inf.json` a write to a typed path lands in: this tree's own, and already there.
fn own_node_file(repo: &Repo, input: &str) -> Result<PathBuf, Error> {
    let path = repo.node_path(input)?;
    refuse_inside_mount(repo, &path)?;
    let file = repo.node_file(&path);
    if !file.is_file() {
        return Err(missing_node(repo, path));
    }
    Ok(file)
}

/// Writes never cross a mount: `InsideMount` when `path` belongs to a mounted tree.
fn refuse_inside_mount(repo: &Repo, path: &NodePath) -> Result<(), Error> {
    let Some(mount) = repo.mount_holding(path) else {
        return Ok(());
    };
    let inside_mount = Error::InsideMount {
        path: path.clone(),
        mount_dir: mount.to_dir(repo.root()),
        mount,
    };
    Err(inside_mount)
}

fn tree(repo: &Repo, selection: &Selection, output: Output) -> Result<ExitCode, Error> {
    let tree = load_tree(repo)?;
    let root = tree
        .root()
        .ok_or_else(|| Error::UnknownNode(NodePath::root()))?;
    let pruned = tree.pruned_to(|node| selection.admits(&node.node));
    let value = if pruned.draws(&root.location) {
        subtree_json(&tree, &pruned, selection, root)
    } else {
        Value::Null
    };
    output.emit(&value, &render::tree_text(&tree, &pruned))
}

/// One drawn node and everything drawn beneath it. A filtered tree says which entries `match`;
/// an unfiltered one keeps its shape.
fn subtree_json(
    tree: &NodeTree,
    pruned: &Pruned,
    selection: &Selection,
    node: &LoadedNode,
) -> Value {
    let children: Vec<Value> = tree
        .children(&node.location)
        .into_iter()
        .filter(|child| pruned.draws(&child.location))
        .map(|child| subtree_json(tree, pruned, selection, child))
        .collect();
    let mut entry = json!({ "path": node.location, "mount": node.is_mount, "type": node.node.node_type, "tags": node.node.tags, "short": node.node.short, "is": node.node.is, "children": children });
    if !selection.is_everything() {
        entry["match"] = json!(pruned.matches(&node.location));
    }
    entry
}

fn ls(repo: &Repo, selection: &Selection, output: Output) -> Result<ExitCode, Error> {
    let tree = load_tree(repo)?;
    let mut listed: Vec<&LoadedNode> = tree
        .iter()
        .filter(|node| selection.admits(&node.node))
        .collect();
    if let Some(category) = selection.ranking_category() {
        // A stable sort: nodes the category fits equally well keep their tree order.
        listed.sort_by_key(|node| node.node.tags.rank_of(category));
    }
    let value: Vec<Value> = listed
        .iter()
        .map(|node| {
            json!({ "path": node.location, "mount": node.is_mount, "type": node.node.node_type, "tags": node.node.tags, "charted": node.node.charted, "short": node.node.short, "is": node.node.is })
        })
        .collect();
    output.emit(&value, &render::ls_text(&listed))
}

fn chain(repo: &Repo, input: &str, output: Output) -> Result<ExitCode, Error> {
    let target = repo.node_path(input)?;
    let tree = load_tree(repo)?;
    let chain = tree.chain(&target);
    let value: Vec<&Node> = chain.iter().map(|node| &node.node).collect();
    let text = chain
        .iter()
        .map(|node| render::node_text(&node.node))
        .collect::<Vec<_>>()
        .join("\n");
    output.emit(&value, &text)
}

fn get(repo: &Repo, input: &str, field: Option<Field>, output: Output) -> Result<ExitCode, Error> {
    let loaded = read_node(repo, input)?;
    let Some(field) = field else {
        return output.emit(&loaded.node, &render::node_text(&loaded.node));
    };
    let value = loaded.node.field(field);
    output.emit(&value, &render::field_text(&loaded.node, field))
}

/// Every node citing `page`, read from the refs each tree keeps — so a checkout that holds only
/// `NODE.refs.json` answers too.
fn refs(repo: &Repo, page: PageId, output: Output) -> Result<ExitCode, Error> {
    let citations = repo.citations_across_mounts()?;
    let citing: Vec<&Citation> = citations
        .iter()
        .filter(|(_, reference)| reference.page == page)
        .collect();
    let value: Vec<Value> = citing
        .iter()
        .map(|(path, reference)| {
            json!({ "path": path, "title": reference.title, "governs": reference.governs })
        })
        .collect();
    let text: String = citing
        .iter()
        .map(|(path, reference)| format!("{path}  {} — {}\n", reference.title, reference.governs))
        .collect();
    output.emit(&value, &text)
}

/// One place a search term was found.
#[derive(Debug, Serialize)]
struct Hit {
    path: NodePath,
    field: String,
    text: String,
}

fn find(repo: &Repo, term: &str, output: Output) -> Result<ExitCode, Error> {
    let needle = term.to_lowercase();
    let tree = load_tree(repo)?;
    let hits: Vec<Hit> = tree
        .iter()
        .flat_map(|node| hits_in(node, &needle))
        .collect();
    let text: String = hits
        .iter()
        .map(|hit| format!("{}  {}: {}\n", hit.path, hit.field, hit.text))
        .collect();
    output.emit(&hits, &text)
}

fn hits_in(node: &LoadedNode, needle: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut consider = |field: String, text: &str| {
        if text.to_lowercase().contains(needle) {
            hits.push(Hit {
                path: node.location.clone(),
                field,
                text: text.to_owned(),
            });
        }
    };
    consider("short".to_owned(), &node.node.short);
    consider("is".to_owned(), &node.node.is);
    consider("type".to_owned(), node.node.node_type.word());
    for (index, tag) in node.node.tags.authored().iter().enumerate() {
        consider(format!("tags[{index}]"), tag.as_str());
    }
    for entry in &node.node.fs {
        consider(format!("fs[{}].role", entry.name), &entry.role);
    }
    for (index, convention) in node.node.conventions.iter().enumerate() {
        consider(format!("conventions[{index}]"), convention);
    }
    for (index, note) in node.node.notes.iter().enumerate() {
        consider(format!("notes[{index}]"), note);
    }
    for reference in &node.node.refs {
        consider(format!("refs[{}].title", reference.page), &reference.title);
        consider(
            format!("refs[{}].governs", reference.page),
            &reference.governs,
        );
    }
    hits
}

/// The directory a walk over content starts at: the root, or a typed path that is a directory.
fn content_start(repo: &Repo, input: Option<&str>) -> Result<PathBuf, Error> {
    let Some(input) = input else {
        return Ok(repo.root().to_path_buf());
    };
    let dir = repo.node_path(input)?.to_dir(repo.root());
    if !dir.is_dir() {
        let not_a_directory = Error::InvalidPath {
            input: input.to_owned(),
            reason: "not a directory".to_owned(),
        };
        return Err(not_a_directory);
    }
    Ok(dir)
}

/// The content at or beneath a typed path. A directory no walk reaches is refused: listing
/// nothing under an ignored path would read as "nothing there".
fn content_beneath(repo: &Repo, input: Option<&str>) -> Result<Content, Error> {
    let start = content_start(repo, input)?;
    let content = repo.content_across_mounts(&start)?;
    if !content.reached {
        let never_reached = Error::InvalidPath {
            input: input.unwrap_or(".").to_owned(),
            reason: "ignored: the walk never reaches it".to_owned(),
        };
        return Err(never_reached);
    }
    Ok(content)
}

/// A content file as output names it: relative to the root. `None` — and a note — for a name that
/// is not UTF-8: a lossy name is a path the caller cannot open.
fn content_name<'a>(repo: &Repo, file: &'a Path) -> Option<&'a str> {
    let relative = file.strip_prefix(repo.root()).unwrap_or(file);
    let name = relative.to_str();
    if name.is_none() {
        note(&format!(
            "passed over {}: its name is not UTF-8",
            relative.display()
        ));
    }
    name
}

/// The node that owns a content file: the deepest one above it — or, in a checkout with no node
/// tree, the root.
fn owner_of(repo: &Repo, content: &Content, file: &Path) -> Result<NodePath, Error> {
    let owner_dir = content.owner_of(file).unwrap_or(repo.root());
    repo.location_of(owner_dir)
}

/// One line of a file that holds the term `grep` looked for.
#[derive(Debug, Serialize)]
struct GrepHit {
    /// The node that owns the file.
    path: NodePath,
    file: String,
    line: usize,
    text: String,
}

fn grep(
    repo: &Repo,
    needle: &Needle,
    limit: Option<Limit>,
    output: Output,
) -> Result<ExitCode, Error> {
    let content = content_beneath(repo, None)?;
    let wanted = limit.map_or(usize::MAX, Limit::get);
    let mut hits: Vec<GrepHit> = Vec::new();
    for file in &content.files {
        if hits.len() >= wanted {
            break;
        }
        let Some(name) = content_name(repo, file) else {
            continue;
        };
        let lines = match grep::search_file(file, needle) {
            Ok(Searched::Text(lines)) => lines,
            Ok(Searched::Binary) => continue,
            Ok(Searched::TooLarge(bytes)) => {
                note(&format!(
                    "passed over {name}: {bytes} bytes, more than grep reads ({})",
                    grep::MAX_SEARCHED_BYTES
                ));
                continue;
            }
            Err(source) => {
                note(&format!("passed over {name}: {source}"));
                continue;
            }
        };
        if lines.is_empty() {
            continue;
        }
        let path = owner_of(repo, &content, file)?;
        let room = wanted - hits.len();
        let found = lines.into_iter().take(room).map(|hit| GrepHit {
            path: path.clone(),
            file: name.to_owned(),
            line: hit.line,
            text: hit.text,
        });
        hits.extend(found);
    }
    let text: String = hits
        .iter()
        .map(|hit| format!("{}  {}:{}: {}\n", hit.path, hit.file, hit.line, hit.text))
        .collect();
    output.emit(&hits, &text)
}

/// One recently modified file.
#[derive(Debug, Serialize)]
struct RecentFile {
    /// The node that owns the file.
    path: NodePath,
    file: String,
    modified: ModifiedTime,
}

fn recent(
    repo: &Repo,
    input: Option<&str>,
    limit: Limit,
    output: Output,
) -> Result<ExitCode, Error> {
    let content = content_beneath(repo, input)?;
    let mut dated: Vec<(ModifiedTime, &str, &Path)> = Vec::new();
    for file in &content.files {
        let Some(name) = content_name(repo, file) else {
            continue;
        };
        let modified = std::fs::metadata(file)
            .and_then(|metadata| metadata.modified())
            .map_err(|source| source.to_string())
            .and_then(|time| ModifiedTime::try_from(time).map_err(|range| range.to_string()));
        match modified {
            Ok(modified) => dated.push((modified, name, file)),
            Err(reason) => note(&format!("passed over {name}: {reason}")),
        }
    }
    // Newest first; files modified in the same second list by name.
    dated.sort_by(|(left_time, left_name, _), (right_time, right_name, _)| {
        right_time
            .cmp(left_time)
            .then_with(|| left_name.cmp(right_name))
    });
    dated.truncate(limit.get());
    let listed: Vec<RecentFile> = dated
        .into_iter()
        .map(|(modified, name, file)| {
            let path = owner_of(repo, &content, file)?;
            let recent_file = RecentFile {
                path,
                file: name.to_owned(),
                modified,
            };
            Ok(recent_file)
        })
        .collect::<Result<_, Error>>()?;
    let text: String = listed
        .iter()
        .map(|entry| format!("{}  {}\n", entry.modified, entry.file))
        .collect();
    output.emit(&listed, &text)
}

/// What a resolved path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CandidateKind {
    Node,
    File,
}

/// A path `resolve` may mean.
#[derive(Debug)]
struct Candidate {
    path: String,
    kind: CandidateKind,
}

impl AsRef<str> for Candidate {
    fn as_ref(&self) -> &str {
        &self.path
    }
}

/// One path a query most likely means.
#[derive(Debug, Serialize)]
struct Resolved {
    path: String,
    kind: CandidateKind,
    score: Score,
}

fn resolve(repo: &Repo, query: &str, limit: Limit, output: Output) -> Result<ExitCode, Error> {
    if query.trim_matches('/').is_empty() {
        let nothing_to_resolve = Error::InvalidArgument {
            what: "query",
            input: query.to_owned(),
            reason: "nothing to resolve".to_owned(),
        };
        return Err(nothing_to_resolve);
    }
    let content = content_beneath(repo, None)?;
    let mut candidates: Vec<Candidate> = Vec::new();
    for node_dir in content.node_dirs() {
        let location = repo.location_of(node_dir)?;
        candidates.push(Candidate {
            path: location.to_string(),
            kind: CandidateKind::Node,
        });
    }
    for file in &content.files {
        let Some(name) = content_name(repo, file) else {
            continue;
        };
        candidates.push(Candidate {
            path: name.to_owned(),
            kind: CandidateKind::File,
        });
    }
    let resolved: Vec<Resolved> = fuzzy::rank(query, candidates)
        .into_iter()
        .take(limit.get())
        .map(|(candidate, score)| Resolved {
            path: candidate.path,
            kind: candidate.kind,
            score,
        })
        .collect();
    let text: String = resolved
        .iter()
        .map(|entry| {
            let marker = match entry.kind {
                CandidateKind::Node => render::NODE_MARKER,
                CandidateKind::File => "",
            };
            format!("{}{marker}\n", entry.path)
        })
        .collect();
    output.emit(&resolved, &text)
}

/// One file `fmt` or `init` visited.
#[derive(Debug, Serialize)]
struct Formatted {
    file: String,
    changed: bool,
}

fn fmt(repo: &Repo, files: &[PathBuf], output: Output) -> Result<ExitCode, Error> {
    let mut visited = Vec::new();
    let targets = if files.is_empty() {
        // The refs first, so that each node's write below touches its own file alone.
        let refs = repo.load_refs()?;
        let refs_changed = repo.write_refs(&refs)?;
        if refs_changed || !refs.is_empty() {
            visited.push(Formatted {
                file: REFS_FILE.to_owned(),
                changed: refs_changed,
            });
        }
        repo.node_files()?
    } else {
        files.iter().map(|file| fmt_target(repo, file)).collect()
    };
    for file in &targets {
        let location = repo.location_of(file)?;
        refuse_inside_mount(repo, &location)?;
        let loaded = repo.read(file)?;
        let written = repo.write(file, &loaded.node)?;
        visited.push(Formatted {
            file: repo.display(file),
            changed: written.node,
        });
    }
    let text: String = visited
        .iter()
        .filter(|entry| entry.changed)
        .map(|entry| format!("fmt: {}\n", entry.file))
        .collect();
    output.emit(&visited, &text)
}

/// A `fmt` argument as the file to format: absolute as given, otherwise under the root; a directory means its NODE.inf.json.
fn fmt_target(repo: &Repo, given: &Path) -> PathBuf {
    let absolute = if given.is_absolute() {
        given.to_path_buf()
    } else {
        repo.root().join(given)
    };
    if absolute.is_dir() {
        absolute.join(NODE_FILE)
    } else {
        absolute
    }
}

fn check(repo: &Repo, args: CheckArgs, output: Output) -> Result<ExitCode, Error> {
    let options = CheckOptions {
        ref_index: args.ref_index.map(|file| fmt_target_file(repo, &file)),
        superseded_marker: args.superseded_marker,
    };
    let report = check::check(repo, &options)?;
    let mut text = String::new();
    for finding in &report.findings {
        text.push_str(&format!(
            "{}: {}: {}\n",
            finding.severity, finding.path, finding.message
        ));
    }
    let mounts = match report.mounts {
        0 => String::new(),
        count => format!(" {count} mounts,"),
    };
    let summary = if report.tree {
        format!(
            "{} nodes,{mounts} {} errors, {} warnings\n",
            report.nodes, report.errors, report.warnings
        )
    } else {
        format!(
            "refs only (no node tree here): {} refs, {} errors, {} warnings\n",
            report.refs, report.errors, report.warnings
        )
    };
    text.push_str(&summary);
    output.emit(&report, &text)?;
    let code = if report.is_clean() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };
    Ok(code)
}

/// A file argument: absolute as given, otherwise under the root.
fn fmt_target_file(repo: &Repo, given: &Path) -> PathBuf {
    if given.is_absolute() {
        given.to_path_buf()
    } else {
        repo.root().join(given)
    }
}

fn set(
    repo: &Repo,
    input: &str,
    field: Field,
    raw: &str,
    output: Output,
) -> Result<ExitCode, Error> {
    if field == Field::Path {
        return Err(Error::PathIsDerived);
    }
    let value: Value = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()));
    let loaded = read_own_node(repo, input)?;
    let updated = loaded
        .node
        .with_field(field, value)
        .map_err(|source| Error::InvalidValue { field, source })?;
    repo.write(&loaded.file, &updated)?;
    output.written(&updated)
}

fn add_ref(
    repo: &Repo,
    input: &str,
    page: PageId,
    title: String,
    governs: String,
    output: Output,
) -> Result<ExitCode, Error> {
    let mut loaded = read_own_node(repo, input)?;
    loaded.node.refs.retain(|reference| reference.page != page);
    let added = Ref {
        page,
        title,
        governs,
    };
    loaded.node.refs.push(added);
    repo.write(&loaded.file, &loaded.node)?;
    output.written(&loaded.node)
}

fn rm_ref(repo: &Repo, input: &str, page: PageId, output: Output) -> Result<ExitCode, Error> {
    let mut loaded = read_own_node(repo, input)?;
    let before = loaded.node.refs.len();
    loaded.node.refs.retain(|reference| reference.page != page);
    if loaded.node.refs.len() == before {
        return Err(Error::RefNotCited {
            path: loaded.location,
            page,
        });
    }
    repo.write(&loaded.file, &loaded.node)?;
    output.written(&loaded.node)
}

/// One term of a closed vocabulary, as `nodes vocabulary` lists it.
#[derive(Debug, Serialize)]
struct Term {
    term: &'static str,
    meaning: &'static str,
}

fn vocabulary(output: Output) -> Result<ExitCode, Error> {
    let types: Vec<Term> = NodeType::ALL
        .iter()
        .map(|node_type| Term {
            term: node_type.word(),
            meaning: node_type.meaning(),
        })
        .collect();
    let value = json!({ "type": types });
    output.emit(&value, &render::vocabulary_text())
}

fn classify(
    repo: &Repo,
    input: &str,
    node_type: NodeType,
    categories: &[Tag],
    output: Output,
) -> Result<ExitCode, Error> {
    let file = own_node_file(repo, input)?;
    let text = std::fs::read_to_string(&file).map_err(|source| Error::io(&file, source))?;
    let classified = Node::parse_classified(&text, node_type, categories)
        .map_err(|source| Error::Schema {
            file: file.clone(),
            source,
        })?
        .node;
    if repo.is_unmigrated() {
        repo.write_legacy_node_file(&file, &classified)?;
        return output.written(&classified);
    }
    let location = repo.location_of(&file)?;
    let refs = repo.load_refs()?.of(&location).to_vec();
    let whole = Node { refs, ..classified };
    repo.write(&file, &whole)?;
    output.written(&whole)
}

/// One file `migrate` visited.
#[derive(Debug, Serialize)]
struct Migrated {
    file: String,
    /// The `NODE.json` the node was moved out of, when it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    renamed_from: Option<String>,
    changed: bool,
}

/// What `migrate` does to one directory's node, decided before anything is written.
#[derive(Debug)]
enum MigrationStep {
    /// Move a `NODE.json` to a `NODE.inf.json` — one atomic rename — then rewrite it in place.
    Rename {
        from: PathBuf,
        to: PathBuf,
        node: Node,
    },
    /// Rewrite a `NODE.inf.json` in place: canonical, and without the refs an interrupted run left.
    Rewrite { file: PathBuf, node: Node },
    /// Drop a `NODE.json` that says what the `NODE.inf.json` beside it says — as a checkout of an
    /// older commit brings back — after rewriting the `NODE.inf.json`.
    DropDuplicate {
        stale: PathBuf,
        kept: PathBuf,
        node: Node,
    },
}

/// Brings this tree's own node files — `NODE.json` or `NODE.inf.json` — through the migration
/// door (see [`Node::parse_migrating`]) into `NODE.inf.json` files, their refs into
/// `NODE.refs.json`. Every file is read before any is written, so a file that does not fit stops
/// the run with nothing changed. Then `.gitignore` (unless told not to), then the refs — so no
/// ref lives only in memory — then each node. A run cut short is completed by the next. A mounted
/// tree is its own to migrate.
fn migrate(repo: &Repo, no_gitignore: bool, output: Output) -> Result<ExitCode, Error> {
    let found = repo.walk_every_era()?;
    if found.node_files.is_empty() && !repo.refs_file().is_file() {
        let no_tree = Error::NoTree {
            root: repo.root().to_path_buf(),
        };
        return Err(no_tree);
    }
    for mount_dir in &found.mounts {
        if Repo::is_unmigrated_dir(mount_dir) {
            note(&format!(
                "`{}`: a mounted tree from before v0.7, left to its own root: `nodes --root {} migrate`",
                repo.display(mount_dir),
                mount_dir.display()
            ));
        }
    }
    let mut refs = repo.load_refs()?;
    let steps = plan_migration(repo, &found.node_files, &mut refs)?;
    let mut visited: Vec<Migrated> = Vec::new();
    let finish = |visited: &mut Vec<Migrated>| -> Result<(), Error> {
        if !no_gitignore {
            visited.push(Migrated {
                file: ".gitignore".to_owned(),
                renamed_from: None,
                changed: repo.ignore_node_files()?,
            });
        }
        let refs_changed = repo.write_refs(&refs)?;
        if refs_changed || !refs.is_empty() {
            visited.push(Migrated {
                file: REFS_FILE.to_owned(),
                renamed_from: None,
                changed: refs_changed,
            });
        }
        for step in steps {
            visited.push(apply_migration_step(repo, step)?);
        }
        Ok(())
    };
    if let Err(stopped) = finish(&mut visited) {
        let done = visited.iter().filter(|entry| entry.changed).count();
        note(&format!(
            "migration stopped part-way, after {done} changed files: re-run `nodes migrate` to finish"
        ));
        return Err(stopped);
    }
    let text: String = visited
        .iter()
        .filter(|entry| entry.changed)
        .map(|entry| match &entry.renamed_from {
            Some(legacy) => format!("migrate: {legacy} → {}\n", entry.file),
            None => format!("migrate: {}\n", entry.file),
        })
        .collect();
    output.emit(&visited, &text)
}

/// Reads every node file and decides what `migrate` does to each directory, gathering the refs
/// the old files carried into `refs`. Writes nothing.
fn plan_migration(
    repo: &Repo,
    node_files: &[PathBuf],
    refs: &mut TreeRefs,
) -> Result<Vec<MigrationStep>, Error> {
    let mut by_dir: BTreeMap<&Path, (Option<&PathBuf>, Option<&PathBuf>)> = BTreeMap::new();
    for file in node_files {
        let dir = file.parent().unwrap_or(repo.root());
        let pair = by_dir.entry(dir).or_default();
        let is_legacy = file
            .file_name()
            .is_some_and(|name| name == LEGACY_NODE_FILE);
        if is_legacy {
            pair.0 = Some(file);
        } else {
            pair.1 = Some(file);
        }
    }
    let mut steps = Vec::new();
    for (dir, files) in by_dir {
        let location = repo.location_of(dir)?;
        let step = match files {
            (Some(legacy), None) => {
                let old = read_migrating(legacy)?;
                if old.carries_refs {
                    refs.set(location, old.node.refs.clone());
                }
                MigrationStep::Rename {
                    from: legacy.clone(),
                    to: dir.join(NODE_FILE),
                    node: old.node,
                }
            }
            (None, Some(current)) => {
                let kept = read_migrating(current)?;
                if kept.carries_refs {
                    refs.set(location, kept.node.refs.clone());
                }
                MigrationStep::Rewrite {
                    file: current.clone(),
                    node: kept.node,
                }
            }
            (Some(legacy), Some(current)) => {
                let old = read_migrating(legacy)?;
                let kept = read_migrating(current)?;
                let kept_refs = if kept.carries_refs {
                    kept.node.refs.clone()
                } else {
                    refs.of(&location).to_vec()
                };
                let kept_whole = Node {
                    refs: kept_refs,
                    ..kept.node
                };
                if old.node.canonical() != kept_whole.clone().canonical() {
                    let two_node_files = Error::TwoNodeFiles {
                        dir: dir.to_path_buf(),
                    };
                    return Err(two_node_files);
                }
                refs.set(location, kept_whole.refs.clone());
                MigrationStep::DropDuplicate {
                    stale: legacy.clone(),
                    kept: current.clone(),
                    node: kept_whole,
                }
            }
            (None, None) => continue,
        };
        steps.push(step);
    }
    Ok(steps)
}

fn read_migrating(file: &Path) -> Result<MigratingNode, Error> {
    let text = std::fs::read_to_string(file).map_err(|source| Error::io(file, source))?;
    Node::parse_migrating(&text).map_err(|source| Error::Schema {
        file: file.to_path_buf(),
        source,
    })
}

fn apply_migration_step(repo: &Repo, step: MigrationStep) -> Result<Migrated, Error> {
    let migrated = match step {
        MigrationStep::Rename { from, to, node } => {
            std::fs::rename(&from, &to).map_err(|source| Error::io(&from, source))?;
            repo.write_node_file(&to, &node)?;
            Migrated {
                file: repo.display(&to),
                renamed_from: Some(repo.display(&from)),
                changed: true,
            }
        }
        MigrationStep::Rewrite { file, node } => Migrated {
            changed: repo.write_node_file(&file, &node)?,
            file: repo.display(&file),
            renamed_from: None,
        },
        MigrationStep::DropDuplicate { stale, kept, node } => {
            repo.write_node_file(&kept, &node)?;
            std::fs::remove_file(&stale).map_err(|source| Error::io(&stale, source))?;
            Migrated {
                file: repo.display(&kept),
                renamed_from: Some(repo.display(&stale)),
                changed: true,
            }
        }
    };
    Ok(migrated)
}

/// Makes `repo`'s directory a tree's root: `.gitignore` first (unless told not to), so no node
/// file is ever seen untracked, then — given a type — the root node, unless one is already there.
fn init(repo: &Repo, args: InitArgs, output: Output) -> Result<ExitCode, Error> {
    if repo.is_unmigrated() {
        let unmigrated = Error::Unmigrated {
            root: repo.root().to_path_buf(),
            found_above: None,
        };
        return Err(unmigrated);
    }
    let mut visited = Vec::new();
    if !args.no_gitignore {
        visited.push(Formatted {
            file: ".gitignore".to_owned(),
            changed: repo.ignore_node_files()?,
        });
    }
    if let Some(node_type) = args.node_type {
        let seeded = seed_root(repo, node_type, args.categories)?;
        visited.push(seeded);
    }
    let text: String = visited
        .iter()
        .filter(|entry| entry.changed)
        .map(|entry| format!("init: {}\n", entry.file))
        .collect();
    output.emit(&visited, &text)
}

/// Writes the root node `init` seeds — `short` is the directory's name, `is` starts empty — unless
/// a root node is already there. A directory holding a node of a tree above is refused: it is no
/// root. Seeding beneath another tree makes a mount of it, and a note says so.
fn seed_root(repo: &Repo, node_type: NodeType, categories: Vec<Tag>) -> Result<Formatted, Error> {
    let file = repo.node_file(&NodePath::root());
    let name = repo.display(&file);
    if file.is_file() {
        let text = std::fs::read_to_string(&file).map_err(|source| Error::io(&file, source))?;
        let existing = Node::parse(&text).map_err(|source| Error::Schema {
            file: file.clone(),
            source,
        })?;
        if !existing.path.is_root() {
            let not_a_root = Error::NotARoot {
                dir: repo.root().to_path_buf(),
                path: existing.path,
            };
            return Err(not_a_root);
        }
        note(&format!("{name} is already there: left as it is"));
        let left_alone = Formatted {
            file: name,
            changed: false,
        };
        return Ok(left_alone);
    }
    let words: Vec<&str> = categories.iter().map(Tag::word).collect();
    let input = words.join(" ");
    let tags = Tags::try_from(categories).map_err(|reason| Error::InvalidArgument {
        what: "categories",
        input,
        reason: reason.to_string(),
    })?;
    let short = repo
        .root()
        .file_name()
        .and_then(|dir_name| dir_name.to_str())
        .unwrap_or(".")
        .to_owned();
    let root_node = Node {
        path: NodePath::root(),
        charted: ChartedDate::today_utc(),
        short,
        is: String::new(),
        node_type,
        tags,
        conventions: Vec::new(),
        entry_points: Vec::new(),
        fs: Vec::new(),
        refs: Vec::new(),
        notes: Vec::new(),
    };
    // The node file alone: a fresh clone's NODE.refs.json may already cite pages for the root.
    let changed = repo.write_node_file(&file, &root_node)?;
    let outer = repo
        .root()
        .parent()
        .and_then(|parent| Repo::discover(parent).ok());
    if let Some(outer) = outer {
        note(&format!(
            "{} is now a tree mounted in the one at {}: list it in the fs of its nearest node above, with node: true",
            repo.root().display(),
            outer.root().display()
        ));
    }
    let seeded = Formatted {
        file: name,
        changed,
    };
    Ok(seeded)
}

fn touch(
    repo: &Repo,
    input: &str,
    date: Option<ChartedDate>,
    output: Output,
) -> Result<ExitCode, Error> {
    let mut loaded = read_own_node(repo, input)?;
    loaded.node.charted = date.unwrap_or_else(ChartedDate::today_utc);
    repo.write(&loaded.file, &loaded.node)?;
    output.written(&loaded.node)
}
