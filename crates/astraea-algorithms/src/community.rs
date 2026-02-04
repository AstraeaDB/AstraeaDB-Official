use std::collections::HashMap;

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::*;

/// Simplified Louvain community detection.
///
/// Greedily assigns nodes to communities by maximizing modularity.
/// The algorithm iterates until no single-node move improves modularity.
///
/// Modularity is defined as:
/// ```text
/// Q = (1 / 2m) * sum_ij [ A_ij - (k_i * k_j) / (2m) ] * delta(c_i, c_j)
/// ```
/// where `m` is the total edge weight, `k_i` is the weighted degree of node `i`,
/// and `delta(c_i, c_j)` is 1 when nodes `i` and `j` belong to the same community.
///
/// Edges are treated as **undirected** for the modularity calculation (each
/// directed edge contributes its weight once; pairs of reciprocal edges
/// contribute independently).
///
/// # Arguments
/// * `graph` - A graph supporting neighbor lookups.
/// * `nodes` - The set of node IDs to partition.
///
/// # Returns
/// A map from `NodeId` to its community identifier (`usize`).
pub fn louvain(
    graph: &dyn GraphOps,
    nodes: &[NodeId],
) -> Result<HashMap<NodeId, usize>> {
    let n = nodes.len();
    if n == 0 {
        return Ok(HashMap::new());
    }

    // Build node -> index mapping for fast lookups.
    let node_to_idx: HashMap<NodeId, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, &nid)| (nid, i))
        .collect();

    // --- Pre-compute adjacency information ---
    // For each node, store the list of (neighbor_index, edge_weight).
    // Treat edges as undirected by using Direction::Both.
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    // Weighted degree of each node.
    let mut k: Vec<f64> = vec![0.0; n];
    for (i, &node) in nodes.iter().enumerate() {
        let neighbors = graph.neighbors(node, Direction::Both)?;
        for (edge_id, neighbor) in neighbors {
            if let Some(&j) = node_to_idx.get(&neighbor) {
                let edge = graph.get_edge(edge_id)?;
                let w = edge.map(|e| e.weight).unwrap_or(1.0);
                adj[i].push((j, w));
                k[i] += w;
            }
        }
    }
    // Each undirected edge was counted twice (once from each endpoint),
    // so m = sum(k_i) / 2.
    let total_weight: f64 = k.iter().sum::<f64>() / 2.0;

    if total_weight == 0.0 {
        // No edges: every node is its own community.
        return Ok(nodes
            .iter()
            .enumerate()
            .map(|(i, &nid)| (nid, i))
            .collect());
    }

    let m2 = 2.0 * total_weight; // 2m, used repeatedly

    // --- Initialize: every node in its own community ---
    let mut community: Vec<usize> = (0..n).collect();
    // Sum of weighted degrees within each community.
    let mut sigma_tot: Vec<f64> = k.clone();

    // --- Iterative greedy optimization ---
    let max_passes = 100;
    for _pass in 0..max_passes {
        let mut improved = false;

        for i in 0..n {
            let current_comm = community[i];
            let ki = k[i];

            // Compute the sum of edge weights from node i to each neighboring community.
            let mut comm_weights: HashMap<usize, f64> = HashMap::new();
            for &(j, w) in &adj[i] {
                let cj = community[j];
                *comm_weights.entry(cj).or_insert(0.0) += w;
            }

            // Weight of edges from i into its own community (k_{i,in}).
            let ki_in = comm_weights.get(&current_comm).copied().unwrap_or(0.0);

            // Modularity delta for removing i from its current community:
            //   delta_remove = ki_in / m - (sigma_tot[c] * ki) / (2m^2)
            // (We do not need to compute the actual delta since we compare
            //  the net gain of moving to each candidate community.)

            let mut best_comm = current_comm;
            let mut best_delta = 0.0_f64;

            for (&cand_comm, &ki_cand) in &comm_weights {
                if cand_comm == current_comm {
                    continue;
                }
                // Modularity gain of moving i from current_comm to cand_comm:
                //   delta_Q = [ ki_cand/m - sigma_tot[cand]*ki/(2m^2) ]
                //           - [ ki_in/m   - (sigma_tot[cur] - ki)*ki/(2m^2) ]
                let sigma_cand = sigma_tot[cand_comm];
                let sigma_cur = sigma_tot[current_comm] - ki;

                let gain = (ki_cand - ki_in) / total_weight
                    - ki * (sigma_cand - sigma_cur) / (m2 * total_weight);

                if gain > best_delta {
                    best_delta = gain;
                    best_comm = cand_comm;
                }
            }

            if best_comm != current_comm {
                // Move node i to best_comm.
                sigma_tot[current_comm] -= ki;
                sigma_tot[best_comm] += ki;
                community[i] = best_comm;
                improved = true;
            }
        }

        if !improved {
            break;
        }
    }

    // --- Compact community IDs to a contiguous range ---
    let mut label_map: HashMap<usize, usize> = HashMap::new();
    let mut next_label = 0usize;

    let mut result = HashMap::with_capacity(n);
    for (i, &nid) in nodes.iter().enumerate() {
        let raw = community[i];
        let compact = *label_map.entry(raw).or_insert_with(|| {
            let l = next_label;
            next_label += 1;
            l
        });
        result.insert(nid, compact);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestGraph;

    /// Two tightly connected clusters joined by a single weak edge.
    ///
    /// Cluster A: nodes 1, 2, 3  (all-to-all, weight 1.0)
    /// Cluster B: nodes 4, 5, 6  (all-to-all, weight 1.0)
    /// Bridge:    3 -> 4 (weight 0.1)  and  4 -> 3 (weight 0.1)
    fn two_cluster_graph() -> TestGraph {
        let g = TestGraph::new();
        for i in 1..=6 {
            g.add_node(i, vec![]);
        }

        let mut eid = 1u64;

        // Cluster A: all-to-all bidirectional
        for &(a, b) in &[(1, 2), (2, 3), (1, 3)] {
            g.add_edge(eid, a, b, 1.0);
            eid += 1;
            g.add_edge(eid, b, a, 1.0);
            eid += 1;
        }

        // Cluster B: all-to-all bidirectional
        for &(a, b) in &[(4, 5), (5, 6), (4, 6)] {
            g.add_edge(eid, a, b, 1.0);
            eid += 1;
            g.add_edge(eid, b, a, 1.0);
            eid += 1;
        }

        // Weak bridge
        g.add_edge(eid, 3, 4, 0.1);
        eid += 1;
        g.add_edge(eid, 4, 3, 0.1);

        g
    }

    #[test]
    fn louvain_detects_two_communities() {
        let g = two_cluster_graph();
        let nodes: Vec<NodeId> = (1..=6).map(NodeId).collect();
        let communities = louvain(&g, &nodes).unwrap();

        // Nodes within the same cluster should share a community.
        assert_eq!(
            communities[&NodeId(1)],
            communities[&NodeId(2)],
            "Nodes 1 and 2 should be in the same community"
        );
        assert_eq!(
            communities[&NodeId(2)],
            communities[&NodeId(3)],
            "Nodes 2 and 3 should be in the same community"
        );
        assert_eq!(
            communities[&NodeId(4)],
            communities[&NodeId(5)],
            "Nodes 4 and 5 should be in the same community"
        );
        assert_eq!(
            communities[&NodeId(5)],
            communities[&NodeId(6)],
            "Nodes 5 and 6 should be in the same community"
        );

        // The two clusters should be in different communities.
        assert_ne!(
            communities[&NodeId(1)],
            communities[&NodeId(4)],
            "Cluster A and Cluster B should be in different communities"
        );
    }

    #[test]
    fn louvain_single_node() {
        let g = TestGraph::new();
        g.add_node(1, vec![]);
        let communities = louvain(&g, &[NodeId(1)]).unwrap();
        assert_eq!(communities.len(), 1);
        assert!(communities.contains_key(&NodeId(1)));
    }

    #[test]
    fn louvain_empty_graph() {
        let g = TestGraph::new();
        let communities = louvain(&g, &[]).unwrap();
        assert!(communities.is_empty());
    }

    #[test]
    fn louvain_no_edges() {
        let g = TestGraph::new();
        for i in 1..=3 {
            g.add_node(i, vec![]);
        }
        let nodes: Vec<NodeId> = (1..=3).map(NodeId).collect();
        let communities = louvain(&g, &nodes).unwrap();
        // With no edges each node stays in its own community.
        let unique_communities: std::collections::HashSet<usize> =
            communities.values().copied().collect();
        assert_eq!(unique_communities.len(), 3);
    }

    #[test]
    fn louvain_fully_connected_clique() {
        // A fully-connected 4-node clique should end up in a single community.
        let g = TestGraph::new();
        for i in 1..=4 {
            g.add_node(i, vec![]);
        }
        let mut eid = 1u64;
        for a in 1..=4u64 {
            for b in (a + 1)..=4 {
                g.add_edge(eid, a, b, 1.0);
                eid += 1;
                g.add_edge(eid, b, a, 1.0);
                eid += 1;
            }
        }

        let nodes: Vec<NodeId> = (1..=4).map(NodeId).collect();
        let communities = louvain(&g, &nodes).unwrap();
        let unique: std::collections::HashSet<usize> = communities.values().copied().collect();
        assert_eq!(unique.len(), 1, "A clique should form a single community");
    }
}
