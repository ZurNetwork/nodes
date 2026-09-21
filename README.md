# nodes

The `NODE.json` normalizer and lookup CLI. A repository is charted as a tree of `NODE.json` files, one per directory that carries information beyond its parent; `nodes` formats them into one canonical shape, validates the tree, and answers lookups so a reader (human or model) can load exactly the nodes a path needs instead of the whole chart.

## The schema

Every `NODE.json` has exactly these fields, in this order; unknown keys are refused.

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
  "refs": [{ "page": 55836674, "title": "The Application Layer", "governs": "use cases" }],
  "notes": ["one note per entry"]
}
```

`path` is the repo-relative directory (`.` for the root). `short` is what `tree` and `ls` print — a noun phrase, no period; `is` is the full sentence. `type` classifies the directory from a closed vocabulary; `tags` label it freely, its categories among them (below). `fs` lists the direct children worth naming; `node: true` means the child carries its own `NODE.json`. `refs` is the only place code points at design: a page id, its title, and what it governs here.

## Type and tags

- `type` — what the directory broadly holds, the one kind that fits best: `code`, `document`, `art`, `media`, `data`, `software` (installed applications, games, servers — not their source). The vocabulary is closed: a word outside it is a schema error, and a new term is a new release. It lives in one table, in `src/vocabulary.rs`; `nodes vocabulary` prints it with its meanings (`--json` for machines), so a charting tool reads it from the binary it runs rather than from this page.
- `tags` — free-form labels, each `word` or `namespace:word`, every part in lowercase letters, digits, `_` and `-` (`wip`, `artist:starsie`, `category:code_source`). No vocabulary closes them. Author order (`fmt` never sorts them), no tag twice.

The one namespace the tool reads is `category:` — what the directory specifically holds. A node carries at least one, and its categories are ranked from most to least fitting by where they stand among each other, whatever other tags sit between them. `--category ui` is short for `--tag category:ui`; every other namespace is selected through `--tag`.

A tree charted before v0.6.0 ranked its categories in a closed `categories` list. `nodes migrate` rewrites every such file of the tree — `"categories": ["ui", "source"]` becomes `"tags": ["category:ui", "category:source"]` — and is the one command that reads one; a second run changes nothing. A mounted tree is migrated from its own root. (A file from before v0.4.0, missing `type` as well, goes through `nodes classify <path> <type> <category>…`.)

## Commands

| Command | Does |
|---|---|
| `nodes tree [--type T] [--tag TAG]… [--category C]` | the whole tree, drawn: each node's name + `short`; filtered, the matches plus the bare ancestors that lead to them |
| `nodes ls [--type T] [--tag TAG]… [--category C]` | every node, one line each: path + `short`; every selected tag must be carried; a category lists best fit first |
| `nodes chain <path>` | root → … → node: what to read to understand a path |
| `nodes get <path> [field]` | one node, or one field of it |
| `nodes refs <page>` | every node citing a page |
| `nodes find <term>` | search the chart: `short`, `is`, `type`, `tags`, `fs[].role`, `conventions`, `notes`, `refs[].title/governs` |
| `nodes grep <term> [--limit N]` | search what is charted: case-insensitive full text of the tree's text files — owning node, file, line, text |
| `nodes recent [path] [--limit N]` | the files modified most recently beneath `path` (default: the root, 10 files), newest first: time (UTC) + file |
| `nodes resolve <query> [--limit N]` | the nodes and files a partial or slightly-off name most likely means, best first (10 by default) |
| `nodes fmt [file…]` | normalize in place (default: every `NODE.json`) |
| `nodes check [--ref-index FILE]` | validate; exit 1 on any error |
| `nodes set <path> <field> <json>` | replace one field, then normalize |
| `nodes add-ref <path> <page> <title> <governs>` / `nodes rm-ref <path> <page>` | ref edits |
| `nodes classify <path> <type> <category>…` | set `type` and the `category:` tags together (the other tags are kept) — also on a file written before they existed |
| `nodes migrate` | rewrite every node written before `tags` existed: `categories` become `category:` tags |
| `nodes touch <path> [--date YYYY-MM-DD]` | set `charted` to today |
| `nodes vocabulary` | the closed `type` vocabulary, each term with its meaning (needs no root) |

`--json` on any command emits JSON for machine consumers; a write command prints nothing in text mode and the updated node with `--json`. The repository root is the nearest ancestor of the current directory holding a `NODE.json` whose `path` is `.`; `--root <dir>` overrides.

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

`find` searches the chart; `grep`, `recent` and `resolve` look at what it charts — every regular file the ignore files let through (below), a mounted tree's included. `NODE.json` files are the chart, not content: they are `find`'s. None of the three loads the chart, so one ill-formed node elsewhere never stops a search.

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

- `grep` matches a literal term line by line and names the node that owns each file — the deepest one above it (`--json`: `{path, file, line, text}`). A long line is cut to a window around the term. A file holding a NUL byte is binary and passed by; one above 8 MiB is passed over with a note on stderr, as is a file that cannot be read. No hits is still exit 0, as with `find`.
- `recent` refuses a `path` that is ignored or no directory rather than listing nothing (`--json`: `{path, file, modified}`, RFC 3339 UTC).
- `resolve` scores every node path and file path against the query, in tiers: the whole path, its last components, the name without its extension, the start of the name, the start of a word in it, anywhere in the name, anywhere in the path, a typo away (one slip from four characters, two from eight), then the query's characters scattered in order. Ties list the shorter path first (`--json`: `{path, kind, score}`, `kind` being `node` or `file`). The table lives in `src/fuzzy.rs`.

## Mounts

A directory beneath the root whose own `NODE.json` declares `path: "."` is another tree, **mounted** here — a documents tree and a few repositories under one home-level root, say. A mount is a boundary for writes and validation, and transparent for reads:

- `tree`, `ls`, `find`, `chain`, `get`, `refs`, `grep`, `recent` and `resolve` cross into it, mounts within mounts included. Every path printed is relative to the root you asked from, so `nodes --root ~ chain "Life/30 Housing"` runs `.` → `Life` → `Life/30 Housing`. `tree` tags a mounted root `[mount]`; `tree` and `ls` carry `"mount": true|false` with `--json`.
- `set`, `touch`, `add-ref`, `rm-ref`, `classify`, `fmt` and `migrate` never cross: a path inside a mount is refused (run the command with `--root <the mount>`), and a bare `fmt` or `migrate` rewrites this tree's files only.
- `check` reads only the mount's root node (it must fit the schema) and holds the parent to the usual listing rule — the mount is an `fs` entry with `node: true`. Nothing beneath the mount's root is validated, and its refs are never held against this tree's `--ref-index`.
- A mounted tree is read exactly as it reads from its own root: its own ignore files govern inside it. To keep a mount out of reads altogether, ignore its directory.

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

The walk that finds `NODE.json` files judges directories only, so a `*.json` line never hides a node. The walks over content (`grep`, `recent`, `resolve`) also judge each file: a pattern such as `*.log` passes it by, and so does a hidden name (`.env`) unless a `!.name` line re-includes it. A line git would not understand is passed over.

## Canonical form

`fmt` is parse → serialize: struct field order, `fs` sorted by `name`, `refs` sorted by `page`, everything else — `tags` included — in author order, two-space indent, one trailing newline. Formatting twice is a no-op.

## Check rules

- schema: every file parses, unknown keys are errors, `charted` is a real date, `type` comes from its vocabulary, every tag is well-formed, and `tags` carry at least one `category:` tag and none twice;
- `path` matches where the file sits;
- every `fs` entry with `node: true` has a `NODE.json` at that path (a nested name like `src/account/` is allowed, so a pass-through directory needs no node of its own) that the walk reaches — a node that is hidden, ignored or inside a mount is an error — and every node or mount directly beneath another node is listed there with `node: true`;
- a mount's root node fits the schema; nothing else of a mounted tree is checked (see Mounts);
- a node cites a page at most once;
- with `--ref-index <file>`: every cited page appears in that file (a line's first run of digits is its page id), and a page whose line carries the superseded marker (`SUPERSEDED` by default, `--superseded-marker` to change) warns.

## Install

Prebuilt binaries are attached to each `vX.Y.Z` [release](https://github.com/ZurNetwork/nodes/releases) for Linux and macOS (x86_64 and aarch64). Or build a release from source, naming its tag:

```bash
cargo install --git https://github.com/ZurNetwork/nodes --locked --tag vX.Y.Z
```
