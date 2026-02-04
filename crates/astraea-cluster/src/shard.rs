use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use astraea_core::types::NodeId;
use crate::partition::PartitionStrategy;

/// Unique shard identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShardId(pub usize);

impl std::fmt::Display for ShardId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "shard-{}", self.0)
    }
}

/// Status of a shard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardStatus {
    Active,
    Inactive,
    Rebalancing,
}

/// Information about a shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardInfo {
    pub id: ShardId,
    pub status: ShardStatus,
    /// Network address of the shard (host:port).
    pub address: String,
    /// Number of nodes stored on this shard.
    pub node_count: u64,
    /// Number of edges stored on this shard.
    pub edge_count: u64,
    /// Replica IDs (for fault tolerance).
    pub replicas: Vec<ShardId>,
}

impl ShardInfo {
    pub fn new(id: ShardId, address: String) -> Self {
        Self {
            id,
            status: ShardStatus::Active,
            address,
            node_count: 0,
            edge_count: 0,
            replicas: Vec::new(),
        }
    }
}

/// Maps shard IDs to their info and provides routing.
pub struct ShardMap {
    shards: HashMap<ShardId, ShardInfo>,
    partitioner: Box<dyn PartitionStrategy>,
}

impl ShardMap {
    pub fn new(partitioner: Box<dyn PartitionStrategy>) -> Self {
        Self {
            shards: HashMap::new(),
            partitioner,
        }
    }

    /// Register a shard.
    pub fn register_shard(&mut self, info: ShardInfo) {
        self.shards.insert(info.id, info);
    }

    /// Remove a shard.
    pub fn remove_shard(&mut self, id: ShardId) -> Option<ShardInfo> {
        self.shards.remove(&id)
    }

    /// Get the shard responsible for a node.
    pub fn shard_for_node(&self, node_id: NodeId) -> ShardId {
        self.partitioner.shard_for_node(node_id)
    }

    /// Get info about a specific shard.
    pub fn get_shard(&self, id: ShardId) -> Option<&ShardInfo> {
        self.shards.get(&id)
    }

    /// Get all active shards.
    pub fn active_shards(&self) -> Vec<&ShardInfo> {
        self.shards.values()
            .filter(|s| s.status == ShardStatus::Active)
            .collect()
    }

    /// Check if a node is local to a given shard.
    pub fn is_local(&self, node_id: NodeId, current_shard: ShardId) -> bool {
        self.shard_for_node(node_id) == current_shard
    }

    /// Number of shards.
    pub fn num_shards(&self) -> usize {
        self.shards.len()
    }

    /// Total number of nodes across all shards.
    pub fn total_nodes(&self) -> u64 {
        self.shards.values().map(|s| s.node_count).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partition::HashPartitioner;

    fn make_shard_map() -> ShardMap {
        let mut map = ShardMap::new(Box::new(HashPartitioner::new(3)));
        map.register_shard(ShardInfo::new(ShardId(0), "host-a:7687".into()));
        map.register_shard(ShardInfo::new(ShardId(1), "host-b:7687".into()));
        map.register_shard(ShardInfo::new(ShardId(2), "host-c:7687".into()));
        map
    }

    #[test]
    fn shard_map_registers_and_retrieves() {
        let map = make_shard_map();
        assert_eq!(map.num_shards(), 3);
        let info = map.get_shard(ShardId(1)).expect("shard 1 should exist");
        assert_eq!(info.address, "host-b:7687");
        assert_eq!(info.status, ShardStatus::Active);
    }

    #[test]
    fn shard_map_routes_nodes() {
        let map = make_shard_map();
        let shard = map.shard_for_node(NodeId(42));
        // The shard ID must be within range.
        assert!(shard.0 < 3);
    }

    #[test]
    fn shard_map_is_local() {
        let map = make_shard_map();
        let node = NodeId(42);
        let target_shard = map.shard_for_node(node);
        assert!(map.is_local(node, target_shard));
        // A different shard should not be local (unless hash collision, so use
        // a shard that differs).
        let other_shard = ShardId((target_shard.0 + 1) % 3);
        assert!(!map.is_local(node, other_shard));
    }

    #[test]
    fn shard_map_active_shards_filters() {
        let mut map = make_shard_map();
        // Mark shard 1 as inactive.
        if let Some(info) = map.shards.get_mut(&ShardId(1)) {
            info.status = ShardStatus::Inactive;
        }
        let active = map.active_shards();
        assert_eq!(active.len(), 2);
        assert!(active.iter().all(|s| s.status == ShardStatus::Active));
    }

    #[test]
    fn shard_map_total_nodes_sums() {
        let mut map = make_shard_map();
        map.shards.get_mut(&ShardId(0)).unwrap().node_count = 100;
        map.shards.get_mut(&ShardId(1)).unwrap().node_count = 200;
        map.shards.get_mut(&ShardId(2)).unwrap().node_count = 50;
        assert_eq!(map.total_nodes(), 350);
    }

    #[test]
    fn shard_map_remove_shard() {
        let mut map = make_shard_map();
        assert_eq!(map.num_shards(), 3);
        let removed = map.remove_shard(ShardId(1));
        assert!(removed.is_some());
        assert_eq!(map.num_shards(), 2);
        assert!(map.get_shard(ShardId(1)).is_none());
    }

    #[test]
    fn shard_id_display() {
        assert_eq!(format!("{}", ShardId(5)), "shard-5");
    }
}
