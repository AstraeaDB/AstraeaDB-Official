use astraea_core::types::NodeId;
use crate::shard::ShardId;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Strategy for partitioning nodes across shards.
pub trait PartitionStrategy: Send + Sync {
    /// Determine which shard owns the given node.
    fn shard_for_node(&self, node_id: NodeId) -> ShardId;

    /// Determine which shard owns a given edge (based on source node).
    fn shard_for_edge(&self, source: NodeId) -> ShardId;

    /// Number of shards in this partitioning scheme.
    fn num_shards(&self) -> usize;
}

/// Hash-based partitioning: shard_id = hash(node_id) % num_shards
pub struct HashPartitioner {
    num_shards: usize,
}

impl HashPartitioner {
    pub fn new(num_shards: usize) -> Self {
        assert!(num_shards > 0, "must have at least 1 shard");
        Self { num_shards }
    }
}

impl PartitionStrategy for HashPartitioner {
    fn shard_for_node(&self, node_id: NodeId) -> ShardId {
        let mut hasher = DefaultHasher::new();
        node_id.0.hash(&mut hasher);
        ShardId(hasher.finish() as usize % self.num_shards)
    }

    fn shard_for_edge(&self, source: NodeId) -> ShardId {
        self.shard_for_node(source)
    }

    fn num_shards(&self) -> usize {
        self.num_shards
    }
}

/// Range-based partitioning: shard_id determined by node ID ranges.
pub struct RangePartitioner {
    /// Boundary values. Shard i owns nodes in [boundaries[i], boundaries[i+1]).
    boundaries: Vec<u64>,
}

impl RangePartitioner {
    /// Create a range partitioner with evenly-spaced boundaries.
    pub fn uniform(num_shards: usize, max_id: u64) -> Self {
        let step = max_id / num_shards as u64;
        let mut boundaries: Vec<u64> = (0..num_shards).map(|i| i as u64 * step).collect();
        boundaries.push(max_id);
        Self { boundaries }
    }

    /// Create from explicit boundary values.
    pub fn from_boundaries(boundaries: Vec<u64>) -> Self {
        assert!(boundaries.len() >= 2, "need at least 2 boundaries");
        Self { boundaries }
    }
}

impl PartitionStrategy for RangePartitioner {
    fn shard_for_node(&self, node_id: NodeId) -> ShardId {
        for i in 0..self.boundaries.len() - 1 {
            if node_id.0 < self.boundaries[i + 1] {
                return ShardId(i);
            }
        }
        ShardId(self.boundaries.len() - 2) // last shard
    }

    fn shard_for_edge(&self, source: NodeId) -> ShardId {
        self.shard_for_node(source)
    }

    fn num_shards(&self) -> usize {
        self.boundaries.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_partitioner_distributes_across_shards() {
        let partitioner = HashPartitioner::new(4);
        let mut seen_shards = std::collections::HashSet::new();
        // Hash enough node IDs that we expect to hit multiple shards.
        for i in 0..100 {
            let shard = partitioner.shard_for_node(NodeId(i));
            assert!(shard.0 < 4);
            seen_shards.insert(shard.0);
        }
        // With 100 nodes and 4 shards, we should see more than 1 shard used.
        assert!(seen_shards.len() > 1, "expected distribution across multiple shards");
    }

    #[test]
    fn hash_partitioner_deterministic() {
        let partitioner = HashPartitioner::new(8);
        let node = NodeId(42);
        let shard1 = partitioner.shard_for_node(node);
        let shard2 = partitioner.shard_for_node(node);
        assert_eq!(shard1, shard2, "same node must always map to same shard");
    }

    #[test]
    fn hash_partitioner_edge_follows_source() {
        let partitioner = HashPartitioner::new(4);
        let source = NodeId(99);
        assert_eq!(
            partitioner.shard_for_node(source),
            partitioner.shard_for_edge(source),
        );
    }

    #[test]
    fn range_partitioner_correct_ranges() {
        // Boundaries: [0, 100, 200, 300]  ->  3 shards
        let partitioner = RangePartitioner::from_boundaries(vec![0, 100, 200, 300]);
        assert_eq!(partitioner.shard_for_node(NodeId(0)), ShardId(0));
        assert_eq!(partitioner.shard_for_node(NodeId(50)), ShardId(0));
        assert_eq!(partitioner.shard_for_node(NodeId(99)), ShardId(0));
        assert_eq!(partitioner.shard_for_node(NodeId(100)), ShardId(1));
        assert_eq!(partitioner.shard_for_node(NodeId(199)), ShardId(1));
        assert_eq!(partitioner.shard_for_node(NodeId(200)), ShardId(2));
        assert_eq!(partitioner.shard_for_node(NodeId(299)), ShardId(2));
    }

    #[test]
    fn range_partitioner_overflow_goes_to_last_shard() {
        let partitioner = RangePartitioner::from_boundaries(vec![0, 100, 200, 300]);
        // Node ID beyond the last boundary falls into the last shard.
        assert_eq!(partitioner.shard_for_node(NodeId(500)), ShardId(2));
    }

    #[test]
    fn range_partitioner_uniform_splits_evenly() {
        let partitioner = RangePartitioner::uniform(4, 1000);
        assert_eq!(partitioner.num_shards(), 4);
        // Shard 0: [0, 250), Shard 1: [250, 500), Shard 2: [500, 750), Shard 3: [750, 1000)
        assert_eq!(partitioner.shard_for_node(NodeId(0)), ShardId(0));
        assert_eq!(partitioner.shard_for_node(NodeId(249)), ShardId(0));
        assert_eq!(partitioner.shard_for_node(NodeId(250)), ShardId(1));
        assert_eq!(partitioner.shard_for_node(NodeId(500)), ShardId(2));
        assert_eq!(partitioner.shard_for_node(NodeId(750)), ShardId(3));
    }

    #[test]
    #[should_panic(expected = "must have at least 1 shard")]
    fn hash_partitioner_zero_shards_panics() {
        HashPartitioner::new(0);
    }

    #[test]
    #[should_panic(expected = "need at least 2 boundaries")]
    fn range_partitioner_too_few_boundaries_panics() {
        RangePartitioner::from_boundaries(vec![0]);
    }
}
