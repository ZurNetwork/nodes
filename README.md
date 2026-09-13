# nodes

The `NODE.json` normalizer and lookup CLI. A repository is charted as a tree of `NODE.json` files, one per directory that carries information beyond its parent; `nodes` formats them into one canonical shape, validates the tree, and answers lookups so a reader (human or model) can load exactly the nodes a path needs instead of the whole chart.

## The schema

Every `NODE.json` has exactly these fields, in this order; unknown keys are refused.

```json
{
  "path": "backend/crates",
  "charted": "2026-09-12",
  "is": "One sentence: what this directory IS.",
  "conventions": ["one rule per entry"],
  "entry_points": ["src/lib.rs"],
  "fs": [{ "name": "api/", "role": "the HTTP driver", "node": true }],
  "refs": [{ "page": 55836674, "title": "The Application Layer", "governs": "use cases" }],
  "notes": ["one note per entry"]
}
```

`path` is the repo-relative directory (`.` for the root). `fs` lists the direct children worth naming; `node: true` means the child carries its own `NODE.json`. `refs` is the only place code points at design: a page id, its title, and what it governs here.

## Commands

| Command | Does |
|---|---|
| `nodes tree` | the whole tree, drawn: each node's name + `is` |
| `nodes ls` | every node, one line each |
| `nodes chain <path>` | root → … → node: what to read to understand a path |
| `nodes get <path> [field]` | one node, or one field of it |
| `nodes refs <page>` | every node citing a page |
| `nodes find <term>` | search `is`, `fs[].role`, `conventions`, `notes`, `refs[].title/governs` |
| `nodes fmt [file…]` | normalize in place (default: every `NODE.json`) |
| `nodes check [--ref-index FILE]` | validate; exit 1 on any error |
| `nodes set <path> <field> <json>` | replace one field, then normalize |
| `nodes add-ref <path> <page> <title> <governs>` / `nodes rm-ref <path> <page>` | ref edits |
| `nodes touch <path> [--date YYYY-MM-DD]` | set `charted` to today |

`--json` on any command emits JSON for machine consumers; a write command prints nothing in text mode and the updated node with `--json`. The repository root is the nearest ancestor of the current directory holding a `NODE.json` whose `path` is `.`; `--root <dir>` overrides.

```
$ nodes tree
.  Zurfur — an AT Protocol-native art-commission platform: …
├── backend  The Rust backend: a pure `domain` core, …
│   └── crates  Twelve workspace crates arranged as a hexagon: …
│       ├── adapter-atproto  The public data boundary: …
│       └── api  The HTTP driving adapter over `composition::Runtime`: …
│           └── src  Composition (lib/main) plus the cross-cutting HTTP concerns …
└── contract  The API contract — protobuf as the independent IDL, …
    └── zurfur  Pass-through namespace directory …
        └── api/v1  The v1 corpus — package `zurfur.api.v1`, …
```

A node beneath a pass-through directory is named by its path from the parent node (`api/v1`). `--json` keeps full paths (`{path, is, children}`).

## Canonical form

`fmt` is parse → serialize: struct field order, `fs` sorted by `name`, `refs` sorted by `page`, everything else in author order, two-space indent, one trailing newline. Formatting twice is a no-op.

## Check rules

- schema: every file parses, unknown keys are errors, `charted` is a real date;
- `path` matches where the file sits;
- every `fs` entry with `node: true` has a `NODE.json` at that path (a nested name like `src/account/` is allowed, so a pass-through directory needs no node of its own), and every node directly beneath another node is listed there with `node: true`;
- a node cites a page at most once;
- with `--ref-index <file>`: every cited page appears in that file (a line's first run of digits is its page id), and a page whose line carries the superseded marker (`SUPERSEDED` by default, `--superseded-marker` to change) warns.

## Install

Prebuilt binaries are attached to each `vX.Y.Z` release for Linux and macOS (x86_64 and aarch64). Or build from source:

```bash
cargo install --git https://github.com/ZurNetwork/nodes --locked --tag v0.1.0
```
