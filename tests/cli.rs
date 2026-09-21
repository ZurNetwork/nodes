//! End-to-end tests: the `nodes` binary over a throwaway charted repository.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A charted repository in a temp directory, removed on drop.
struct TempRepo {
    root: PathBuf,
}

impl TempRepo {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("nodes-cli-test-{}-{unique}", std::process::id()));
        fs::create_dir_all(&root).expect("temp dir");
        // The binary canonicalizes `--root`, and macOS keeps its temp dir behind a symlink
        // (`/var` → `/private/var`): hold the canonical root, so an expected path is the printed one.
        let root = root.canonicalize().expect("a canonical temp dir");
        Self { root }
    }

    /// The fixture every test starts from: root → backend → backend/crates, plus frontend and a ref index.
    fn charted() -> Self {
        let repo = Self::new();
        let root = node(
            ".",
            "The whole repository.",
            &[
                ("backend/", "the Rust workspace", true),
                (
                    "backend/crates/",
                    "the members, charted two levels down",
                    true,
                ),
                ("docs/", "pointers", false),
                ("frontend/", "the web app", true),
            ],
            &[],
            &["Docstrings carry no design pointers; refs do."],
        );
        repo.write("NODE.json", &root);
        let backend = node(
            "backend",
            "The backend.",
            &[("crates/", "workspace members", true)],
            &[],
            &[],
        );
        repo.write("backend/NODE.json", &backend);
        let crates = node(
            "backend/crates",
            "Twelve crates in a hexagon.",
            &[("api/", "the HTTP driver", false)],
            &[
                (
                    11_763_713,
                    "Domains and Applications",
                    "the dependency rule",
                ),
                (55_836_674, "The Application Layer", "use cases"),
            ],
            &["Adapters depend on domain, never the reverse."],
        );
        repo.write("backend/crates/NODE.json", &crates);
        let frontend = node(
            "frontend",
            "The client tier.",
            &[],
            &[(39_944_194, "Frontend Stack", "SvelteKit")],
            &[],
        );
        repo.write("frontend/NODE.json", &frontend);
        repo.write(
            "docs/index.md",
            "# Index built 2026-06-30\n- `11763713` — Domains and Applications\n- `55836674` — The Application Layer\n- `39944194` — Frontend Stack (SUPERSEDED IN PART by 47939586)\n",
        );
        repo
    }

    /// A root that mounts two other trees: `life` (which itself mounts `life/archive`) and, beneath
    /// the pass-through `code/`, `code/zurfur`.
    fn mounting() -> Self {
        let repo = Self::new();
        let home = node(
            ".",
            "Home.",
            &[
                ("code/zurfur/", "the monorepo — own tree", true),
                ("life/", "documents — own tree", true),
            ],
            &[],
            &[],
        );
        repo.write("NODE.json", &home);
        let life = node(
            ".",
            "Personal documents.",
            &[
                ("archive/", "closed years — own tree", true),
                ("housing/", "where we live", true),
            ],
            &[],
            &[],
        );
        repo.write("life/NODE.json", &life);
        let housing = node(
            "housing",
            "Lease and utilities.",
            &[],
            &[(7, "The Lease", "the tenancy")],
            &[],
        );
        repo.write("life/housing/NODE.json", &housing);
        let archive = node(
            ".",
            "Closed years.",
            &[("2019/", "that year", true)],
            &[],
            &[],
        );
        repo.write("life/archive/NODE.json", &archive);
        let year = node("2019", "Everything from 2019.", &[], &[], &[]);
        repo.write("life/archive/2019/NODE.json", &year);
        let zurfur = node(".", "The Zurfur monorepo.", &[], &[], &[]);
        repo.write("code/zurfur/NODE.json", &zurfur);
        repo
    }

    fn write(&self, relative: &str, text: &str) {
        let file = self.root.join(relative);
        let parent = file.parent().expect("a parent");
        fs::create_dir_all(parent).expect("parent dirs");
        fs::write(file, text).expect("write");
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.root.join(relative)).expect("read")
    }

    fn nodes(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nodes"))
            .arg("--root")
            .arg(&self.root)
            .args(args)
            .output()
            .expect("the binary runs")
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.nodes(args);
        assert!(
            output.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8 stdout")
    }

    fn fails(&self, args: &[&str]) -> (String, String) {
        let output = self.nodes(args);
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
        let stderr = String::from_utf8(output.stderr).expect("utf-8 stderr");
        (stdout, stderr)
    }

    /// Pins a file's modification time, in whole seconds since the epoch.
    fn modified_at(&self, relative: &str, seconds_since_epoch: u64) {
        let file = fs::File::options()
            .write(true)
            .open(self.root.join(relative))
            .expect("opens for writing");
        let instant = UNIX_EPOCH + Duration::from_secs(seconds_since_epoch);
        file.set_modified(instant)
            .expect("sets the modification time");
    }

    fn index_arg(&self) -> String {
        self.root.join("docs/index.md").display().to_string()
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A canonical node file's text.
fn node(
    path: &str,
    is: &str,
    fs: &[(&str, &str, bool)],
    refs: &[(u64, &str, &str)],
    notes: &[&str],
) -> String {
    let fs: Vec<Value> = fs
        .iter()
        .map(|(name, role, node)| json!({"name": name, "role": role, "node": node}))
        .collect();
    let refs: Vec<Value> = refs
        .iter()
        .map(|(page, title, governs)| json!({"page": page, "title": title, "governs": governs}))
        .collect();
    let value = json!({
        "path": path,
        "charted": "2026-09-12",
        "short": is.trim_end_matches('.'),
        "is": is,
        "type": "code",
        "tags": ["category:source"],
        "conventions": [],
        "entry_points": [],
        "fs": fs,
        "refs": refs,
        "notes": notes,
    });
    nodes::schema::Node::parse(&value.to_string())
        .expect("a valid node")
        .canonical_json()
}

/// The same node file, classified otherwise: another type, other tags.
fn classified(text: &str, node_type: &str, tags: &[&str]) -> String {
    let mut value: Value = serde_json::from_str(text).expect("json");
    value["type"] = json!(node_type);
    value["tags"] = json!(tags);
    nodes::schema::Node::parse(&value.to_string())
        .expect("a valid node")
        .canonical_json()
}

#[test]
fn fmt_normalizes_once_and_is_then_a_no_op() {
    let repo = TempRepo::charted();
    let messy = r#"{"notes": [], "short": "Messy", "refs": [{"governs": "b", "title": "B", "page": 20}, {"page": 10, "title": "A", "governs": "a"}],
        "fs": [{"name": "z/", "role": "last", "node": false}, {"name": "a/", "role": "first", "node": false}],
        "entry_points": ["x"], "conventions": ["c"], "tags": ["category:tests", "wip", "category:source"], "type": "code", "is": "Messy.", "charted": "2026-01-02", "path": "backend/crates"}"#;
    repo.write("backend/crates/NODE.json", messy);
    let first = repo.ok(&["fmt"]);
    assert_eq!(first, "fmt: backend/crates/NODE.json\n");
    let formatted = repo.read("backend/crates/NODE.json");
    let expected = nodes::schema::Node::parse(messy)
        .expect("valid")
        .canonical_json();
    assert_eq!(formatted, expected);
    let canonical_head = "{\n  \"path\": \"backend/crates\",\n  \"charted\": \"2026-01-02\",\n  \"short\": \"Messy\",\n  \"is\": \"Messy.\",\n  \"type\": \"code\",\n  \"tags\": [\n    \"category:tests\",\n    \"wip\",\n    \"category:source\"\n  ],\n";
    assert!(formatted.starts_with(canonical_head), "{formatted}");
    let a_before_z = formatted.find("\"a/\"").expect("a/") < formatted.find("\"z/\"").expect("z/");
    assert!(a_before_z, "fs sorted by name");
    let ten_before_twenty =
        formatted.find("\"page\": 10").expect("10") < formatted.find("\"page\": 20").expect("20");
    assert!(ten_before_twenty, "refs sorted by page");
    let second = repo.ok(&["fmt"]);
    assert_eq!(second, "", "a second fmt changes nothing");
    assert_eq!(repo.read("backend/crates/NODE.json"), formatted);
}

#[test]
fn fmt_takes_files_or_directories_and_refuses_unknown_keys() {
    let repo = TempRepo::charted();
    repo.write(
        "frontend/NODE.json",
        &node("frontend", "The client tier.", &[], &[], &[]).replace("  \"is\"", "\"is\""),
    );
    let by_dir = repo.ok(&["fmt", "frontend"]);
    assert_eq!(by_dir, "fmt: frontend/NODE.json\n");
    let json_report = repo.ok(&["--json", "fmt", "frontend/NODE.json"]);
    let parsed: Value = serde_json::from_str(&json_report).expect("json");
    assert_eq!(
        parsed,
        json!([{"file": "frontend/NODE.json", "changed": false}])
    );
    let with_extra = node("frontend", "x", &[], &[], &[])
        .replace("\"notes\": []", "\"notes\": [], \"extra\": 1");
    repo.write("frontend/NODE.json", &with_extra);
    let (_, stderr) = repo.fails(&["fmt", "frontend/NODE.json"]);
    assert!(stderr.contains("unknown field `extra`"), "{stderr}");
}

#[test]
fn check_is_green_on_a_consistent_tree_and_warns_on_a_superseded_ref() {
    let repo = TempRepo::charted();
    let report = repo.ok(&["check", "--ref-index", &repo.index_arg()]);
    assert_eq!(
        report,
        "warning: frontend: ref 39944194 (Frontend Stack) is marked superseded in the ref index\n4 nodes, 0 errors, 1 warnings\n"
    );
    let without_index = repo.ok(&["check"]);
    assert_eq!(without_index, "4 nodes, 0 errors, 0 warnings\n");
}

#[test]
fn check_reports_every_rule_and_exits_one() {
    let repo = TempRepo::charted();
    let wrong_path = node(
        "backend/elsewhere",
        "Mislocated.",
        &[
            ("crates/", "members", true),
            ("ghost/", "absent", true),
            ("deep/child/", "nested, absent", true),
        ],
        &[],
        &[],
    );
    repo.write("backend/NODE.json", &wrong_path);
    let unlisted = node(
        "frontend/web",
        "Unlisted child.",
        &[],
        &[
            (11_763_713, "A", "x"),
            (11_763_713, "A", "y"),
            (99, "Nowhere", "z"),
        ],
        &[],
    );
    repo.write("frontend/web/NODE.json", &unlisted);
    repo.write("docs/NODE.json", "{ not json");
    let (stdout, _) = repo.fails(&["check", "--ref-index", &repo.index_arg()]);
    let expected_lines = [
        "error: docs: ",
        "error: backend: path is `backend/elsewhere` but the file sits at `backend`",
        "error: backend: fs entry `ghost/` says node: true but `backend/ghost` has no NODE.json",
        "error: backend: fs entry `deep/child/` says node: true but `backend/deep/child` has no NODE.json",
        "error: frontend/web: not listed in `frontend`'s fs",
        "error: frontend/web: page 11763713 is cited more than once",
        "error: frontend/web: ref 99 (Nowhere) is not in the ref index",
        "5 nodes, 7 errors, 1 warnings",
    ];
    for line in expected_lines {
        assert!(stdout.contains(line), "missing {line:?} in:\n{stdout}");
    }
    let listed_false = node(
        "frontend",
        "The client tier.",
        &[("web/", "the app", false)],
        &[],
        &[],
    );
    repo.write("frontend/NODE.json", &listed_false);
    let (stdout, _) = repo.fails(&["check"]);
    assert!(stdout.contains("error: frontend/web: listed in `frontend`'s fs with node: false, but it has a NODE.json"), "{stdout}");
}

#[test]
fn tree_ls_and_chain_walk_the_tree_in_order() {
    let repo = TempRepo::charted();
    let tree = repo.ok(&["tree"]);
    let drawn = "\
.  The whole repository
├── backend  The backend
│   └── crates  Twelve crates in a hexagon
└── frontend  The client tier
";
    assert_eq!(tree, drawn);
    let ls = repo.ok(&["ls"]);
    assert_eq!(
        ls,
        ".  The whole repository\nbackend  The backend\nbackend/crates  Twelve crates in a hexagon\nfrontend  The client tier\n"
    );
    let chain = repo.ok(&["--json", "chain", "backend/crates/api/src/lib.rs"]);
    let parsed: Value = serde_json::from_str(&chain).expect("json");
    let paths: Vec<&str> = parsed
        .as_array()
        .expect("array")
        .iter()
        .map(|n| n["path"].as_str().expect("path"))
        .collect();
    assert_eq!(paths, [".", "backend", "backend/crates"]);
    let tree_json: Value = serde_json::from_str(&repo.ok(&["--json", "tree"])).expect("json");
    assert_eq!(
        tree_json["children"][0]["children"][0]["path"],
        "backend/crates"
    );
}

#[test]
fn get_refs_and_find_look_things_up() {
    let repo = TempRepo::charted();
    let is = repo.ok(&["get", "backend/crates", "is"]);
    assert_eq!(is, "Twelve crates in a hexagon.\n");
    let refs = repo.ok(&["get", "backend/crates/", "refs"]);
    assert_eq!(
        refs,
        "11763713  Domains and Applications — the dependency rule\n55836674  The Application Layer — use cases\n"
    );
    let whole = repo.ok(&["get", "."]);
    assert!(
        whole.starts_with(
            ".  (charted 2026-09-12)\nshort: The whole repository\nis: The whole repository.\ntype: code\ntags: category:source\nconventions: (none)\n"
        ),
        "{whole}"
    );
    let short = repo.ok(&["get", ".", "short"]);
    assert_eq!(short, "The whole repository\n");
    let (_, stderr) = repo.fails(&["get", "nowhere"]);
    assert_eq!(stderr, "nodes: no node charted at `nowhere`\n");
    let citing = repo.ok(&["refs", "55836674"]);
    assert_eq!(
        citing,
        "backend/crates  The Application Layer — use cases\n"
    );
    let found = repo.ok(&["find", "DEPEND"]);
    assert_eq!(
        found,
        "backend/crates  notes[0]: Adapters depend on domain, never the reverse.\nbackend/crates  refs[11763713].governs: the dependency rule\n"
    );
    let found_role = repo.ok(&["--json", "find", "http driver"]);
    let parsed: Value = serde_json::from_str(&found_role).expect("json");
    assert_eq!(parsed[0]["field"], "fs[api/].role");
}

#[test]
fn write_commands_edit_one_field_and_leave_canonical_files() {
    let repo = TempRepo::charted();
    repo.ok(&[
        "set",
        "frontend",
        "conventions",
        r#"["runes only", "no null"]"#,
    ]);
    assert_eq!(
        repo.ok(&["get", "frontend", "conventions"]),
        "runes only\nno null\n"
    );
    repo.ok(&["set", "frontend", "is", "A plain sentence, not JSON."]);
    assert_eq!(
        repo.ok(&["get", "frontend", "is"]),
        "A plain sentence, not JSON.\n"
    );
    let (_, stderr) = repo.fails(&["set", "frontend", "notes", "\"a string where a list goes\""]);
    assert!(
        stderr.contains("value does not fit field `notes`"),
        "{stderr}"
    );
    let (_, stderr) = repo.fails(&["set", "frontend", "path", "\"elsewhere\""]);
    assert!(
        stderr.contains("derived from the file's location"),
        "{stderr}"
    );
    repo.ok(&["add-ref", "frontend", "5", "Early", "sorts first"]);
    repo.ok(&[
        "add-ref",
        "frontend",
        "39944194",
        "Frontend Stack (amended)",
        "SvelteKit, still",
    ]);
    assert_eq!(
        repo.ok(&["get", "frontend", "refs"]),
        "5  Early — sorts first\n39944194  Frontend Stack (amended) — SvelteKit, still\n"
    );
    repo.ok(&["rm-ref", "frontend", "5"]);
    assert_eq!(
        repo.ok(&["get", "frontend", "refs"]),
        "39944194  Frontend Stack (amended) — SvelteKit, still\n"
    );
    let (_, stderr) = repo.fails(&["rm-ref", "frontend", "5"]);
    assert_eq!(stderr, "nodes: `frontend` does not cite page 5\n");
    repo.ok(&["touch", "frontend", "--date", "2030-01-31"]);
    assert_eq!(repo.ok(&["get", "frontend", "charted"]), "2030-01-31\n");
    let (_, stderr) = repo.fails(&["touch", "frontend", "--date", "2030-02-30"]);
    assert!(stderr.contains("does not exist in month 2"), "{stderr}");
    let touched = repo.ok(&["--json", "touch", "frontend", "--date", "2031-01-01"]);
    let written: Value = serde_json::from_str(&touched).expect("json");
    assert_eq!(written["path"], "frontend");
    assert_eq!(written["charted"], "2031-01-01");
    let silent = repo.ok(&["set", "frontend", "notes", "[]"]);
    assert_eq!(silent, "", "text mode stays silent");
    let after_edits = repo.read("frontend/NODE.json");
    let refmt = repo.ok(&["fmt", "frontend"]);
    assert_eq!(refmt, "", "every write command leaves a canonical file");
    assert_eq!(repo.read("frontend/NODE.json"), after_edits);
}

#[test]
fn a_closed_stdout_ends_the_run_quietly() {
    let repo = TempRepo::charted();
    let (reader, writer) = std::io::pipe().expect("a pipe");
    drop(reader);
    let output = Command::new(env!("CARGO_BIN_EXE_nodes"))
        .arg("--root")
        .arg(&repo.root)
        .arg("tree")
        .stdout(writer)
        .output()
        .expect("runs");
    assert!(output.status.success(), "exit {:?}", output.status);
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "",
        "no panic, no error line"
    );
}

