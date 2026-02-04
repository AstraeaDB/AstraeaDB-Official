use std::collections::HashMap;

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::*;

/// Configuration for the PageRank algorithm.
pub struct PageRankConfig {
    /// Damping factor (probability of following a link vs. teleporting).
    /// Typical value: 0.85.
    pub damping: f64,
    /// Maximum number of power-iteration steps.
    pub max_iterations: usize,
    /// Convergence threshold: stop when the L1 norm of the rank
    /// difference between consecutive iterations falls below this value.
    pub tolerance: f64,
}

impl Default for PageRankConfig {
    fn default() -> Self {
        Self {
            damping: 0.85,
            max_iterations: 100,
            tolerance: 1e-6,
        }
    }
}

/// Compute PageRank scores for the given set of nodes.
///
/// The algorithm performs power iteration over the link structure
/// exposed by [`GraphOps::neighbors`] in the [`Direction::Outgoing`] direction.
///
/// # Arguments
/// * `graph` - A graph supporting neighbor lookups.
/// * `nodes` - The set of node IDs to include in the computation.
/// * `config` - Damping factor, iteration limit, and convergence tolerance.
///
/// # Returns
/// A map from `NodeId` to its PageRank score.  Scores sum to approximately 1.0.
pub fn pagerank(
    graph: &dyn GraphOps,
    nodes: &[NodeId],
    config: &PageRankConfig,
) -> Result<HashMap<NodeId, f64>> {
    let n = nodes.len();
    if n == 0 {
        return Ok(HashMap::new());
    }

    let initial_rank = 1.0 / n as f64;
    let teleport = (1.0 - config.damping) / n as f64;

    // Current ranks.
    let mut ranks: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, initial_rank)).collect();

    // Pre-compute out-degrees so we only query the graph once per node.
    let mut out_degree: HashMap<NodeId, usize> = HashMap::with_capacity(n);
    for &node in nodes {
        let neighbors = graph.neighbors(node, Direction::Outgoing)?;
        out_degree.insert(node, neighbors.len());
    }

    // Pre-compute the incoming neighbor lists (only over the node set).
    // For each node, store the list of (source, out_degree_of_source).
    let node_set: std::collections::HashSet<NodeId> = nodes.iter().copied().collect();
    let mut incoming: HashMap<NodeId, Vec<NodeId>> = HashMap::with_capacity(n);
    for &node in nodes {
        incoming.insert(node, Vec::new());
    }
    for &node in nodes {
        let neighbors = graph.neighbors(node, Direction::Outgoing)?;
        for (_edge_id, target) in neighbors {
            if node_set.contains(&target) {
                incoming.get_mut(&target).unwrap().push(node);
            }
        }
    }

    // Identify dangling nodes (out-degree 0). Their rank is redistributed
    // uniformly to all nodes each iteration.
    let dangling_nodes: Vec<NodeId> = nodes
        .iter()
        .copied()
        .filter(|nid| out_degree[nid] == 0)
        .collect();

    for _iter in 0..config.max_iterations {
        // Sum of ranks held by dangling nodes, to be redistributed.
        let dangling_sum: f64 = dangling_nodes.iter().map(|nid| ranks[nid]).sum();
        let dangling_contrib = config.damping * dangling_sum / n as f64;

        let mut new_ranks: HashMap<NodeId, f64> = HashMap::with_capacity(n);
        let mut diff = 0.0_f64;

        for &node in nodes {
            let mut sum = 0.0_f64;
            if let Some(sources) = incoming.get(&node) {
                for &src in sources {
                    let deg = out_degree[&src];
                    if deg > 0 {
                        sum += ranks[&src] / deg as f64;
                    }
                }
            }
            let new_rank = teleport + dangling_contrib + config.damping * sum;
            diff += (new_rank - ranks[&node]).abs();
            new_ranks.insert(node, new_rank);
        }

        ranks = new_ranks;

        if diff < config.tolerance {
            break;
        }
    }

    Ok(ranks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestGraph;

    /// Build a small triangle graph: 1 -> 2 -> 3 -> 1
    fn triangle_graph() -> TestGraph {
        let g = TestGraph::new();
        g.add_node(1, vec!["A".into()]);
        g.add_node(2, vec!["B".into()]);
        g.add_node(3, vec!["C".into()]);
        g.add_edge(1, 1, 2, 1.0);
        g.add_edge(2, 2, 3, 1.0);
        g.add_edge(3, 3, 1, 1.0);
        g
    }

    #[test]
    fn pagerank_triangle_sums_to_one() {
        let g = triangle_graph();
        let nodes = vec![NodeId(1), NodeId(2), NodeId(3)];
        let config = PageRankConfig::default();
        let ranks = pagerank(&g, &nodes, &config).unwrap();

        let sum: f64 = ranks.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-4,
            "PageRank scores should sum to ~1.0, got {}",
            sum
        );
    }

    #[test]
    fn pagerank_triangle_uniform() {
        // In a symmetric cycle, all nodes should have equal rank.
        let g = triangle_graph();
        let nodes = vec![NodeId(1), NodeId(2), NodeId(3)];
        let config = PageRankConfig::default();
        let ranks = pagerank(&g, &nodes, &config).unwrap();

        let expected = 1.0 / 3.0;
        for &nid in &nodes {
            let r = ranks[&nid];
            assert!(
                (r - expected).abs() < 1e-4,
                "Node {:?} should have rank ~{:.4}, got {:.4}",
                nid,
                expected,
                r
            );
        }
    }

    #[test]
    fn pagerank_converges() {
        // Star graph: 1 -> 2, 1 -> 3, 1 -> 4.  No back-edges.
        let g = TestGraph::new();
        g.add_node(1, vec![]);
        g.add_node(2, vec![]);
        g.add_node(3, vec![]);
        g.add_node(4, vec![]);
        g.add_edge(1, 1, 2, 1.0);
        g.add_edge(2, 1, 3, 1.0);
        g.add_edge(3, 1, 4, 1.0);

        let nodes = vec![NodeId(1), NodeId(2), NodeId(3), NodeId(4)];
        let config = PageRankConfig {
            damping: 0.85,
            max_iterations: 200,
            tolerance: 1e-8,
        };
        let ranks = pagerank(&g, &nodes, &config).unwrap();

        let sum: f64 = ranks.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-4,
            "PageRank scores should sum to ~1.0, got {}",
            sum
        );
        // Leaf nodes (2,3,4) should all have the same rank.
        let r2 = ranks[&NodeId(2)];
        let r3 = ranks[&NodeId(3)];
        let r4 = ranks[&NodeId(4)];
        assert!((r2 - r3).abs() < 1e-6);
        assert!((r3 - r4).abs() < 1e-6);
    }

    #[test]
    fn pagerank_empty_graph() {
        let g = TestGraph::new();
        let nodes: Vec<NodeId> = vec![];
        let ranks = pagerank(&g, &nodes, &PageRankConfig::default()).unwrap();
        assert!(ranks.is_empty());
    }

    #[test]
    fn pagerank_five_nodes_with_hub() {
        // Hub-and-spoke: 2->1, 3->1, 4->1, 5->1  plus  1->2
        let g = TestGraph::new();
        for i in 1..=5 {
            g.add_node(i, vec![]);
        }
        g.add_edge(1, 2, 1, 1.0);
        g.add_edge(2, 3, 1, 1.0);
        g.add_edge(3, 4, 1, 1.0);
        g.add_edge(4, 5, 1, 1.0);
        g.add_edge(5, 1, 2, 1.0);

        let nodes: Vec<NodeId> = (1..=5).map(NodeId).collect();
        let ranks = pagerank(&g, &nodes, &PageRankConfig::default()).unwrap();

        // Node 1 receives links from 4 other nodes; it should have the highest rank.
        let r1 = ranks[&NodeId(1)];
        for &nid in &[NodeId(2), NodeId(3), NodeId(4), NodeId(5)] {
            assert!(
                r1 > ranks[&nid],
                "Hub node 1 (rank {:.4}) should rank higher than {:?} (rank {:.4})",
                r1,
                nid,
                ranks[&nid]
            );
        }
    }
}
