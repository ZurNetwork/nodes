# CLAUDE.md

`nodes` is the NODE.inf.json normalizer and lookup CLI: a small Rust program that formats, validates and queries a repository's per-directory `NODE.inf.json` chart files (committed with the code by default, or kept local-only when `nodes init`/`nodes migrate` run with `--gitignore`, which adds `**/*.inf.json` to `.gitignore`) and each tree's one committed `NODE.refs.json`, where the authored refs live. This repository commits its own tree. Zurfur is its first consumer; the tool itself knows nothing about Zurfur beyond the schema.

The human is the Engineer and owns every decision; Claude proposes. Field names, check severities and output shapes are the Engineer's call.

New Rust follows the Zurfur semantic style rulebook (Confluence DESIGN page `37519361`, "Code Style — Semantic Rulings (Rust)" — fetch it, don't quote from memory): domain-meaningful primitives behind newtypes; multi-line constructions named into a `let` first (tests too); `ok_or_else`/`let-else`/`map_err` over match-plumbing; std traits (`FromStr`, `TryFrom`, `Display`) before bespoke constructors; clarity beats brevity. Formatting is rustfmt's job.

## Commands

```bash
cargo build                # build the `nodes` binary
cargo test                 # unit + integration tests
cargo fmt --all --check    # CI `format`
cargo clippy --all-targets -- -D warnings   # CI `lint`
```

## Branch Strategy
- `main` — stable; the only long-lived branch. All PRs target it. **Never push directly to `main`** (a GitHub ruleset enforces this).
- `feature/*` — a unit of new work, branched from `main` (e.g. `feature/<ticket>-short-slug`).
- `bug/*` — a bug fix, branched from `main`.
- `chore/*`, `docs/*`, `hotfix/*` — maintenance, documentation, and urgent fixes.

## Commits
- PRs merge via **Squash and merge** only (other methods are disabled), so `main` keeps **one commit per PR**; the branch itself may carry several granular commits. Merged branches auto-delete.
- Required CI checks (`format` · `lint` · `test`) must pass before merge.
- Local `/understand` + `/remember` briefings live in `.understand/` (tracked in the repo).