#[test]
fn the_root_is_discovered_from_any_directory_beneath_it() {
    let repo = TempRepo::charted();
    let from_below = Command::new(env!("CARGO_BIN_EXE_nodes"))
        .current_dir(repo.root.join("backend/crates"))
        .arg("ls")
        .output()
        .expect("runs");
    assert!(from_below.status.success());
    let listed = String::from_utf8(from_below.stdout).expect("utf-8");
    assert!(listed.starts_with(".  The whole repository\n"));
    let nowhere = TempRepo::new();
    let outside = Command::new(env!("CARGO_BIN_EXE_nodes"))
        .current_dir(Path::new(&nowhere.root))
        .arg("ls")
        .output()
        .expect("runs");
    assert!(!outside.status.success());
    assert!(String::from_utf8_lossy(&outside.stderr).contains("no root NODE.json"));
}

/// The paths `ls` prints, one per node.
fn listed_paths(repo: &TempRepo) -> Vec<String> {
    repo.ok(&["ls"])
        .lines()
        .map(|line| line.split("  ").next().expect("a path").to_owned())
        .collect()
}

#[test]
fn reads_cross_a_mount_with_paths_rebased_onto_the_outer_root() {
    let repo = TempRepo::mounting();
    let tree = repo.ok(&["tree"]);
    let drawn = "\
.  Home
├── code/zurfur  [mount]  The Zurfur monorepo
└── life  [mount]  Personal documents
    ├── archive  [mount]  Closed years
    │   └── 2019  Everything from 2019
    └── housing  Lease and utilities
";
    assert_eq!(tree, drawn);
    let ls = repo.ok(&["ls"]);
    let listed = "\
.  Home
code/zurfur  The Zurfur monorepo
life  Personal documents
life/archive  Closed years
life/archive/2019  Everything from 2019
life/housing  Lease and utilities
";
    assert_eq!(ls, listed);
    let chain = repo.ok(&["--json", "chain", "life/archive/2019/taxes.pdf"]);
    let parsed: Value = serde_json::from_str(&chain).expect("json");
    let paths: Vec<&str> = parsed
        .as_array()
        .expect("array")
        .iter()
        .map(|n| n["path"].as_str().expect("path"))
        .collect();
    assert_eq!(paths, [".", "life", "life/archive", "life/archive/2019"]);
    assert_eq!(repo.ok(&["get", "life/housing", "path"]), "life/housing\n");
    assert_eq!(repo.ok(&["get", "life", "path"]), "life\n");
    assert_eq!(
        repo.ok(&["get", "life/archive/2019", "path"]),
        "life/archive/2019\n"
    );
    let whole = repo.ok(&["get", "life/housing"]);
    assert!(
        whole.starts_with("life/housing  (charted 2026-09-12)\nshort: Lease and utilities\n"),
        "{whole}"
    );
    assert_eq!(
        repo.ok(&["find", "LEASE"]),
        "life/housing  short: Lease and utilities\nlife/housing  is: Lease and utilities.\nlife/housing  refs[7].title: The Lease\n"
    );
    assert_eq!(
        repo.ok(&["refs", "7"]),
        "life/housing  The Lease — the tenancy\n"
    );
    let tree_json: Value = serde_json::from_str(&repo.ok(&["--json", "tree"])).expect("json");
    assert_eq!(tree_json["mount"], false);
    let life = &tree_json["children"][1];
    assert_eq!(life["path"], "life");
    assert_eq!(life["mount"], true);
    assert_eq!(life["children"][0]["path"], "life/archive");
    assert_eq!(life["children"][0]["mount"], true);
    assert_eq!(life["children"][1]["mount"], false);
    let ls_json: Value = serde_json::from_str(&repo.ok(&["--json", "ls"])).expect("json");
    let mounts: Vec<bool> = ls_json
        .as_array()
        .expect("array")
        .iter()
        .map(|n| n["mount"].as_bool().expect("mount"))
        .collect();
    assert_eq!(mounts, [false, true, true, true, false, false]);
}

