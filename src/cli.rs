//! The command line: argument parsing and one function per command.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};

use crate::check::{self, CheckOptions};
use crate::error::Error;
use crate::refindex::DEFAULT_SUPERSEDED_MARKER;
use crate::render;
use crate::repo::{LoadedNode, Repo};
use crate::schema::{ChartedDate, Field, NODE_FILE, Node, NodePath, NodeType, PageId, Ref, Tag};
use crate::tag::MalformedTag;
use crate::tree::{NodeTree, Pruned};

/// NODE.json normalizer and lookup.
#[derive(Debug, Parser)]
#[command(name = "nodes", version, about)]
pub struct Cli {
    /// Emit JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    /// Repository root (default: the nearest ancestor of the current directory holding a root NODE.json).
    #[arg(long, global = true, value_name = "DIR")]
    pub root: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
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
    /// Every node citing a page.
    Refs { page: PageId },
    /// Case-insensitive search over `short`, `is`, `type`, `tags`, `fs[].role`, `conventions`, `notes`, `refs[].title` and `refs[].governs`.
    Find { term: String },
    /// Normalize NODE.json files in place (default: every one in the repository).
    Fmt {
        /// NODE.json files or their directories, repo-relative or absolute.
        files: Vec<PathBuf>,
    },
    /// Validate every node; exit 1 on any error.
    Check(CheckArgs),
    /// Replace one field with a JSON value (a value that is not JSON is taken as a string), then normalize.
    Set {
        path: String,
        field: Field,
        value: String,
    },
    /// Add a ref, or replace the one citing the same page.
    AddRef {
        path: String,
        page: PageId,
        title: String,
        governs: String,
    },
    /// Remove the ref citing a page.
    RmRef { path: String, page: PageId },
    /// Set `type` and the `category:` tags together — also on a file written before they existed.
    /// The node's other tags are kept, after the categories.
    Classify {
        path: String,
        /// The one broad kind that fits best (see `nodes vocabulary`).
        node_type: NodeType,
        /// What the directory specifically holds, most fitting first: `ui` is tagged `category:ui`.
        #[arg(required = true, value_name = "CATEGORY", value_parser = category_tag)]
        categories: Vec<Tag>,
    },
    /// Rewrite every node written before `tags` existed (up to v0.5): its ranked `categories`
    /// become `category:` tags. Also normalizes, like `fmt`; a second run changes nothing.
    Migrate,
    /// The closed vocabulary of `type`, each term with its meaning.
    Vocabulary,
    /// Set `charted` to today (UTC), or to --date.
    Touch {
        path: String,
        #[arg(long, value_name = "YYYY-MM-DD")]
        date: Option<ChartedDate>,
    },
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
    // The vocabularies belong to the tool, not to a tree: printing them needs no root.
    if matches!(cli.command, Command::Vocabulary) {
        return vocabulary(output);
    }
    let repo = open_repo(cli.root)?;
    match cli.command {
        Command::Vocabulary => vocabulary(output),
        Command::Classify {
            path,
            node_type,
            categories,
        } => classify(&repo, &path, node_type, &categories, output),
        Command::Migrate => migrate(&repo, output),
        Command::Tree(selection) => tree(&repo, &selection, output),
        Command::Ls(selection) => ls(&repo, &selection, output),
        Command::Chain { path } => chain(&repo, &path, output),
        Command::Get { path, field } => get(&repo, &path, field, output),
        Command::Refs { page } => refs(&repo, page, output),
        Command::Find { term } => find(&repo, &term, output),
        Command::Fmt { files } => fmt(&repo, &files, output),
        Command::Check(args) => check(&repo, args, output),
        Command::Set { path, field, value } => set(&repo, &path, field, &value, output),
        Command::AddRef {
            path,
            page,
            title,
            governs,
        } => add_ref(&repo, &path, page, title, governs, output),
        Command::RmRef { path, page } => rm_ref(&repo, &path, page, output),
        Command::Touch { path, date } => touch(&repo, &path, date, output),
    }
}

fn open_repo(root: Option<PathBuf>) -> Result<Repo, Error> {
    let Some(root) = root else {
        let cwd = std::env::current_dir().map_err(|source| Error::io(".", source))?;
        return Repo::discover(&cwd);
    };
    let canonical = root
        .canonicalize()
        .map_err(|source| Error::io(&root, source))?;
    Ok(Repo::at(canonical))
}

