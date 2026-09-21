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
  "conventions": ["one rule per entry"],
  "entry_points": ["src/lib.rs"],
  "fs": [{ "name": "api/", "role": "the HTTP driver", "node": true }],
  "refs": [{ "page": 55836674, "title": "The Application Layer", "governs": "use cases" }],
  "notes": ["one note per entry"]
}
```

`path` is the repo-relative directory (`.` for the root). `short` is what `tree` and `ls` print — a noun phrase, no period; `is` is the full sentence. `fs` lists the direct children worth naming; `node: true` means the child carries its own `NODE.json`. `refs` is the only place code points at design: a page id, its title, and what it governs here.

## Commands

| Command | Does |
|---|---|
| `nodes tree` | the whole tree, drawn: each node's name + `short` |
| `nodes ls` | every node, one line each: path + `short` |
| `nodes chain <path>` | root → … → node: what to read to understand a path |
| `nodes get <path> [field]` | one node, or one field of it |
| `nodes refs <page>` | every node citing a page |
| `nodes find <term>` | search `short`, `is`, `fs[].role`, `conventions`, `notes`, `refs[].title/governs` |
| `nodes fmt [file…]` | normalize in place (default: every `NODE.json`) |
| `nodes check [--ref-index FILE]` | validate; exit 1 on any error |
| `nodes set <path> <field> <json>` | replace one field, then normalize |
| `nodes add-ref <path> <page> <title> <governs>` / `nodes rm-ref <path> <page>` | ref edits |
| `nodes touch <path> [--date YYYY-MM-DD]` | set `charted` to today |

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

A node beneath a pass-through directory is named by its path from the parent node (`api/v1`). `--json` keeps full paths (`{path, mount, short, is, children}`).

## Mounts

A directory beneath the root whose own `NODE.json` declares `path: "."` is another tree, **mounted** here — a documents tree and a few repositories under one home-level root, say. A mount is a boundary for writes and validation, and transparent for reads:

- `tree`, `ls`, `find`, `chain`, `get` and `refs` cross into it, mounts within mounts included. Every path printed is relative to the root you asked from, so `nodes --root ~ chain "Life/30 Housing"` runs `.` → `Life` → `Life/30 Housing`. `tree` tags a mounted root `[mount]`; `tree` and `ls` carry `"mount": true|false` with `--json`.
- `set`, `touch`, `add-ref`, `rm-ref` and `fmt` never cross: a path inside a mount is refused (run the command with `--root <the mount>`), and a bare `fmt` formats this tree's files only.
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

The walk that finds `NODE.json` files stays out of:

- whatever `.chartignore` or `.gitignore` excludes — real gitignore semantics (globs, anchoring such as `/build/` or `a/b/`, `!` negation, last match wins, and a child of an excluded directory cannot be re-included), read at every directory level, a deeper file overriding a shallower one and `.chartignore` overriding the `.gitignore` beside it. Nothing above the root is read;
- hidden directories (`.name/`), unless a `!.name/` line re-includes them;
- `.git`, `.jj`, `node_modules` and `target`, always.

Patterns are matched against directories; a line git would not understand is passed over.

## Canonical form

`fmt` is parse → serialize: struct field order, `fs` sorted by `name`, `refs` sorted by `page`, everything else in author order, two-space indent, one trailing newline. Formatting twice is a no-op.

## Check rules

- schema: every file parses, unknown keys are errors, `charted` is a real date;
- `path` matches where the file sits;
- every `fs` entry with `node: true` has a `NODE.json` at that path (a nested name like `src/account/` is allowed, so a pass-through directory needs no node of its own) that the walk reaches — a node that is hidden, ignored or inside a mount is an error — and every node or mount directly beneath another node is listed there with `node: true`;
- a mount's root node fits the schema; nothing else of a mounted tree is checked (see Mounts);
- a node cites a page at most once;
- with `--ref-index <file>`: every cited page appears in that file (a line's first run of digits is its page id), and a page whose line carries the superseded marker (`SUPERSEDED` by default, `--superseded-marker` to change) warns.

## Install

Prebuilt binaries are attached to each `vX.Y.Z` release for Linux and macOS (x86_64 and aarch64). Or build from source:

```bash
cargo install --git https://github.com/ZurNetwork/nodes --locked --tag v0.1.0
```
