//! `nodes` — the NODE.inf.json normalizer and lookup CLI.
//!
//! A repository is charted as a tree of `NODE.inf.json` files, one per directory
//! that carries information beyond its parent, committed with the code by default
//! or kept out of git (`init --gitignore`) as a local-only tree; the design pages
//! each node cites live in one committed `NODE.refs.json` per tree
//! ([`treerefs`]). This crate holds the schema (the serde structs in
//! [`schema`]), the canonical form (`nodes fmt`), the validation rules
//! (`nodes check`), the read/write commands over the tree, and the lookups into
//! the files it charts (`nodes grep`, `recent`, `resolve`).

pub mod check;
pub mod cli;
pub mod date;
pub mod error;
pub mod fuzzy;
pub mod grep;
pub mod ignores;
pub mod refindex;
pub mod render;
pub mod repo;
pub mod schema;
pub mod tag;
pub mod tree;
pub mod treerefs;
pub mod vocabulary;
