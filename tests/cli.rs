//! End-to-end tests: the `nodes` binary over a throwaway charted repository.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[test]
fn fmt_normalizes_once_and_is_then_a_no_op() {
    let repo = TempRepo::charted();
    let messy = r#"{"notes": [], "short": "Messy", "refs": [{"governs": "b", "title": "B", "page": 20}, {"page": 10, "title": "A", "governs": "a"}],
        "fs": [{"name": "z/", "role": "last", "node": false}, {"name": "a/", "role": "first", "node": false}],
        "entry_points": ["x"], "conventions": ["c"], "is": "Messy.", "charted": "2026-01-02", "path": "backend/crates"}"#;
    repo.write("backend/crates/NODE.json", messy);
    let first = repo.ok(&["fmt"]);
    assert_eq!(first, "fmt: backend/crates/NODE.json\n");
    let formatted = repo.read("backend/crates/NODE.json");
    let expected = nodes::schema::Node::parse(messy)
        .expect("valid")
        .canonical_json();
    assert_eq!(formatted, expected);
    let canonical_head = "{\n  \"path\": \"backend/crates\",\n  \"charted\": \"2026-01-02\",\n  \"short\": \"Messy\",\n  \"is\": \"Messy.\",\n";
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
            ".  (charted 2026-09-12)\nshort: The whole repository\nis: The whole repository.\nconventions: (none)\n"
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
        "conventions": [], "is": "Messy.", "charted": "2026-01-02", "path": "housing"}"#;
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