#[test]
fn a_mounted_tree_reads_the_same_from_its_own_root() {
    let repo = TempRepo::mounting();
    let from_inside = Command::new(env!("CARGO_BIN_EXE_nodes"))
        .current_dir(repo.root.join("life/housing"))
        .arg("ls")
        .output()
        .expect("runs");
    assert!(from_inside.status.success());
    let listed = String::from_utf8(from_inside.stdout).expect("utf-8");
    assert_eq!(
        listed,
        ".  Personal documents\narchive  Closed years\narchive/2019  Everything from 2019\nhousing  Lease and utilities\n",
        "the nearest root wins, and its own mounts are crossed too"
    );
}

#[test]
fn check_stops_at_a_mount_and_reads_only_its_root() {
    let repo = TempRepo::mounting();
    repo.write("docs/index.md", "- `1` — The only citable page\n");
    let mislocated = node(
        "elsewhere",
        "Broken inside the mount.",
        &[("ghost/", "absent", true)],
        &[(99, "Nowhere", "not in the outer index")],
        &[],
    );
    repo.write("life/housing/NODE.json", &mislocated);
    let report = repo.ok(&["check", "--ref-index", &repo.index_arg()]);
    assert_eq!(report, "1 nodes, 2 mounts, 0 errors, 0 warnings\n");
    let report_json: Value = serde_json::from_str(&repo.ok(&["--json", "check"])).expect("json");
    assert_eq!(report_json["nodes"], 1);
    assert_eq!(report_json["mounts"], 2);
    let broken_root = node(".", "Personal documents.", &[], &[], &[])
        .replace("\"notes\": []", "\"notes\": [], \"extra\": 1");
    repo.write("life/NODE.json", &broken_root);
    let (stdout, _) = repo.fails(&["check"]);
    assert!(stdout.contains("error: life: "), "{stdout}");
    assert!(stdout.contains("unknown field `extra`"), "{stdout}");
    assert!(
        stdout.contains("1 nodes, 2 mounts, 1 errors, 0 warnings"),
        "{stdout}"
    );
}

