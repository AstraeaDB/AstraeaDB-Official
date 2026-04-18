use astraea_core::types::NodeId;
use crate::shard::ShardId;

/// Strategy for partitioning nodes across shards.
pub trait PartitionStrategy: Send + Sync {
    /// Determine which shard owns the given node.
    fn shard_for_node(&self, node_id: NodeId) -> ShardId;

    /// Determine which shard owns a given edge (based on source node).
    fn shard_for_edge(&self, source: NodeId) -> ShardId;

    /// Number of shards in this partitioning scheme.
    fn num_shards(&self) -> usize;
}

/// Hash-based partitioning: shard_id = hash(node_id) % num_shards.
///
/// Uses FNV-1a (64-bit) — a fixed, bitwise-specified hash. astraeadb-issues.md
/// #23. The previous implementation used `std::collections::hash_map::DefaultHasher`,
/// which is documented as **not** stable across Rust versions: a shard
/// assignment persisted to disk (or shared between nodes built with
/// different rustc versions) could silently move under the cluster.
pub struct HashPartitioner {
    num_shards: usize,
}

impl HashPartitioner {
    pub fn new(num_shards: usize) -> Self {
        assert!(num_shards > 0, "must have at least 1 shard");
        Self { num_shards }
    }
}

/// FNV-1a 64-bit hash. Specified by the FNV-1a standard; stable across
/// compiler versions, platforms, and releases.
fn fnv1a_u64(mut input: u64) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash: u64 = FNV_OFFSET;
    for _ in 0..8 {
        let byte = (input & 0xff) as u64;
        hash ^= byte;
        hash = hash.wrapping_mul(FNV_PRIME);
        input >>= 8;
    }
    hash
}

impl PartitionStrategy for HashPartitioner {
    fn shard_for_node(&self, node_id: NodeId) -> ShardId {
        ShardId((fnv1a_u64(node_id.0) as usize) % self.num_shards)
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

    #[test]
    fn hash_partitioner_stable_across_runs() {
        // astraeadb-issues.md #23. The FNV-1a hash is bitwise-specified,
        // so the shard assignment for any given (node_id, num_shards) is
        // a known constant — not subject to rustc version drift.
        let p = HashPartitioner::new(8);
        // Golden values: if these ever change, any persisted shard
        // assignment in production would silently relocate. Treat that
        // as a breaking change requiring a migration plan.
        assert_eq!(p.shard_for_node(NodeId(0)), ShardId(5));
        assert_eq!(p.shard_for_node(NodeId(1)), ShardId(4));
        assert_eq!(p.shard_for_node(NodeId(42)), ShardId(7));
        assert_eq!(p.shard_for_node(NodeId(u64::MAX)), ShardId(5));
    }
}
