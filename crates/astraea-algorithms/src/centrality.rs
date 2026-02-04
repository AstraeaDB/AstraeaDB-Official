use std::collections::{HashMap, HashSet, VecDeque};

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::*;

/// Compute degree centrality for every node in the given set.
///
/// Degree centrality of a node is defined as the number of edges in the
/// specified direction, normalized by `(N - 1)` where `N` is the total
/// number of nodes in the set.
///
/// # Arguments
/// * `graph` - A graph supporting neighbor lookups.
/// * `nodes` - The set of node IDs to evaluate.
/// * `direction` - Which edges to count ([`Direction::Outgoing`],
///   [`Direction::Incoming`], or [`Direction::Both`]).
///
/// # Returns
/// A map from `NodeId` to its degree centrality in `[0.0, 1.0]`.
pub fn degree_centrality(
    graph: &dyn GraphOps,
    nodes: &[NodeId],
    direction: Direction,
) -> Result<HashMap<NodeId, f64>> {
    let n = nodes.len();
    if n <= 1 {
        return Ok(nodes.iter().map(|&id| (id, 0.0)).collect());
    }

    let norm = (n - 1) as f64;
    let mut result = HashMap::with_capacity(n);

    for &node in nodes {
        let neighbors = graph.neighbors(node, direction)?;
        let degree = neighbors.len() as f64;
        result.insert(node, degree / norm);
    }

    Ok(result)
}

