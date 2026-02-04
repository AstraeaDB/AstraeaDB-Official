use astraea_core::types::{NodeId, EdgeId, Node, Edge};
use astraea_core::error::Result;
use crate::shard::{ShardId, ShardInfo, ShardMap};

/// Operations that must be coordinated across the cluster.
pub trait ClusterCoordinator: Send + Sync {
    /// Get the shard map.
    fn shard_map(&self) -> &ShardMap;

    /// Route a node read to the appropriate shard.
    fn get_node(&self, id: NodeId) -> Result<Option<Node>>;

    /// Route a node write to the appropriate shard.
    fn put_node(&self, node: &Node) -> Result<()>;

    /// Route an edge read to the appropriate shard.
    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>>;

    /// Route an edge write to the appropriate shard.
    fn put_edge(&self, edge: &Edge) -> Result<()>;

    /// Check if a node is local to this shard.
    fn is_local(&self, id: NodeId) -> bool;

    /// Get the current shard ID of this node in the cluster.
    fn current_shard(&self) -> ShardId;
}

/// A single-node coordinator that doesn't actually distribute.
/// This is the default when running without clustering.
pub struct LocalCoordinator {
    shard_map: ShardMap,
    shard_id: ShardId,
}

impl LocalCoordinator {
    pub fn new() -> Self {
        use crate::partition::HashPartitioner;
        let mut shard_map = ShardMap::new(Box::new(HashPartitioner::new(1)));
        let shard_id = ShardId(0);
        shard_map.register_shard(ShardInfo::new(shard_id, "localhost:7687".into()));
        Self { shard_map, shard_id }
    }

    pub fn shard_map(&self) -> &ShardMap {
        &self.shard_map
    }

    pub fn shard_id(&self) -> ShardId {
        self.shard_id
    }

    /// In a single-node setup, every node is local.
    pub fn is_local(&self, _id: NodeId) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_coordinator_routes_to_shard_zero() {
        let coord = LocalCoordinator::new();
        // With a single shard, every node routes to shard 0.
        assert_eq!(coord.shard_map().shard_for_node(NodeId(0)), ShardId(0));
        assert_eq!(coord.shard_map().shard_for_node(NodeId(1)), ShardId(0));
        assert_eq!(coord.shard_map().shard_for_node(NodeId(999)), ShardId(0));
        assert_eq!(coord.shard_map().shard_for_node(NodeId(u64::MAX)), ShardId(0));
    }

    #[test]
    fn local_coordinator_is_always_local() {
        let coord = LocalCoordinator::new();
        assert!(coord.is_local(NodeId(0)));
        assert!(coord.is_local(NodeId(42)));
        assert!(coord.is_local(NodeId(u64::MAX)));
    }

    #[test]
    fn local_coordinator_shard_id() {
        let coord = LocalCoordinator::new();
        assert_eq!(coord.shard_id(), ShardId(0));
    }

    #[test]
    fn local_coordinator_shard_map_has_one_shard() {
        let coord = LocalCoordinator::new();
        assert_eq!(coord.shard_map().num_shards(), 1);
        let active = coord.shard_map().active_shards();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].address, "localhost:7687");
    }
}
