use std::collections::{HashMap, HashSet};

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, EdgeId, NodeId};
use rand::seq::SliceRandom;
use rand::Rng;

use crate::sparse::CSRAdjacency;

/// Configuration for GraphSAGE-style neighbor sampling.
#[derive(Debug, Clone)]
pub struct SamplingConfig {
    /// Number of neighbors to sample at each layer.
    /// Length must equal the number of GNN layers.
    /// Example: `[25, 10]` means sample 25 neighbors at layer 1, 10 at layer 2.
    pub fanout: Vec<usize>,
    /// Number of target (labeled) nodes per mini-batch.
    pub batch_size: usize,
}

/// A sampled computation subgraph for mini-batch training.
#[derive(Debug, Clone)]
pub struct SampledSubgraph {
    /// All node IDs involved (target nodes + sampled neighbors).
    pub node_ids: Vec<NodeId>,
    /// Node ID to local index mapping.
    pub node_to_idx: HashMap<NodeId, usize>,
    /// CSR adjacency built from the sampled edges (using local indices).
    pub adjacency: CSRAdjacency,
    /// Indices of target (labeled) nodes within `node_ids`.
    pub target_indices: Vec<usize>,
}

/// Sample a k-hop neighborhood around target nodes with fixed fanout.
///
/// For each layer (starting from innermost):
/// 1. For each node in the current frontier, sample up to `fanout[layer]` neighbors
/// 2. Add sampled neighbors to the computation graph
/// 3. Build the next frontier from newly discovered nodes
///
/// Returns a `SampledSubgraph` containing all discovered nodes and a CSR adjacency.
pub fn sample_subgraph(
    graph: &dyn GraphOps,
    target_nodes: &[NodeId],
    fanout: &[usize],
    edge_weights: &HashMap<EdgeId, f32>,
    rng: &mut impl Rng,
) -> Result<SampledSubgraph> {
    let mut all_nodes: Vec<NodeId> = Vec::new();
    let mut node_set: HashSet<NodeId> = HashSet::new();
    let mut edges: Vec<(NodeId, NodeId, f32)> = Vec::new();

    // Add target nodes first.
    let target_indices: Vec<usize> = (0..target_nodes.len()).collect();
    for &nid in target_nodes {
        if node_set.insert(nid) {
            all_nodes.push(nid);
        }
    }

    let mut frontier: Vec<NodeId> = target_nodes.to_vec();

    // Sample outward from target nodes layer by layer.
    for &fan in fanout.iter() {
        let mut next_frontier = Vec::new();

        for &node_id in &frontier {
            let neighbors = graph.neighbors(node_id, Direction::Both)?;
            let neighbor_list: Vec<(EdgeId, NodeId)> = neighbors;

            // Sample up to `fan` neighbors.
            let sampled = if neighbor_list.len() <= fan {
                neighbor_list
            } else {
                let mut indices: Vec<usize> = (0..neighbor_list.len()).collect();
                indices.shuffle(rng);
                indices
                    .into_iter()
                    .take(fan)
                    .map(|i| neighbor_list[i])
                    .collect()
            };

            for (edge_id, neighbor_id) in sampled {
                let w = edge_weights.get(&edge_id).copied().unwrap_or(1.0);
                edges.push((node_id, neighbor_id, w));

                if node_set.insert(neighbor_id) {
                    all_nodes.push(neighbor_id);
                    next_frontier.push(neighbor_id);
                }
            }
        }

        frontier = next_frontier;
    }

    // Build local index mapping.
    let node_to_idx: HashMap<NodeId, usize> = all_nodes
        .iter()
        .enumerate()
        .map(|(i, &nid)| (nid, i))
        .collect();

    // Build CSR from sampled edges.
    let num_nodes = all_nodes.len();
    let mut row_entries: Vec<Vec<(usize, f32)>> = vec![vec![]; num_nodes];

    for &(src, dst, w) in &edges {
        if let (Some(&i), Some(&j)) = (node_to_idx.get(&src), node_to_idx.get(&dst)) {
            row_entries[i].push((j, w));
            // Add reverse edge for bidirectional message passing.
            row_entries[j].push((i, w));
        }
    }

    // Deduplicate edges per row.
    for row in &mut row_entries {
        row.sort_by_key(|&(j, _)| j);
        row.dedup_by_key(|entry| entry.0);
    }

    let mut row_ptr = vec![0usize; num_nodes + 1];
    let mut col_idx = Vec::new();
    let mut weights = Vec::new();

    for (i, row) in row_entries.iter().enumerate() {
        for &(j, w) in row {
            col_idx.push(j);
            weights.push(w);
        }
        row_ptr[i + 1] = col_idx.len();
    }

    Ok(SampledSubgraph {
        node_ids: all_nodes,
        node_to_idx,
        adjacency: CSRAdjacency {
            row_ptr,
            col_idx,
            weights,
            num_nodes,
        },
        target_indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;

    fn make_star_graph() -> (Graph, NodeId, Vec<NodeId>) {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let center = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![1.0, 0.0]))
            .unwrap();
        let mut leaves = Vec::new();
        for i in 0..10 {
            let leaf = graph
                .create_node(
                    vec![],
                    serde_json::json!({}),
                    Some(vec![0.0, i as f32 / 10.0]),
                )
                .unwrap();
            graph
                .create_edge(
                    center,
                    leaf,
                    "LINK".into(),
                    serde_json::json!({}),
                    1.0,
                    None,
                    None,
                )
                .unwrap();
            leaves.push(leaf);
        }
        (graph, center, leaves)
    }

    #[test]
    fn test_sample_subgraph_fanout() {
        let (graph, center, _leaves) = make_star_graph();
        let mut rng = rand::thread_rng();

        // Sample with fanout [3]: should get center + at most 3 neighbors.
        let subgraph = sample_subgraph(
            &graph,
            &[center],
            &[3],
            &HashMap::new(),
            &mut rng,
        )
        .unwrap();

        // Target is the center node.
        assert_eq!(subgraph.target_indices, vec![0]);
        // Should have at most 1 (center) + 3 (sampled) = 4 nodes.
        assert!(
            subgraph.node_ids.len() <= 4,
            "expected <= 4 nodes, got {}",
            subgraph.node_ids.len()
        );
        assert!(subgraph.node_ids.len() >= 2, "should have at least center + 1 neighbor");
    }

    #[test]
    fn test_sample_subgraph_all_neighbors() {
        let (graph, center, _leaves) = make_star_graph();
        let mut rng = rand::thread_rng();

        // Fanout larger than actual degree: should get all neighbors.
        let subgraph = sample_subgraph(
            &graph,
            &[center],
            &[100],
            &HashMap::new(),
            &mut rng,
        )
        .unwrap();

        // Center has 10 neighbors, so we should get all 11 nodes.
        assert_eq!(subgraph.node_ids.len(), 11);
    }

    #[test]
    fn test_sample_subgraph_csr() {
        let (graph, center, _leaves) = make_star_graph();
        let mut rng = rand::thread_rng();

        let subgraph = sample_subgraph(
            &graph,
            &[center],
            &[5],
            &HashMap::new(),
            &mut rng,
        )
        .unwrap();

        // CSR should be valid.
        let csr = &subgraph.adjacency;
        assert_eq!(csr.num_nodes, subgraph.node_ids.len());
        assert_eq!(csr.row_ptr.len(), csr.num_nodes + 1);
        assert_eq!(csr.col_idx.len(), csr.weights.len());
    }
}
