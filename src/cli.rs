//! The command line: argument parsing and one function per command.

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
use crate::schema::{ChartedDate, Field, NODE_FILE, Node, NodePath, PageId, Ref};
use crate::tree::NodeTree;

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
    /// The whole node tree, drawn: each node's name (relative to its parent node) + `short`.
    Tree,
    /// Every node, one line each: path + `short`.
    Ls,
    /// Root → … → node: the nodes a reader loads to understand a path.
    Chain {
        /// A repo-relative directory (or a file in it).
        path: String,
    },
    /// One node, or one field of it.
    Get {
        path: String,
        /// path, charted, short, is, conventions, entry_points, fs, refs or notes.
        field: Option<Field>,
    },
    /// Every node citing a page.
    Refs { page: PageId },
    /// Case-insensitive search over `short`, `is`, `fs[].role`, `conventions`, `notes`, `refs[].title` and `refs[].governs`.
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
    /// Set `charted` to today (UTC), or to --date.
    Touch {
        path: String,
        #[arg(long, value_name = "YYYY-MM-DD")]
        date: Option<ChartedDate>,
    },
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
    fn emit(self, value: &impl Serialize, text: &str) -> ExitCode {
        if self.json {
            let rendered = serde_json::to_string_pretty(value).expect("output values serialize");
            println!("{rendered}");
        } else {
            print!("{text}");
        }
        ExitCode::SUCCESS
    }

    /// After a write: silent in text mode, the updated node in JSON mode.
    fn written(self, node: &Node) -> ExitCode {
        if self.json {
            let canonical = node.clone().canonical();
            let rendered =
                serde_json::to_string_pretty(&canonical).expect("a Node always serializes");
            println!("{rendered}");
        }
        ExitCode::SUCCESS
    }
}

/// Runs one command; the exit code is `1` for a failed `check`, `0` otherwise.
pub fn run(cli: Cli) -> Result<ExitCode, Error> {
    let repo = open_repo(cli.root)?;
    let output = Output { json: cli.json };
    match cli.command {
        Command::Tree => tree(&repo, output),
        Command::Ls => ls(&repo, output),
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

fn load_tree(repo: &Repo) -> Result<NodeTree, Error> {
    let nodes = repo.read_all()?;
    Ok(NodeTree::from_nodes(nodes))
}

/// The node at a typed path, or `UnknownNode`.
fn read_node(repo: &Repo, input: &str) -> Result<LoadedNode, Error> {
    let path = repo.node_path(input)?;
    let file = repo.node_file(&path);
    if !file.is_file() {
        return Err(Error::UnknownNode(path));
    }
    repo.read(&file)
}

fn tree(repo: &Repo, output: Output) -> Result<ExitCode, Error> {
    let tree = load_tree(repo)?;
    let root = tree
        .root()
        .ok_or_else(|| Error::UnknownNode(NodePath::root()))?;
    let value = subtree_json(&tree, root);
    Ok(output.emit(&value, &render::tree_text(&tree)))
}

fn subtree_json(tree: &NodeTree, node: &LoadedNode) -> Value {
    let children: Vec<Value> = tree
        .children(&node.location)
        .into_iter()
        .map(|child| subtree_json(tree, child))
        .collect();
    json!({ "path": node.location, "short": node.node.short, "is": node.node.is, "children": children })
}

fn ls(repo: &Repo, output: Output) -> Result<ExitCode, Error> {
    let tree = load_tree(repo)?;
    let value: Vec<Value> = tree
        .iter()
        .map(|node| {
            json!({ "path": node.location, "charted": node.node.charted, "short": node.node.short, "is": node.node.is })
        })
        .collect();
    Ok(output.emit(&value, &render::ls_text(&tree)))
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
    Ok(output.emit(&value, &text))
}

fn get(repo: &Repo, input: &str, field: Option<Field>, output: Output) -> Result<ExitCode, Error> {
    let loaded = read_node(repo, input)?;
    let Some(field) = field else {
        return Ok(output.emit(&loaded.node, &render::node_text(&loaded.node)));
    };
    let value = loaded.node.field(field);
    Ok(output.emit(&value, &render::field_text(&loaded.node, field)))
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
    Ok(output.emit(&value, &text))
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
    Ok(output.emit(&hits, &text))
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

/// One file `fmt` visited.
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
    Ok(output.emit(&visited, &text))
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
    text.push_str(&format!(
        "{} nodes, {} errors, {} warnings\n",
        report.nodes, report.errors, report.warnings
    ));
    output.emit(&report, &text);
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
    let loaded = read_node(repo, input)?;
    let updated = loaded
        .node
        .with_field(field, value)
        .map_err(|source| Error::InvalidValue { field, source })?;
    repo.write(&loaded.file, &updated)?;
    Ok(output.written(&updated))
}

fn add_ref(
    repo: &Repo,
    input: &str,
    page: PageId,
    title: String,
    governs: String,
    output: Output,
) -> Result<ExitCode, Error> {
    let mut loaded = read_node(repo, input)?;
    loaded.node.refs.retain(|reference| reference.page != page);
    let added = Ref {
        page,
        title,
        governs,
    };
    loaded.node.refs.push(added);
    repo.write(&loaded.file, &loaded.node)?;
    Ok(output.written(&loaded.node))
}

fn rm_ref(repo: &Repo, input: &str, page: PageId, output: Output) -> Result<ExitCode, Error> {
    let mut loaded = read_node(repo, input)?;
    let before = loaded.node.refs.len();
    loaded.node.refs.retain(|reference| reference.page != page);
    if loaded.node.refs.len() == before {
        return Err(Error::RefNotCited {
            path: loaded.location,
            page,
        });
    }
    repo.write(&loaded.file, &loaded.node)?;
    Ok(output.written(&loaded.node))
}

fn touch(
    repo: &Repo,
    input: &str,
    date: Option<ChartedDate>,
    output: Output,
) -> Result<ExitCode, Error> {
    let mut loaded = read_node(repo, input)?;
    loaded.node.charted = date.unwrap_or_else(ChartedDate::today_utc);
    repo.write(&loaded.file, &loaded.node)?;
    Ok(output.written(&loaded.node))
}