/// The tree a reader sees: this repository's nodes and every mounted tree's.
fn load_tree(repo: &Repo) -> Result<NodeTree, Error> {
    let nodes = repo.read_across_mounts()?;
    Ok(NodeTree::from_nodes(nodes))
}

/// The node at a typed path — this tree's or a mounted tree's — or `UnknownNode`.
fn read_node(repo: &Repo, input: &str) -> Result<LoadedNode, Error> {
    let path = repo.node_path(input)?;
    let file = repo.node_file(&path);
    if !file.is_file() {
        return Err(Error::UnknownNode(path));
    }
    let Some(mount_point) = repo.mount_holding(&path) else {
        return repo.read(&file);
    };
    let mounted_node = repo.mounted_tree(&mount_point).read(&file)?;
    Ok(mounted_node.mounted_at(&mount_point))
}

/// The node at a typed path, for a write: a node of a mounted tree is refused.
fn read_own_node(repo: &Repo, input: &str) -> Result<LoadedNode, Error> {
    let file = own_node_file(repo, input)?;
    repo.read(&file)
}

/// The `NODE.json` a write to a typed path lands in: this tree's own, and already there.
fn own_node_file(repo: &Repo, input: &str) -> Result<PathBuf, Error> {
    let path = repo.node_path(input)?;
    refuse_inside_mount(repo, &path)?;
    let file = repo.node_file(&path);
    if !file.is_file() {
        return Err(Error::UnknownNode(path));
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

fn refs(repo: &Repo, page: PageId, output: Output) -> Result<ExitCode, Error> {
    let tree = load_tree(repo)?;
    let citing = tree.citing(page);
    let value: Vec<Value> = citing
        .iter()
        .map(|(node, reference)| {
            json!({ "path": node.location, "title": reference.title, "governs": reference.governs })
        })
        .collect();
    let text: String = citing
        .iter()
        .map(|(node, reference)| {
            format!(
                "{}  {} — {}\n",
                node.location, reference.title, reference.governs
            )
        })
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

/// One file `fmt` or `migrate` visited.
#[derive(Debug, Serialize)]
struct Formatted {
    file: String,
    changed: bool,
}

fn fmt(repo: &Repo, files: &[PathBuf], output: Output) -> Result<ExitCode, Error> {
    let targets = if files.is_empty() {
        repo.node_files()?
    } else {
        files.iter().map(|file| fmt_target(repo, file)).collect()
    };
    let mut visited = Vec::new();
    for file in &targets {
        let location = repo.location_of(file)?;
        refuse_inside_mount(repo, &location)?;
        let loaded = repo.read(file)?;
        let changed = repo.write(file, &loaded.node)?;
        visited.push(Formatted {
            file: repo.display(file),
            changed,
        });
    }
    let text: String = visited
        .iter()
        .filter(|entry| entry.changed)
        .map(|entry| format!("fmt: {}\n", entry.file))
        .collect();
    output.emit(&visited, &text)
}

/// A `fmt` argument as the file to format: absolute as given, otherwise under the root; a directory means its NODE.json.
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
    text.push_str(&format!(
        "{} nodes,{mounts} {} errors, {} warnings\n",
        report.nodes, report.errors, report.warnings
    ));
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
    let classified =
        Node::parse_classified(&text, node_type, categories).map_err(|source| Error::Schema {
            file: file.clone(),
            source,
        })?;
    repo.write(&file, &classified)?;
    output.written(&classified)
}

/// Walks this tree's own node files through the migration door (see [`Node::parse_migrating`]);
/// a mounted tree is its own to migrate.
fn migrate(repo: &Repo, output: Output) -> Result<ExitCode, Error> {
    let mut visited = Vec::new();
    for file in &repo.node_files()? {
        let text = std::fs::read_to_string(file).map_err(|source| Error::io(file, source))?;
        let migrated = Node::parse_migrating(&text).map_err(|source| Error::Schema {
            file: file.clone(),
            source,
        })?;
        let changed = repo.write(file, &migrated)?;
        visited.push(Formatted {
            file: repo.display(file),
            changed,
        });
    }
    let text: String = visited
        .iter()
        .filter(|entry| entry.changed)
        .map(|entry| format!("migrate: {}\n", entry.file))
        .collect();
    output.emit(&visited, &text)
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