#[test]
fn a_mount_is_listed_by_its_parent_with_node_true() {
    let repo = TempRepo::mounting();
    let listed_false = node(
        ".",
        "Home.",
        &[
            ("code/zurfur/", "the monorepo — own tree", true),
            ("life/", "documents — own tree", false),
        ],
        &[],
        &[],
    );
    repo.write("NODE.json", &listed_false);
    let (stdout, _) = repo.fails(&["check"]);
    assert!(
        stdout.contains("error: life: listed in `.`'s fs with node: false, but it has a NODE.json"),
        "{stdout}"
    );
    let unlisted = node(".", "Home.", &[], &[], &[]);
    repo.write("NODE.json", &unlisted);
    let (stdout, _) = repo.fails(&["check"]);
    assert!(
        stdout.contains("error: life: not listed in `.`'s fs"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("code/zurfur"),
        "a mount beneath a pass-through directory need not be listed: {stdout}"
    );
}

#[test]
fn writes_and_fmt_never_cross_a_mount() {
    let repo = TempRepo::mounting();
    let messy = r#"{"notes": [], "short": "Messy", "refs": [], "fs": [], "entry_points": [],
        "conventions": [], "tags": ["category:housing"], "type": "document", "is": "Messy.", "charted": "2026-01-02", "path": "housing"}"#;
    repo.write("life/housing/NODE.json", messy);
    let refused = [
        vec!["set", "life/housing", "short", "Rewritten"],
        vec!["set", "life", "short", "Rewritten"],
        vec!["touch", "life/housing", "--date", "2030-01-31"],
        vec!["add-ref", "life/housing", "5", "Early", "sorts first"],
        vec!["rm-ref", "life/housing", "7"],
        vec!["fmt", "life/housing"],
        vec!["fmt", "life/archive/2019/NODE.json"],
    ];
    for args in &refused {
        let (_, stderr) = repo.fails(args);
        assert!(
            stderr.contains("belongs to the tree mounted at `life"),
            "{args:?}: {stderr}"
        );
    }
    let (_, stderr) = repo.fails(&["set", "life/archive/2019", "short", "Rewritten"]);
    let innermost = format!(
        "nodes: `life/archive/2019` belongs to the tree mounted at `life/archive`; writes do not cross a mount — run this with --root {}\n",
        repo.root.join("life/archive").display()
    );
    assert_eq!(stderr, innermost);
    let bare = repo.ok(&["fmt"]);
    assert_eq!(bare, "", "a bare fmt formats this tree's files only");
    assert_eq!(repo.read("life/housing/NODE.json"), messy);
    let life_root = repo.root.join("life");
    let from_its_own_root = Command::new(env!("CARGO_BIN_EXE_nodes"))
        .arg("--root")
        .arg(&life_root)
        .arg("fmt")
        .output()
        .expect("runs");
    assert!(from_its_own_root.status.success());
    assert_eq!(
        String::from_utf8_lossy(&from_its_own_root.stdout),
        "fmt: housing/NODE.json\n"
    );
}

#[test]
fn ignore_files_follow_gitignore_semantics() {
    let repo = TempRepo::charted();
    let stray = |path: &str| node(path, "A stray node.", &[], &[], &[]);
    for dir in [
        "build",
        "backend/build",
        "frontend/generated",
        "a.cache",
        "keep.cache",
        "dist",
        "vendor",
        "frontend/out",
        "backend/out",
        ".hidden",
        ".github",
        "target",
    ] {
        repo.write(&format!("{dir}/NODE.json"), &stray(dir));
    }
    repo.write(".gitignore", "dist/\nvendor/\n");
    repo.write("frontend/.gitignore", "/out/\n");
    repo.write(
        ".chartignore",
        "# anchored, nested, globbed, negated\n/build/\nfrontend/generated/\n*.cache/\n!keep.cache/\n!vendor/\n!.github/\n",
    );
    let visible = listed_paths(&repo);
    let expected = [
        ".",
        ".github",
        "backend",
        "backend/build",
        "backend/crates",
        "backend/out",
        "frontend",
        "keep.cache",
        "vendor",
    ];
    assert_eq!(visible, expected);
}

