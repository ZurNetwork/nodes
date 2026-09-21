//! `nodes` — the NODE.json normalizer and lookup CLI.
//!
//! A repository is charted as a tree of `NODE.json` files, one per directory
//! that carries information beyond its parent. This crate holds the schema
//! (the serde structs in [`schema`]), the canonical form (`nodes fmt`), the
//! validation rules (`nodes check`) and the read/write commands over the tree.

pub mod check;
pub mod cli;
pub mod date;
pub mod error;
pub mod ignores;
pub mod refindex;
pub mod render;
pub mod repo;
pub mod schema;
pub mod tag;
pub mod tree;
pub mod vocabulary;
