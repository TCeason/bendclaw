//! `search` — semantic-ish code search for agents.
//!
//! Finds code by concept or identifier and returns ranked snippets, so the
//! agent can locate relevant code without the "grep �� read whole file → grep
//! again" loop. Pure Rust: tree-sitter chunking + BM25 + code-aware reranking,
//! no embedding model, no network, no GPU.
//!
//! The index is built lazily on first search per root and cached in-process,
//! so the one-time build cost (~hundreds of ms on large repos) is paid once
//! per session. Queries themselves run in well under a millisecond.

mod chunk;
mod index;
mod rank;
mod tool;

pub use tool::SearchTool;