#[test]
fn a_mount_reads_under_its_own_ignore_files_and_an_ignored_mount_is_not_read() {
    let repo = TempRepo::mounting();
    repo.write(".chartignore", "housing/\n");
    repo.write("life/.chartignore", "/archive/\n");
    let visible = listed_paths(&repo);
    assert_eq!(
        visible,
        [".", "code/zurfur", "life", "life/housing"],
        "the outer `housing/` line does not reach into the mount; the mount's own `/archive/` does"
    );
    repo.write(".chartignore", "/life/\n");
    assert_eq!(listed_paths(&repo), [".", "code/zurfur"]);
}

#[test]
fn a_listed_node_the_walk_never_reaches_is_an_error() {
    let repo = TempRepo::mounting();
    let home = node(
        ".",
        "Home.",
        &[
            (".index/", "hidden, yet charted", true),
            ("code/zurfur/", "the monorepo — own tree", true),
            ("life/", "documents — own tree", true),
            ("life/housing/", "a node of another tree", true),
        ],
        &[],
        &[],
    );
    repo.write("NODE.json", &home);
    let index = node(".index", "The hidden index.", &[], &[], &[]);
    repo.write(".index/NODE.json", &index);
    let (stdout, _) = repo.fails(&["check"]);
    let expected_lines = [
        "error: .: fs entry `.index/` says node: true but `.index` is hidden or ignored — re-include it with a `!` line in .chartignore, or say node: false",
        "error: .: fs entry `life/housing/` says node: true but `life/housing` belongs to the tree mounted at `life`",
        "1 nodes, 2 mounts, 2 errors, 0 warnings",
    ];
    for line in expected_lines {
        assert!(stdout.contains(line), "missing {line:?} in:\n{stdout}");
    }
    repo.write(".chartignore", "!.index/\n/code/\n");
    let (stdout, _) = repo.fails(&["check"]);
    assert!(
        stdout.contains("error: .: fs entry `code/zurfur/` says node: true but `code/zurfur` is hidden or ignored"),
        "an ignored mount is as unreachable as an ignored node: {stdout}"
    );
    assert!(!stdout.contains("`.index`"), "{stdout}");
    assert!(
        stdout.contains("2 nodes, 1 mounts, 2 errors, 0 warnings"),
        "{stdout}"
    );
}

#[test]
fn a_line_git_would_not_understand_is_passed_over() {
    let repo = TempRepo::charted();
    repo.write(".chartignore", "a[\nfrontend/\n");
    assert_eq!(listed_paths(&repo), [".", "backend", "backend/crates"]);
}

#[cfg(unix)]
#[test]
fn an_unreadable_ignore_file_stops_the_run() {
    use std::os::unix::fs::PermissionsExt;

    let repo = TempRepo::charted();
    repo.write("frontend/.chartignore", "generated/\n");
    let ignore_file = repo.root.join("frontend/.chartignore");
    let no_access = fs::Permissions::from_mode(0o000);
    fs::set_permissions(&ignore_file, no_access).expect("chmod");
    if fs::read(&ignore_file).is_ok() {
        return; // running as root: nothing is unreadable
    }
    let (_, stderr) = repo.fails(&["ls"]);
    assert!(stderr.contains("frontend/.chartignore"), "{stderr}");
}

#[test]
fn nodes_are_classified_by_type_and_ranked_category_tags() {
    let repo = TempRepo::charted();
    let frontend = classified(
        &repo.read("frontend/NODE.json"),
        "code",
        &["category:ui", "category:source"],
    );
    repo.write("frontend/NODE.json", &frontend);
    let crates = classified(
        &repo.read("backend/crates/NODE.json"),
        "code",
        &["category:source", "category:tests", "category:ui"],
    );
    repo.write("backend/crates/NODE.json", &crates);
    let docs = node("docs", "Pointers into the design corpus.", &[], &[], &[]);
    let docs = classified(&docs, "document", &["category:docs", "category:generated"]);
    repo.write("docs/NODE.json", &docs);
    let root = classified(&repo.read("NODE.json"), "code", &["category:project"]).replace(
        "\"name\": \"docs/\",\n      \"role\": \"pointers\",\n      \"node\": false",
        "\"name\": \"docs/\",\n      \"role\": \"pointers\",\n      \"node\": true",
    );
    repo.write("NODE.json", &root);
    assert_eq!(repo.ok(&["check"]), "5 nodes, 0 errors, 0 warnings\n");
    assert_eq!(
        repo.ok(&["ls", "--category", "ui"]),
        "frontend  The client tier\nbackend/crates  Twelve crates in a hexagon\n",
        "best fit first: `ui` leads frontend's categories and trails the crates'"
    );
    assert_eq!(
        repo.ok(&["ls", "--type", "document"]),
        "docs  Pointers into the design corpus\n"
    );
    assert_eq!(
        repo.ok(&["ls", "--type", "code", "--category", "source"]),
        "backend  The backend\nbackend/crates  Twelve crates in a hexagon\nfrontend  The client tier\n"
    );
    assert_eq!(
        repo.ok(&["ls", "--category", "finance"]),
        "",
        "no vocabulary closes the categories: one nobody carries lists nothing"
    );
    let (_, stderr) = repo.fails(&["ls", "--category", "Code Source"]);
    assert!(
        stderr.contains("`category:Code Source` is not a tag"),
        "{stderr}"
    );
    assert_eq!(repo.ok(&["get", "frontend", "type"]), "code\n");
    assert_eq!(
        repo.ok(&["get", "frontend", "tags"]),
        "category:ui\ncategory:source\n"
    );
    let whole = repo.ok(&["get", "frontend"]);
    assert!(
        whole.contains("\ntype: code\ntags: category:ui, category:source\n"),
        "{whole}"
    );
    let ls_json: Value = serde_json::from_str(&repo.ok(&["--json", "ls"])).expect("json");
    assert_eq!(ls_json[4]["path"], "frontend");
    assert_eq!(ls_json[4]["type"], "code");
    assert_eq!(
        ls_json[4]["tags"],
        json!(["category:ui", "category:source"])
    );
    let tree_json: Value = serde_json::from_str(&repo.ok(&["--json", "tree"])).expect("json");
    assert_eq!(tree_json["type"], "code");
    assert_eq!(tree_json["tags"], json!(["category:project"]));
    assert_eq!(
        repo.ok(&["find", "generated"]),
        "docs  tags[1]: category:generated\n"
    );
    assert_eq!(repo.ok(&["find", "DOCUMENT"]), "docs  type: document\n");
}