/// Compute betweenness centrality for every node using Brandes' algorithm.
///
/// Betweenness centrality measures how often a node lies on the shortest
/// path between all pairs of nodes.  The result is normalized by
/// `((N-1)*(N-2))` for directed graphs.
///
/// This implementation uses BFS (unweighted shortest paths) over
/// [`Direction::Outgoing`] edges.
///
/// # Arguments
/// * `graph` - A graph supporting neighbor lookups.
/// * `nodes` - The set of node IDs to evaluate.
///
/// # Returns
/// A map from `NodeId` to its betweenness centrality.
pub fn betweenness_centrality(
    graph: &dyn GraphOps,
    nodes: &[NodeId],
) -> Result<HashMap<NodeId, f64>> {
    let n = nodes.len();
    let node_set: HashSet<NodeId> = nodes.iter().copied().collect();

    // Accumulator for each node's betweenness.
    let mut centrality: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 0.0)).collect();

    if n <= 2 {
        return Ok(centrality);
    }

    // Brandes' algorithm: for each source, perform a BFS and then
    // back-propagate dependency scores.
    for &source in nodes {
        // --- BFS phase ---
        let mut stack: Vec<NodeId> = Vec::new();
        // predecessors[w] = list of nodes v such that v is on a shortest path from source to w
        let mut predecessors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        // sigma[t] = number of shortest paths from source to t
        let mut sigma: HashMap<NodeId, f64> = HashMap::new();
        // dist[t] = distance from source to t (-1 means not visited)
        let mut dist: HashMap<NodeId, i64> = HashMap::new();

        for &node in nodes {
            predecessors.insert(node, Vec::new());
            sigma.insert(node, 0.0);
            dist.insert(node, -1);
        }

        *sigma.get_mut(&source).unwrap() = 1.0;
        *dist.get_mut(&source).unwrap() = 0;

        let mut queue: VecDeque<NodeId> = VecDeque::new();
        queue.push_back(source);

        while let Some(v) = queue.pop_front() {
            stack.push(v);
            let d_v = dist[&v];

            let neighbors = graph.neighbors(v, Direction::Outgoing)?;
            for (_edge_id, w) in neighbors {
                if !node_set.contains(&w) {
                    continue;
                }

                // w found for the first time?
                if dist[&w] < 0 {
                    queue.push_back(w);
                    *dist.get_mut(&w).unwrap() = d_v + 1;
                }

                // Shortest path to w via v?
                if dist[&w] == d_v + 1 {
                    *sigma.get_mut(&w).unwrap() += sigma[&v];
                    predecessors.get_mut(&w).unwrap().push(v);
                }
            }
        }

        // --- Back-propagation phase ---
        let mut delta: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 0.0)).collect();

        while let Some(w) = stack.pop() {
            for v in predecessors[&w].clone() {
                let contribution = (sigma[&v] / sigma[&w]) * (1.0 + delta[&w]);
                *delta.get_mut(&v).unwrap() += contribution;
            }
            if w != source {
                *centrality.get_mut(&w).unwrap() += delta[&w];
            }
        }
    }

    // Normalize for directed graphs: divide by (N-1)*(N-2).
    let norm = ((n - 1) * (n - 2)) as f64;
    if norm > 0.0 {
        for val in centrality.values_mut() {
            *val /= norm;
        }
    }

    Ok(centrality)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestGraph;

    /// Star graph: center node 1 connected to leaves 2, 3, 4, 5.
    fn star_graph() -> TestGraph {
        let g = TestGraph::new();
        for i in 1..=5 {
            g.add_node(i, vec![]);
        }
        // Bidirectional edges: 1 <-> 2, 1 <-> 3, 1 <-> 4, 1 <-> 5
        g.add_edge(1, 1, 2, 1.0);
        g.add_edge(2, 1, 3, 1.0);
        g.add_edge(3, 1, 4, 1.0);
        g.add_edge(4, 1, 5, 1.0);
        g.add_edge(5, 2, 1, 1.0);
        g.add_edge(6, 3, 1, 1.0);
        g.add_edge(7, 4, 1, 1.0);
        g.add_edge(8, 5, 1, 1.0);
        g
    }

    #[test]
    fn degree_centrality_star_outgoing() {
        let g = star_graph();
        let nodes: Vec<NodeId> = (1..=5).map(NodeId).collect();
        let dc = degree_centrality(&g, &nodes, Direction::Outgoing).unwrap();

        // Center node 1 has out-degree 4; normalized by N-1=4 -> 1.0
        assert!(
            (dc[&NodeId(1)] - 1.0).abs() < 1e-9,
            "Center out-degree centrality should be 1.0, got {}",
            dc[&NodeId(1)]
        );
        // Leaf nodes have out-degree 1; normalized by 4 -> 0.25
        for &leaf in &[NodeId(2), NodeId(3), NodeId(4), NodeId(5)] {
            assert!(
                (dc[&leaf] - 0.25).abs() < 1e-9,
                "Leaf out-degree centrality should be 0.25, got {}",
                dc[&leaf]
            );
        }
    }

    #[test]
    fn degree_centrality_star_both() {
        let g = star_graph();
        let nodes: Vec<NodeId> = (1..=5).map(NodeId).collect();
        let dc = degree_centrality(&g, &nodes, Direction::Both).unwrap();

        // Center node has 4 outgoing + 4 incoming = 8 edges when Direction::Both.
        // Normalized by N-1=4 -> 2.0 (can exceed 1.0 for Direction::Both with bidirectional edges).
        assert!(
            (dc[&NodeId(1)] - 2.0).abs() < 1e-9,
            "Center both-degree centrality should be 2.0, got {}",
            dc[&NodeId(1)]
        );
    }

    #[test]
    fn degree_centrality_single_node() {
        let g = TestGraph::new();
        g.add_node(1, vec![]);
        let dc = degree_centrality(&g, &[NodeId(1)], Direction::Outgoing).unwrap();
        assert_eq!(dc[&NodeId(1)], 0.0);
    }

    /// Path graph: 1 -> 2 -> 3 -> 4
    fn path_graph() -> TestGraph {
        let g = TestGraph::new();
        for i in 1..=4 {
            g.add_node(i, vec![]);
        }
        g.add_edge(1, 1, 2, 1.0);
        g.add_edge(2, 2, 3, 1.0);
        g.add_edge(3, 3, 4, 1.0);
        g
    }

    #[test]
    fn betweenness_centrality_path_graph() {
        let g = path_graph();
        let nodes: Vec<NodeId> = (1..=4).map(NodeId).collect();
        let bc = betweenness_centrality(&g, &nodes).unwrap();

        // In a directed path 1->2->3->4:
        // Node 2 lies on shortest paths: 1->2->3, 1->2->3->4 => 2 paths through it.
        // Node 3 lies on shortest paths: 2->3->4, 1->2->3->4 => 2 paths through it.
        // Nodes 1 and 4 are endpoints and have betweenness 0.
        // After normalization by (N-1)*(N-2) = 3*2 = 6:
        //   B(2) = 2/6, B(3) = 2/6

        assert!(
            bc[&NodeId(1)].abs() < 1e-9,
            "Endpoint node 1 should have betweenness ~0, got {}",
            bc[&NodeId(1)]
        );
        assert!(
            bc[&NodeId(4)].abs() < 1e-9,
            "Endpoint node 4 should have betweenness ~0, got {}",
            bc[&NodeId(4)]
        );

        // Nodes 2 and 3 should have the highest betweenness.
        assert!(
            bc[&NodeId(2)] > bc[&NodeId(1)],
            "Node 2 should have higher betweenness than node 1"
        );
        assert!(
            bc[&NodeId(3)] > bc[&NodeId(4)],
            "Node 3 should have higher betweenness than node 4"
        );

        // Nodes 2 and 3 should have equal betweenness in this directed path.
        assert!(
            (bc[&NodeId(2)] - bc[&NodeId(3)]).abs() < 1e-9,
            "Nodes 2 and 3 should have equal betweenness: {} vs {}",
            bc[&NodeId(2)],
            bc[&NodeId(3)]
        );
    }

    #[test]
    fn betweenness_centrality_two_nodes() {
        let g = TestGraph::new();
        g.add_node(1, vec![]);
        g.add_node(2, vec![]);
        g.add_edge(1, 1, 2, 1.0);

        let nodes = vec![NodeId(1), NodeId(2)];
        let bc = betweenness_centrality(&g, &nodes).unwrap();
        // With only 2 nodes, betweenness is 0 for both (N-2 = 0).
        assert_eq!(bc[&NodeId(1)], 0.0);
        assert_eq!(bc[&NodeId(2)], 0.0);
    }
}
