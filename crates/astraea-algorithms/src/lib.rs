//! Graph algorithms operating over `astraea_core::GraphOps`.
//!
//! Provides `pagerank` (configured via `PageRankConfig`), `louvain`
//! community detection, `degree_centrality`, `betweenness_centrality`,
//! and `connected_components` / `strongly_connected_components`. All
//! algorithms read through the `GraphOps` trait and never touch a
//! storage backend directly.
//!
//! Invariants worth knowing: PageRank traverses only `Direction::Outgoing`
//! and computes only over the supplied node slice (edges to outside
//! targets are dropped). Louvain treats edges as `Direction::Both` and
//! is single-level (flat) — not the multi-level variant. No algorithm
//! here parallelizes across source nodes.

pub mod pagerank;
pub mod components;
pub mod centrality;
pub mod community;

#[cfg(test)]
pub(crate) mod test_support;