#[test]
fn nodes_are_selected_by_any_tag() {
    let repo = TempRepo::charted();
    let frontend = classified(
        &repo.read("frontend/NODE.json"),
        "code",
        &["category:ui", "artist:starsie", "wip"],
    );
    repo.write("frontend/NODE.json", &frontend);
    let crates = classified(
        &repo.read("backend/crates/NODE.json"),
        "code",
        &["category:source", "wip", "category:ui"],
    );
    repo.write("backend/crates/NODE.json", &crates);
    assert_eq!(
        repo.ok(&["ls", "--tag", "artist:starsie"]),
        "frontend  The client tier\n",
        "any namespace is selected through --tag"
    );
    assert_eq!(
        repo.ok(&["ls", "--tag", "wip"]),
        "backend/crates  Twelve crates in a hexagon\nfrontend  The client tier\n",
        "only a category ranks: any other tag lists in tree order"
    );
    assert_eq!(
        repo.ok(&["ls", "--tag", "wip", "--tag", "artist:starsie"]),
        "frontend  The client tier\n",
        "every selected tag must be carried"
    );
    assert_eq!(
        repo.ok(&["ls", "--tag", "wip", "--category", "source"]),
        "backend/crates  Twelve crates in a hexagon\n"
    );
    assert_eq!(
        repo.ok(&["ls", "--tag", "category:ui"]),
        repo.ok(&["ls", "--category", "ui"]),
        "--category is --tag category:…, shorter"
    );
    assert_eq!(
        repo.ok(&["ls", "--tag", "category:ui"]),
        "frontend  The client tier\nbackend/crates  Twelve crates in a hexagon\n",
        "best fit first, however the category was spelled"
    );
    let by_tag = "\
.
└── frontend  The client tier
";
    assert_eq!(repo.ok(&["tree", "--tag", "artist:starsie"]), by_tag);
    assert_eq!(repo.ok(&["ls", "--tag", "nobody:home"]), "");
    let (_, stderr) = repo.fails(&["ls", "--tag", "Code Source"]);
    assert!(
        stderr.contains(
            "`Code Source` is not a tag: a tag is `word` or `namespace:word`, in lowercase letters, digits, `_` and `-`"
        ),
        "{stderr}"
    );
}

