# nodes

The `NODE.inf.json` normalizer and lookup CLI. A repository is charted as a tree of `NODE.inf.json` files, one per directory that carries information beyond its parent; `nodes` formats them into one canonical shape, validates the tree, and answers lookups so a reader (human or model) can load exactly the nodes a path needs instead of the whole chart.

A tree keeps two kinds of data, two ways:

- **The nodes** — what each directory is, its rules, its named children — are derived from the code and can be re-charted, so they stay out of version control: `nodes init` and `nodes migrate` add `**/*.inf.json` to the root's `.gitignore`. A tree lives in the checkout where it was charted; a fresh clone, a worktree or CI has none.
- **The refs** — which design page governs what — are authored, so they are committed: one [`NODE.refs.json`](#the-refs-file) at the tree's root holds every node's. They are versioned, reviewed with the code, and checked in CI, where there is no node tree (`nodes check --ref-index`).

Tools that honour `.gitignore` — ripgrep, fd, an editor's search, `eza` — hide the node files by default; `nodes` always finds them.

## The schema

Every `NODE.inf.json` has exactly these fields, in this order; unknown keys — `refs` among them — are refused.

```json
{
  "path": "backend/crates",
  "charted": "2026-09-12",
  "short": "A few words, for listings",
  "is": "One sentence: what this directory IS.",
  "type": "code",
  "tags": ["category:source", "category:tests", "wip"],
  "conventions": ["one rule per entry"],
  "entry_points": ["src/lib.rs"],
  "fs": [{ "name": "api/", "role": "the HTTP driver", "node": true }],
  "notes": ["one note per entry"]
}
```

`path` is the repo-relative directory (`.` for the root). `short` is what `tree` and `ls` print — a noun phrase, no period; `is` is the full sentence. `type` classifies the directory from a closed vocabulary; `tags` label it freely, its categories among them (below). `fs` lists the direct children worth naming; `node: true` means the child carries its own `NODE.inf.json`. A node's `refs` are kept apart, in the tree's refs file; every read shows them as part of the node.

## The refs file

`NODE.refs.json`, at the root of a tree, committed, holds the refs of all its nodes: the only place code points at design.

```json
{
  "backend/crates": [
    { "page": 11763713, "title": "Domains and Applications", "governs": "the dependency rule" },
    { "page": 55836674, "title": "The Application Layer", "governs": "use cases" }
  ]
}
```

Each key is a node's path; each ref is a page id, its title, and what it governs there. A path that cites nothing has no entry, and a tree that cites nothing has no file. `add-ref`, `rm-ref` and `set <path> refs` write it; `get`, `chain`, `find` and every `--json` node carry each node's refs, so a reader never opens it. `nodes refs <page>` and `nodes check` read it with no node tree present, and in such a checkout — a worktree, a fresh clone — `add-ref` and `rm-ref` edit it alone (for any directory there), so a ref change ships in the same commit as the code it describes. Leave its formatting to `nodes fmt`.

## Type and tags

- `type` — what the directory broadly holds, the one kind that fits best: `code`, `document`, `art`, `media`, `data`, `software` (installed applications, games, servers — not their source). The vocabulary is closed: a word outside it is a schema error, and a new term is a new release. It lives in one table, in `src/vocabulary.rs`; `nodes vocabulary` prints it with its meanings (`--json` for machines), so a charting tool reads it from the binary it runs rather than from this page.
- `tags` — free-form labels, each `word` or `namespace:word`, every part in lowercase letters, digits, `_` and `-` (`wip`, `artist:starsie`, `category:code_source`). No vocabulary closes them. Author order (`fmt` never sorts them), no tag twice.

The one namespace the tool reads is `category:` — what the directory specifically holds. A node carries at least one, and its categories are ranked from most to least fitting by where they stand among each other, whatever other tags sit between them. `--category ui` is short for `--tag category:ui`; every other namespace is selected through `--tag`.

A tree charted before v0.6.0 ranked its categories in a closed `categories` list; `nodes migrate` turns them into tags (see [Migrating](#migrating)).

## Starting a tree

`nodes init` makes the current directory (or `--root`) a tree's root. It first adds `**/*.inf.json` to that directory's `.gitignore`, so no node file is ever seen untracked; then, given a type and categories, it seeds the root node — `short` is the directory's name, `is` starts empty, for `nodes set` to fill:

```
$ nodes init code project tooling
init: .gitignore
init: NODE.inf.json
```

It never walks up to an ancestor's tree, and it is safe to run again: an existing root node is left as it is (a note on stderr says so), and with no type it only sees to the `.gitignore` line. A directory that holds a node of a tree above is no root, and is refused; seeding a root beneath another tree makes a mount of it, and a note says to list it there.

The `.gitignore` line:

- `--no-gitignore` (on `init` and `migrate`) leaves `.gitignore` alone: for a repository that is not yours, which keeps the node files out through its own exclude file (`.git/info/exclude`), or a tree outside any repository;
- it is not added twice, nor beside a line that already says it (`*.inf.json`), nor after a `!` line that re-includes node files — that `.gitignore` is left as it is, with a note;
- a `.gitignore` with CRLF line endings gets CRLF; a missing one is created.

No file is written through a symbolic link, and each is written beside itself first, then renamed into place, so a concurrent reader sees the old file or the new one.

## Migrating

Up to v0.6 a node lived in a `NODE.json` that held its own refs. `nodes migrate`, run from a tree's root, brings the whole tree up to date in one go:

- the root's `.gitignore` gains `**/*.inf.json` (unless `--no-gitignore`);
- every node's refs move into the tree's `NODE.refs.json`;
- every `NODE.json` becomes a `NODE.inf.json` beside it;
- a file from before v0.6.0 has its ranked `categories` rewritten into tags — `"categories": ["ui", "source"]` becomes `"tags": ["category:ui", "category:source"]`;
- every file is normalized, like `fmt`.

```
$ nodes migrate
migrate: .gitignore
migrate: NODE.refs.json
migrate: NODE.json → NODE.inf.json
migrate: backend/NODE.json → backend/NODE.inf.json
```

- **Nothing half-done.** Every file is read before any is written, so one that does not fit stops the run with nothing changed. The refs file is written before any node, and each node is renamed — atomically — before it is rewritten, so a run cut short (a crash, a full disk, a file another program holds) loses nothing: it says how far it got, and the next run finishes it. A run on a migrated tree changes nothing.
- **Both files in one directory** — what checking out an older commit brings back: when the `NODE.json` says what the `NODE.inf.json` says, it goes; when they differ, the run is refused.
- **Mounts.** A mounted tree — whichever file holds its root — is migrated from its own root; `migrate` names each one it leaves alone. A directory holding no tree is refused, not given a `.gitignore`.
- `--json` lists every file visited as `{file, changed}`, plus `renamed_from` for a moved node.

**Until then, the tree is read as it is.** Every read — `tree`, `ls`, `chain`, `get`, `refs`, `find`, `grep`, `recent`, `resolve`, `check` — works on a tree whose root holds a `NODE.json`, with a note on stderr. The writes — `set`, `touch`, `add-ref`, `rm-ref`, `fmt`, `init` — wait for `nodes migrate`. So installing the binary and migrating each tree are separate steps. The one write an old tree takes is `classify`, which readies a file from before v0.4.0 (no `type`) for `migrate`, in place: `nodes classify <path> <type> <category>…`.

**Committing it.** In git, the old `NODE.json` files leave the index as deletions, and `NODE.refs.json` and the `.gitignore` line arrive: `git add -u && git add NODE.refs.json`, then commit (`git add -A` would stage unrelated work too).

**Other checkouts of the repository** lose their tree when they pull that commit: its deleted `NODE.json` files were their nodes. To keep it, migrate there first and hand the two committed files back to git before pulling — `nodes migrate`, `git restore .gitignore`, `rm NODE.refs.json`, `git pull` — or pull, and re-chart.

## Commands

| Command | Does |
|---|---|
| `nodes tree [--type T] [--tag TAG]… [--category C]` | the whole tree, drawn: each node's name + `short`; filtered, the matches plus the bare ancestors that lead to them |
| `nodes ls [--type T] [--tag TAG]… [--category C]` | every node, one line each: path + `short`; every selected tag must be carried; a category lists best fit first |
| `nodes chain <path>` | root → … → node: what to read to understand a path |
| `nodes get <path> [field]` | one node, or one field of it |
| `nodes refs <page>` | every node citing a page — read from `NODE.refs.json`, so it answers with no node tree too |
| `nodes find <term>` | search the chart: `short`, `is`, `type`, `tags`, `fs[].role`, `conventions`, `notes`, `refs[].title/governs` |
| `nodes grep <term> [--limit N]` | search what is charted: case-insensitive full text of the tree's text files — owning node, file, line, text |
| `nodes recent [path] [--limit N]` | the files modified most recently beneath `path` (default: the root, 10 files), newest first: time (UTC) + file |
| `nodes resolve <query> [--limit N]` | the nodes and files a partial or slightly-off name most likely means, best first (10 by default) |
| `nodes fmt [file…]` | normalize in place (default: every `NODE.inf.json`, and `NODE.refs.json`) |
| `nodes check [--ref-index FILE]` | validate; exit 1 on any error. With no node tree here, the refs file alone |
| `nodes set <path> <field> <json>` | replace one field, then normalize |
| `nodes add-ref <path> <page> <title> <governs>` / `nodes rm-ref <path> <page>` | ref edits, into `NODE.refs.json` — with no node tree here, for any directory |
| `nodes classify <path> <type> <category>…` | set `type` and the `category:` tags together (the other tags are kept) — also on a file written before they existed, and on a tree that has not migrated yet |
| `nodes init [--no-gitignore] [<type> <category>…]` | make this directory a tree's root: `**/*.inf.json` into `.gitignore`, then — given a type — a root node |
| `nodes migrate [--no-gitignore]` | bring an older tree up to date: `NODE.json` → `NODE.inf.json`, refs → `NODE.refs.json`, `categories` → `category:` tags, `**/*.inf.json` into `.gitignore` |
| `nodes touch <path> [--date YYYY-MM-DD]` | set `charted` to today |
| `nodes vocabulary` | the closed `type` vocabulary, each term with its meaning (needs no root) |

`--json` on any command emits JSON for machine consumers; a write command prints nothing in text mode and the updated node with `--json`. The repository root is the nearest ancestor of the current directory that roots a tree: its node file (a `NODE.inf.json`, or a `NODE.json` from before v0.7) declares `path` `.`, or it holds a `NODE.refs.json` — all a checkout with no node tree has. `init` starts in the current directory instead; `--root <dir>` overrides. A tree lives in the checkout where it was charted: from another worktree or clone, read it with `--root <that checkout>`.

```
$ nodes tree
.  The Zurfur monorepo: Rust backend, SvelteKit frontend, contract, lexicons
├── backend  The Rust backend (ports and adapters)
│   └── crates  The twelve workspace crates
│       ├── adapter-atproto  Public boundary: the AT Protocol adapter
│       └── api  The axum HTTP driver
│           └── src  Composition + cross-cutting HTTP concerns
└── contract  The protobuf API contract, authoritative over both tiers
    └── zurfur  Pass-through to api/v1
        └── api/v1  The v1 proto corpus
```

A filtered tree keeps each match and the ancestors above it; an ancestor that only leads to a match is drawn bare, without its `short` (and `--json` entries gain `"match": true|false`):

```
$ nodes --root ~ tree --category finance
.
└── Life  [mount]
    ├── 20 Money  invoices, receipts, taxes, tuition, bank letters
    └── 30 Housing  one folder per home
```

A node beneath a pass-through directory is named by its path from the parent node (`api/v1`). `--json` keeps full paths (`{path, mount, type, tags, short, is, children}`).

## Looking into files

`find` searches the chart; `grep`, `recent` and `resolve` look at what it charts — every regular file the ignore files let through (below), a mounted tree's included. Node files and `NODE.refs.json` are the chart, not content: they are `find`'s. None of the three loads the chart, so one ill-formed node elsewhere never stops a search.

```
$ nodes grep "graceful shutdown"
backend/crates/api  backend/crates/api/src/main.rs:41: // graceful shutdown: drain, then close the pool
$ nodes recent backend --limit 2
2026-09-20T14:03:11Z  backend/crates/api/src/main.rs
2026-09-19T08:12:40Z  backend/Cargo.lock
$ nodes resolve shcema
backend/crates/domain/src/schema.rs
contract/schema  [node]
```

- `grep` matches a literal term line by line and names the node that owns each file — the deepest one above it; `.` in a checkout with no node tree (`--json`: `{path, file, line, text}`). A long line is cut to a window around the term. A file holding a NUL byte is binary and passed by; one above 8 MiB is passed over with a note on stderr, as is a file that cannot be read. No hits is still exit 0, as with `find`.
- `recent` refuses a `path` that is ignored or no directory rather than listing nothing (`--json`: `{path, file, modified}`, RFC 3339 UTC).
- `resolve` scores every node path and file path against the query, in tiers: the whole path, its last components, the name without its extension, the start of the name, the start of a word in it, anywhere in the name, anywhere in the path, a typo away (one slip from four characters, two from eight), then the query's characters scattered in order. Ties list the shorter path first (`--json`: `{path, kind, score}`, `kind` being `node` or `file`). The table lives in `src/fuzzy.rs`.

## Mounts

A directory beneath the root that roots a tree of its own — its node file declares `path: "."`, or it holds a `NODE.refs.json` — is another tree, **mounted** here — a documents tree and a few repositories under one home-level root, say. A mount is a boundary for writes and validation, and transparent for reads:

- `tree`, `ls`, `find`, `chain`, `get`, `refs`, `grep`, `recent` and `resolve` cross into it, mounts within mounts included. Every path printed is relative to the root you asked from, so `nodes --root ~ chain "Life/30 Housing"` runs `.` → `Life` → `Life/30 Housing`. `tree` tags a mounted root `[mount]`; `tree` and `ls` carry `"mount": true|false` with `--json`.
- `set`, `touch`, `add-ref`, `rm-ref`, `classify`, `fmt` and `migrate` never cross: a path inside a mount is refused (run the command with `--root <the mount>`), and a bare `fmt` or `migrate` rewrites this tree's files only.
- `check` reads only the mount's root node (it must fit the schema) and holds the parent to the usual listing rule — the mount is an `fs` entry with `node: true`. Nothing beneath the mount's root is validated, and its refs are never held against this tree's `--ref-index`.
- A mounted tree is read exactly as it reads from its own root: its own ignore files govern inside it, its own refs file holds its refs, and a mount from before v0.7 is read from its `NODE.json` files. To keep a mount out of reads altogether, ignore its directory.
- A node file of the other release left in a tree — a `NODE.json` in a tree that has migrated — is passed over with a note on stderr; `migrate` settles it.

```
$ nodes --root ~ tree
.  Home
├── Life  [mount]  Personal documents
│   └── 30 Housing  Lease, utilities
└── code/zurfur  [mount]  The Zurfur monorepo
    └── backend  The Rust backend (ports and adapters)
```

## Ignore files

Every walk stays out of:

- whatever `.chartignore` or `.gitignore` excludes — real gitignore semantics (globs, anchoring such as `/build/` or `a/b/`, `!` negation, last match wins, and a child of an excluded directory cannot be re-included), read at every directory level, a deeper file overriding a shallower one and `.chartignore` overriding the `.gitignore` beside it. Nothing above the root is read;
- hidden directories (`.name/`), unless a `!.name/` line re-includes them;
- `.git`, `.jj`, `node_modules` and `target`, always.

The walk that finds `NODE.inf.json` files judges directories only, so a `*.json` line — or the `**/*.inf.json` that keeps the nodes out of git — never hides a node. The walks over content (`grep`, `recent`, `resolve`) also judge each file: a pattern such as `*.log` passes it by, and so does a hidden name (`.env`) unless a `!.name` line re-includes it. A line git would not understand is passed over.

## Canonical form

`fmt` is parse → serialize: struct field order, `fs` sorted by `name`, everything else — `tags` included — in author order, two-space indent, one trailing newline. `NODE.refs.json` lists its paths in tree order and each path's refs by `page`, in the same indent. Formatting twice is a no-op.

## Check rules

- schema: every file parses, unknown keys are errors (a `refs` in a node file among them), `charted` is a real date, `type` comes from its vocabulary, every tag is well-formed, and `tags` carry at least one `category:` tag and none twice; `NODE.refs.json` parses too — every key a node path, every ref `{page, title, governs}`;
- `path` matches where the file sits;
- every `fs` entry with `node: true` has a `NODE.inf.json` at that path (a nested name like `src/account/` is allowed, so a pass-through directory needs no node of its own) that the walk reaches — a node that is hidden, ignored or inside a mount is an error — and every node or mount directly beneath another node is listed there with `node: true`;
- a mount's root node fits the schema; nothing else of a mounted tree is checked (see Mounts);
- a node cites a page at most once;
- `NODE.refs.json` cites pages only for directories that exist (an error otherwise) and that carry a node (a warning otherwise: chart the directory, or move the refs to the node that should carry them);
- with `--ref-index <file>`: every cited page appears in that file (a line's first run of digits is its page id), and a page whose line carries the superseded marker (`SUPERSEDED` by default, `--superseded-marker` to change) warns.

**With no node tree here** — only the committed `NODE.refs.json`, as in CI or a fresh clone — `check` checks the refs alone: every path a directory, no page twice per path, and the `--ref-index` rules. Its summary says so: `refs only (no node tree here): 12 refs, 0 errors, 0 warnings`. `--json` reports carry `refs` (how many were checked) and `tree` (whether a node tree was there).

## Install

Prebuilt binaries are attached to each `vX.Y.Z` [release](https://github.com/ZurNetwork/nodes/releases) for Linux and macOS (x86_64 and aarch64). Or build a release from source, naming its tag:

```bash
cargo install --git https://github.com/ZurNetwork/nodes --locked --tag vX.Y.Z
```
