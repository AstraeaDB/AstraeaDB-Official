//! Routing and partitioning skeleton for a future distributed AstraeaDB.
//!
//! Defines the `ClusterCoordinator` and `PartitionStrategy` traits plus
//! `HashPartitioner`, `RangePartitioner`, `ShardId`, `ShardInfo`, and
//! `ShardMap`. This crate is a stub: the only `ClusterCoordinator`
//! implementor is `LocalCoordinator`, which hard-codes a single
//! `ShardId(0)` at `localhost:7687` and reports every `NodeId` as local.
//!
//! There is no consensus, replication, or network transport here — no
//! Raft, no gossip, no RPC. `HashPartitioner` uses a bitwise-specified
//! FNV-1a 64-bit hash so shard assignments are stable across Rust
//! versions, platforms, and processes. Edge ownership always follows
//! the source node's shard.

pub mod partition;
pub mod shard;
pub mod coordinator;

pub use partition::{PartitionStrategy, HashPartitioner, RangePartitioner};
pub use shard::{ShardId, ShardInfo, ShardMap};
pub use coordinator::ClusterCoordinator;