#[test]
fn type_is_set_from_its_closed_vocabulary_and_tags_are_set_freely() {
    let repo = TempRepo::charted();
    repo.ok(&["set", "frontend", "type", "document"]);
    let free_tags = r#"["category:work", "invoices-2026", "category:code_source"]"#;
    repo.ok(&["set", "frontend", "tags", free_tags]);
    assert_eq!(repo.ok(&["get", "frontend", "type"]), "document\n");
    assert_eq!(
        repo.ok(&["get", "frontend", "tags"]),
        "category:work\ninvoices-2026\ncategory:code_source\n",
        "as authored, never sorted"
    );
    assert_eq!(repo.ok(&["fmt"]), "", "set leaves the file canonical");
    let (_, stderr) = repo.fails(&["set", "frontend", "type", "paperwork"]);
    assert!(
        stderr.contains("unknown type `paperwork`; the types are code, document"),
        "{stderr}"
    );
    let (_, stderr) = repo.fails(&[
        "set",
        "frontend",
        "tags",
        r#"["category:work", "Code Source"]"#,
    ]);
    assert!(stderr.contains("`Code Source` is not a tag"), "{stderr}");
    let (_, stderr) = repo.fails(&["set", "frontend", "tags", "[]"]);
    assert!(stderr.contains("at least one `category:` tag"), "{stderr}");
    let (_, stderr) = repo.fails(&["set", "frontend", "tags", r#"["wip"]"#]);
    assert!(stderr.contains("at least one `category:` tag"), "{stderr}");
    let (_, stderr) = repo.fails(&[
        "set",
        "frontend",
        "tags",
        r#"["category:work", "wip", "wip"]"#,
    ]);
    assert!(
        stderr.contains("`wip` is tagged more than once"),
        "{stderr}"
    );
    let unclassified = repo
        .read("backend/NODE.json")
        .replace("  \"type\": \"code\",\n", "");
    repo.write("backend/NODE.json", &unclassified);
    let (stdout, _) = repo.fails(&["check"]);
    assert!(stdout.contains("error: backend: "), "{stdout}");
    assert!(stdout.contains("missing field `type`"), "{stdout}");
}

#[test]
fn vocabulary_prints_the_closed_type_vocabulary_and_needs_no_root() {
    let nowhere = TempRepo::new();
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_nodes"))
            .current_dir(&nowhere.root)
            .args(args)
            .output()
            .expect("runs");
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8")
    };
    let text = run(&["vocabulary"]);
    let head = "\
type — what a directory broadly holds: the one kind that fits it best
  code  program source and what is built from it
  document  paperwork and written records
";
    assert!(text.starts_with(head), "{text}");
    assert_eq!(
        text.lines().count(),
        7,
        "the heading and the six types — tags have no vocabulary: {text}"
    );
    let parsed: Value = serde_json::from_str(&run(&["--json", "vocabulary"])).expect("json");
    let first_type = json!({"term": "code", "meaning": "program source and what is built from it"});
    assert_eq!(parsed["type"][0], first_type);
    assert_eq!(parsed["type"].as_array().expect("types").len(), 6);
    assert!(parsed.get("categories").is_none());
}

#[test]
fn classify_sets_the_type_and_the_categories_even_where_they_are_missing() {
    let repo = TempRepo::charted();
    let unclassified = repo
        .read("frontend/NODE.json")
        .replace("  \"type\": \"code\",\n", "")
        .replace("  \"tags\": [\n    \"category:source\"\n  ],\n", "");
    repo.write("frontend/NODE.json", &unclassified);
    let (_, stderr) = repo.fails(&["get", "frontend"]);
    assert!(stderr.contains("missing field `type`"), "{stderr}");
    let (_, stderr) = repo.fails(&["set", "frontend", "type", "code"]);
    assert!(
        stderr.contains("missing field `type`"),
        "set needs a file that parses: {stderr}"
    );
    let silent = repo.ok(&["classify", "frontend", "code", "ui", "source"]);
    assert_eq!(silent, "", "text mode stays silent");
    assert_eq!(repo.ok(&["get", "frontend", "type"]), "code\n");
    assert_eq!(
        repo.ok(&["get", "frontend", "tags"]),
        "category:ui\ncategory:source\n"
    );
    assert_eq!(repo.ok(&["fmt"]), "", "classify leaves a canonical file");
    let labelled = r#"["wip", "category:ui", "artist:starsie", "category:source"]"#;
    repo.ok(&["set", "frontend", "tags", labelled]);
    let reclassified = repo.ok(&["--json", "classify", "frontend", "document", "docs"]);
    let written: Value = serde_json::from_str(&reclassified).expect("json");
    assert_eq!(written["type"], "document");
    assert_eq!(
        written["tags"],
        json!(["category:docs", "wip", "artist:starsie"]),
        "the categories are replaced and lead; every other tag is kept"
    );
    let (_, stderr) = repo.fails(&["classify", "frontend", "code", "ui", "ui"]);
    assert!(
        stderr.contains("`category:ui` is tagged more than once"),
        "{stderr}"
    );
    let (_, stderr) = repo.fails(&["classify", "frontend", "code"]);
    assert!(
        stderr.contains("<CATEGORY>"),
        "at least one category: {stderr}"
    );
    let (_, stderr) = repo.fails(&["classify", "frontend", "code", "Code Source"]);
    assert!(
        stderr.contains("`category:Code Source` is not a tag"),
        "{stderr}"
    );
    let (_, stderr) = repo.fails(&["classify", "nowhere", "code", "source"]);
    assert_eq!(stderr, "nodes: no node charted at `nowhere`\n");
    let shortless = repo
        .read("backend/NODE.json")
        .replace("  \"short\": \"The backend\",\n", "");
    repo.write("backend/NODE.json", &shortless);
    let (_, stderr) = repo.fails(&["classify", "backend", "code", "source"]);
    assert!(
        stderr.contains("missing field `short`"),
        "only the classification may be missing: {stderr}"
    );
    let mounting = TempRepo::mounting();
    let (_, stderr) = mounting.fails(&["classify", "life/housing", "document", "housing"]);
    assert!(
        stderr.contains("belongs to the tree mounted at `life`"),
        "{stderr}"
    );
}

#[test]
fn migrate_rewrites_ranked_categories_into_category_tags_once() {
    let repo = TempRepo::charted();
    let legacy = repo.read("frontend/NODE.json").replace(
        "  \"tags\": [\n    \"category:source\"\n  ],\n",
        "  \"categories\": [\n    \"ui\",\n    \"source\"\n  ],\n",
    );
    repo.write("frontend/NODE.json", &legacy);
    for refused in [vec!["get", "frontend"], vec!["ls"], vec!["fmt"]] {
        let (_, stderr) = repo.fails(&refused);
        assert!(
            stderr.contains("unknown field `categories`"),
            "only migrate reads a file from before the tags: {refused:?}: {stderr}"
        );
    }
    assert_eq!(repo.ok(&["migrate"]), "migrate: frontend/NODE.json\n");
    assert_eq!(
        repo.ok(&["get", "frontend", "tags"]),
        "category:ui\ncategory:source\n",
        "the ranking carries over"
    );
    assert_eq!(repo.ok(&["migrate"]), "", "a second run changes nothing");
    assert_eq!(repo.ok(&["fmt"]), "", "migrate leaves canonical files");
    assert_eq!(repo.ok(&["check"]), "4 nodes, 0 errors, 0 warnings\n");
    let visited: Value = serde_json::from_str(&repo.ok(&["--json", "migrate"])).expect("json");
    let frontend_visit = json!({"file": "frontend/NODE.json", "changed": false});
    assert_eq!(visited[3], frontend_visit);
}

#[test]
fn migrate_leaves_a_mounted_tree_to_its_own_root() {
    let repo = TempRepo::mounting();
    let legacy = repo.read("life/housing/NODE.json").replace(
        "  \"tags\": [\n    \"category:source\"\n  ],\n",
        "  \"categories\": [\n    \"housing\"\n  ],\n",
    );
    repo.write("life/housing/NODE.json", &legacy);
    assert_eq!(repo.ok(&["migrate"]), "");
    assert_eq!(
        repo.read("life/housing/NODE.json"),
        legacy,
        "writes never cross a mount"
    );
    let life_root = repo.root.join("life");
    let from_its_own_root = Command::new(env!("CARGO_BIN_EXE_nodes"))
        .arg("--root")
        .arg(&life_root)
        .arg("migrate")
        .output()
        .expect("runs");
    assert!(from_its_own_root.status.success());
    assert_eq!(
        String::from_utf8_lossy(&from_its_own_root.stdout),
        "migrate: housing/NODE.json\n"
    );
}

#[test]
fn a_filtered_tree_keeps_the_matches_and_the_ancestors_that_lead_to_them() {
    let repo = TempRepo::charted();
    let frontend = classified(
        &repo.read("frontend/NODE.json"),
        "code",
        &["category:ui", "category:source"],
    );
    repo.write("frontend/NODE.json", &frontend);
    let crates = classified(
        &repo.read("backend/crates/NODE.json"),
        "document",
        &["category:docs", "category:ui"],
    );
    repo.write("backend/crates/NODE.json", &crates);
    let by_category = "\
.
├── backend
│   └── crates  Twelve crates in a hexagon
└── frontend  The client tier
";
    assert_eq!(
        repo.ok(&["tree", "--category", "ui"]),
        by_category,
        "a match carries its `short`; an ancestor that only leads to one is bare"
    );
    let by_type = "\
.
└── backend
    └── crates  Twelve crates in a hexagon
";
    assert_eq!(repo.ok(&["tree", "--type", "document"]), by_type);
    let both = "\
.  The whole repository
├── backend  The backend
└── frontend  The client tier
";
    assert_eq!(
        repo.ok(&["tree", "--type", "code", "--category", "source"]),
        both
    );
    assert_eq!(repo.ok(&["tree", "--category", "finance"]), "");
    let filtered: Value =
        serde_json::from_str(&repo.ok(&["--json", "tree", "--category", "ui"])).expect("json");
    assert_eq!(filtered["match"], false);
    assert_eq!(filtered["children"].as_array().expect("children").len(), 2);
    assert_eq!(filtered["children"][0]["path"], "backend");
    assert_eq!(filtered["children"][0]["match"], false);
    assert_eq!(filtered["children"][0]["children"][0]["match"], true);
    assert_eq!(filtered["children"][1]["match"], true);
    let nothing: Value =
        serde_json::from_str(&repo.ok(&["--json", "tree", "--category", "finance"])).expect("json");
    assert_eq!(nothing, Value::Null);
    let unfiltered: Value = serde_json::from_str(&repo.ok(&["--json", "tree"])).expect("json");
    assert!(
        unfiltered.get("match").is_none(),
        "an unfiltered tree keeps its shape"
    );
}

#[test]
fn a_filtered_tree_reaches_into_mounts() {
    let repo = TempRepo::mounting();
    let housing = classified(
        &repo.read("life/housing/NODE.json"),
        "document",
        &["category:housing", "category:legal"],
    );
    repo.write("life/housing/NODE.json", &housing);
    let drawn = "\
.
└── life  [mount]
    └── housing  Lease and utilities
";
    assert_eq!(repo.ok(&["tree", "--category", "legal"]), drawn);
}

#[test]
fn grep_searches_file_contents_and_names_the_owning_node() {
    let repo = TempRepo::charted();
    repo.write(
        "backend/crates/api/src/lib.rs",
        "fn main() {\n    serve();\n    // TODO: Graceful Shutdown\n    // graceful, again\n}\n",
    );
    let both_lines = "\
backend/crates  backend/crates/api/src/lib.rs:3: // TODO: Graceful Shutdown
backend/crates  backend/crates/api/src/lib.rs:4: // graceful, again
";
    assert_eq!(
        repo.ok(&["grep", "GRACEFUL"]),
        both_lines,
        "the node that owns a file is the deepest one above it"
    );
    assert_eq!(
        repo.ok(&["grep", "index built"]),
        ".  docs/index.md:1: # Index built 2026-06-30\n"
    );
    assert_eq!(
        repo.ok(&["grep", "hexagon"]),
        "",
        "NODE.json is the chart: find searches it, grep does not"
    );
    assert_eq!(
        repo.ok(&["grep", "graceful", "--limit", "1"]),
        "backend/crates  backend/crates/api/src/lib.rs:3: // TODO: Graceful Shutdown\n"
    );
    let hits: Value = serde_json::from_str(&repo.ok(&["--json", "grep", "again"])).expect("json");
    let expected_hits = json!([{
        "path": "backend/crates",
        "file": "backend/crates/api/src/lib.rs",
        "line": 4,
        "text": "// graceful, again",
    }]);
    assert_eq!(hits, expected_hits);
    let (_, stderr) = repo.fails(&["grep", ""]);
    assert!(
        stderr.contains("an empty term matches every line"),
        "{stderr}"
    );
}

#[test]
fn grep_stays_out_of_ignored_hidden_and_binary_files() {
    let repo = TempRepo::charted();
    repo.write(".chartignore", "*.log\n/frontend/\n");
    repo.write("server.log", "a needle here\n");
    repo.write("frontend/app.ts", "a needle here\n");
    repo.write(".env", "NEEDLE=1\n");
    repo.write("docs/blob.bin", "a needle\0here\n");
    repo.write("docs/notes.md", "a needle here\n");
    repo.write("docs/huge.txt", &"needle\n".repeat(1_300_000));
    let output = repo.nodes(&["grep", "needle"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ".  docs/notes.md:1: a needle here\n"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr, "nodes: passed over docs/huge.txt: 9100000 bytes, more than grep reads (8388608)\n",
        "a file too large to search is said so; a binary or ignored one is not worth a word"
    );
}

#[test]
fn grep_crosses_a_mount_under_the_mounts_own_ignore_files() {
    let repo = TempRepo::mounting();
    repo.write("life/.chartignore", "scans/\n");
    repo.write("life/housing/lease.md", "The deposit is two months.\n");
    repo.write("life/scans/lease.txt", "deposit\n");
    repo.write("life/archive/2019/taxes.md", "No deposit that year.\n");
    let across_mounts = "\
life/archive/2019  life/archive/2019/taxes.md:1: No deposit that year.
life/housing  life/housing/lease.md:1: The deposit is two months.
";
    assert_eq!(repo.ok(&["grep", "deposit"]), across_mounts);
}

#[test]
fn recent_lists_the_newest_files_first_and_ties_by_name() {
    let repo = TempRepo::charted();
    repo.write("backend/crates/api/src/lib.rs", "");
    repo.write("frontend/b.ts", "");
    repo.write("frontend/app.ts", "");
    repo.modified_at("docs/index.md", 1_789_257_600);
    repo.modified_at("frontend/app.ts", 1_789_300_800);
    repo.modified_at("frontend/b.ts", 1_789_300_800);
    repo.modified_at("backend/crates/api/src/lib.rs", 1_789_912_991);
    let newest_first = "\
2026-09-20T14:03:11Z  backend/crates/api/src/lib.rs
2026-09-13T12:00:00Z  frontend/app.ts
2026-09-13T12:00:00Z  frontend/b.ts
2026-09-13T00:00:00Z  docs/index.md
";
    assert_eq!(
        repo.ok(&["recent"]),
        newest_first,
        "NODE.json files are the chart, not content: a charting run does not flood the list"
    );
    assert_eq!(
        repo.ok(&["recent", "--limit", "1"]),
        "2026-09-20T14:03:11Z  backend/crates/api/src/lib.rs\n"
    );
    let beneath_frontend = "\
2026-09-13T12:00:00Z  frontend/app.ts
2026-09-13T12:00:00Z  frontend/b.ts
";
    assert_eq!(repo.ok(&["recent", "frontend"]), beneath_frontend);
    let listed: Value =
        serde_json::from_str(&repo.ok(&["--json", "recent", "--limit", "1"])).expect("json");
    let expected_listing = json!([{
        "path": "backend/crates",
        "file": "backend/crates/api/src/lib.rs",
        "modified": "2026-09-20T14:03:11Z",
    }]);
    assert_eq!(listed, expected_listing);
}

#[test]
fn recent_refuses_a_path_that_is_ignored_or_no_directory() {
    let repo = TempRepo::charted();
    repo.write(".chartignore", "/frontend/\n");
    repo.write("frontend/app.ts", "");
    let (_, stderr) = repo.fails(&["recent", "frontend"]);
    assert_eq!(
        stderr, "nodes: `frontend`: ignored: the walk never reaches it\n",
        "an empty list would read as: nothing changed"
    );
    let (_, stderr) = repo.fails(&["recent", "docs/index.md"]);
    assert_eq!(stderr, "nodes: `docs/index.md`: not a directory\n");
    let (_, stderr) = repo.fails(&["recent", "nowhere"]);
    assert_eq!(stderr, "nodes: `nowhere`: not a directory\n");
    let (_, stderr) = repo.fails(&["recent", "--limit", "0"]);
    assert!(
        stderr.contains("`0` is not a limit: a whole number from 1 up"),
        "{stderr}"
    );
}

#[test]
fn resolve_ranks_exact_partial_and_slightly_off_names() {
    let repo = TempRepo::charted();
    repo.write("backend/crates/api/src/lib.rs", "");
    let exact_name_first = "\
backend/crates  [node]
backend/crates/api/src/lib.rs
";
    assert_eq!(
        repo.ok(&["resolve", "crates"]),
        exact_name_first,
        "a node is marked; a path that merely holds the name trails the one that ends in it"
    );
    assert_eq!(
        repo.ok(&["resolve", "crates", "--limit", "1"]),
        "backend/crates  [node]\n"
    );
    assert_eq!(
        repo.ok(&["resolve", "frotnend"]),
        "frontend  [node]\n",
        "two neighbours transposed"
    );
    assert_eq!(
        repo.ok(&["resolve", "indx"]),
        "docs/index.md\n",
        "a dropped character, measured against the name without its extension"
    );
    assert_eq!(repo.ok(&["resolve", "zzz"]), "");
    let resolved: Value =
        serde_json::from_str(&repo.ok(&["--json", "resolve", "crates", "--limit", "1"]))
            .expect("json");
    let expected_candidate = json!([{"path": "backend/crates", "kind": "node", "score": 900}]);
    assert_eq!(resolved, expected_candidate);
    let (_, stderr) = repo.fails(&["resolve", ""]);
    assert_eq!(stderr, "nodes: query ``: nothing to resolve\n");
    let mounting = TempRepo::mounting();
    assert_eq!(
        mounting.ok(&["resolve", "housing"]),
        "life/housing  [node]\n",
        "a mounted tree's paths are rebased onto the outer root"
    );
}
