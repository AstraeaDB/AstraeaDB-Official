//! Foundational types and traits shared across every AstraeaDB crate.
//!
//! Exposes the core ID newtypes (`NodeId`, `EdgeId`, `PageId`, `Lsn`,
//! `TransactionId`), the `Node` / `Edge` / `GraphPath` records, the
//! `Direction` and `DistanceMetric` enums, and the central traits:
//! `StorageEngine`, `TransactionalEngine`, `GraphOps`, and
//! `VectorIndex`. `AstraeaError` plus the `Result` alias are the
//! crate-wide error type; downstream crates should never define their
//! own.
//!
//! Invariants: `NodeId` and `EdgeId` are opaque `u64` newtypes with no
//! density or ordering guarantees, and IDs are always **server-assigned**
//! by `GraphOps::create_node` / `create_edge` — clients never supply
//! them. Node `embedding` dimension is pinned by the HNSW index on first
//! insert; do not mix dimensions within a single store.

pub mod error;
pub mod traits;
pub mod types;

// Re-export commonly used items at the crate root.
pub use error::{AstraeaError, Result};
pub use types::*;
