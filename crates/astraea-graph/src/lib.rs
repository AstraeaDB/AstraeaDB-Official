//! High-level graph operations layered on a pluggable `StorageEngine`.
//!
//! `Graph` owns a boxed `dyn StorageEngine` (and an optional
//! `dyn VectorIndex`) and implements `astraea_core::GraphOps` plus
//! traversals: `bfs`, `dfs`, `shortest_path_unweighted`,
//! `shortest_path_dijkstra`, and their temporal `_at` variants which
//! filter by `ValidityInterval::contains(timestamp)`. `InMemoryStorage`
//! (under the `test-utils` feature) is the in-process backend used by
//! tests and the current CLI `Serve` path.
//!
//! Invariants: every mutation goes through the `StorageEngine` trait —
//! never reach into a backend-specific type. `create_edge` verifies both
//! endpoints exist, `delete_node` cascades through outgoing and incoming
//! edges, and node/edge IDs come from monotonic `AtomicU64` counters
//! starting at 1 (0 is reserved). `shortest_path_dijkstra` requires
//! non-negative weights; negative weights silently produce wrong answers.

pub mod graph;
pub mod traversal;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

#[cfg(test)]
mod cybersecurity_test;

pub use graph::Graph;
