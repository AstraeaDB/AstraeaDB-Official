//! Subgraph extraction for GraphRAG.
//!
//! Extracts a local neighborhood around a center node via BFS, optionally
//! guided by vector similarity. The extracted subgraph is suitable for
//! linearization and inclusion in an LLM context window.

use std::collections::HashSet;

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::{GraphOps, VectorIndex};
use astraea_core::types::{Direction, Edge, Node, NodeId};

/// A local subgraph extracted around a center node.
///
/// Contains the center node, all collected nodes (in BFS order), and all
/// edges whose both endpoints are within the collected node set.
#[derive(Debug, Clone)]
pub struct Subgraph {
    /// The center node around which the subgraph was extracted.
    pub center: NodeId,
    /// All nodes in the subgraph, in BFS discovery order (center first).
    pub nodes: Vec<Node>,
    /// All edges between collected nodes.
    pub edges: Vec<Edge>,
}

/// Extract a local subgraph around `center` via BFS up to `hops` depth.
///
/// Caps at `max_nodes` to fit LLM context windows. Closer nodes (by BFS
/// depth) are preferred -- they appear first in the BFS result and are
/// kept when truncation is necessary.
///
/// # Algorithm
///
/// 1. BFS from `center` up to `hops` using `graph.bfs(center, hops)`.
/// 2. Truncate the discovered node list to `max_nodes` if it exceeds the cap.
/// 3. Collect all `Node` objects via `graph.get_node()`.
/// 4. Collect all edges between collected nodes: for each node, check outgoing
///    edges via `graph.neighbors()`, keeping an edge only if both endpoints
///    are in the node set.
/// 5. Return the assembled `Subgraph`.
pub fn extract_subgraph(
    graph: &dyn GraphOps,
    center: NodeId,
    hops: usize,
    max_nodes: usize,
) -> Result<Subgraph> {
    // Step 1: BFS to discover nodes with their depths.
    let bfs_results = graph.bfs(center, hops)?;

    // Step 2: Truncate to max_nodes (BFS order preserves proximity).
    let node_ids: Vec<NodeId> = bfs_results
        .iter()
        .take(max_nodes)
        .map(|(id, _)| *id)
        .collect();

    let node_set: HashSet<NodeId> = node_ids.iter().copied().collect();

    // Step 3: Collect Node objects.
    let mut nodes: Vec<Node> = Vec::with_capacity(node_ids.len());
    for &nid in &node_ids {
        if let Some(node) = graph.get_node(nid)? {
            nodes.push(node);
        }
    }

    // Step 4: Collect edges between collected nodes.
    let mut edges: Vec<Edge> = Vec::new();
    let mut seen_edges: HashSet<astraea_core::types::EdgeId> = HashSet::new();

    for &nid in &node_ids {
        let neighbors = graph.neighbors(nid, Direction::Outgoing)?;
        for (edge_id, neighbor_id) in neighbors {
            if node_set.contains(&neighbor_id) && seen_edges.insert(edge_id) {
                if let Some(edge) = graph.get_edge(edge_id)? {
                    edges.push(edge);
                }
            }
        }
    }

    Ok(Subgraph {
        center,
        nodes,
        edges,
    })
}

/// Extract a subgraph guided by vector similarity.
///
/// Uses the vector index to find the nearest node to `query_embedding`,
/// then calls [`extract_subgraph`] centered on that node.
///
/// # Errors
///
/// Returns `AstraeaError::QueryExecution` if the vector search returns
/// no results (e.g., the index is empty).
pub fn extract_subgraph_semantic(
    graph: &dyn GraphOps,
    vector_index: &dyn VectorIndex,
    query_embedding: &[f32],
    hops: usize,
    max_nodes: usize,
) -> Result<Subgraph> {
    let results = vector_index.search(query_embedding, 1)?;

    let nearest = results.first().ok_or_else(|| {
        AstraeaError::QueryExecution("vector search returned no results".into())
    })?;

    extract_subgraph(graph, nearest.node_id, hops, max_nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::traits::GraphOps;
    use astraea_core::types::DistanceMetric;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;
    use astraea_vector::HnswVectorIndex;
    use std::sync::Arc;

    /// Build a chain graph: n1 -> n2 -> n3 -> n4 -> n5
    fn build_chain_graph() -> Graph {
        let storage = InMemoryStorage::new();
        let graph = Graph::new(Box::new(storage));

        graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Alice"}),
                None,
            )
            .unwrap(); // n1
        graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Bob"}),
                None,
            )
            .unwrap(); // n2
        graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Charlie"}),
                None,
            )
            .unwrap(); // n3
        graph
            .create_node(
                vec!["Company".into()],
                serde_json::json!({"name": "Acme"}),
                None,
            )
            .unwrap(); // n4
        graph
            .create_node(
                vec!["Company".into()],
                serde_json::json!({"name": "Globex"}),
                None,
            )
            .unwrap(); // n5

        // Chain: 1->2->3->4->5
        graph
            .create_edge(
                NodeId(1),
                NodeId(2),
                "KNOWS".into(),
                serde_json::json!({"since": 2020}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                NodeId(2),
                NodeId(3),
                "KNOWS".into(),
                serde_json::json!({"since": 2021}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                NodeId(3),
                NodeId(4),
                "WORKS_AT".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                NodeId(4),
                NodeId(5),
                "PARTNERS_WITH".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        graph
    }

    #[test]
    fn test_extract_subgraph_basic() {
        let graph = build_chain_graph();

        // Extract 2-hop subgraph from node 1
        let subgraph = extract_subgraph(&graph, NodeId(1), 2, 100).unwrap();

        // Should include n1, n2, n3 (2 hops from n1)
        assert_eq!(subgraph.center, NodeId(1));
        assert_eq!(subgraph.nodes.len(), 3);

        let node_ids: Vec<NodeId> = subgraph.nodes.iter().map(|n| n.id).collect();
        assert!(node_ids.contains(&NodeId(1)));
        assert!(node_ids.contains(&NodeId(2)));
        assert!(node_ids.contains(&NodeId(3)));

        // Should include edges 1->2 and 2->3
        assert_eq!(subgraph.edges.len(), 2);
    }

    #[test]
    fn test_extract_subgraph_max_nodes_cap() {
        let graph = build_chain_graph();

        // Extract up to 10 hops but cap at 2 nodes
        let subgraph = extract_subgraph(&graph, NodeId(1), 10, 2).unwrap();

        // Should only include 2 nodes (n1 and n2, closest by BFS)
        assert_eq!(subgraph.nodes.len(), 2);

        let node_ids: Vec<NodeId> = subgraph.nodes.iter().map(|n| n.id).collect();
        assert!(node_ids.contains(&NodeId(1)));
        assert!(node_ids.contains(&NodeId(2)));
    }

    #[test]
    fn test_extract_subgraph_includes_edges() {
        let graph = build_chain_graph();

        // Extract 3 hops from node 1, but cap at 3 nodes
        // BFS order: n1(0), n2(1), n3(2), n4(3) -- cap at 3 means n1, n2, n3
        let subgraph = extract_subgraph(&graph, NodeId(1), 3, 3).unwrap();

        assert_eq!(subgraph.nodes.len(), 3);

        // Only edges between n1, n2, n3 should be included:
        //   1->2 (KNOWS) and 2->3 (KNOWS) -- yes
        //   3->4 (WORKS_AT) -- no, because n4 is not in the set
        assert_eq!(subgraph.edges.len(), 2);

        for edge in &subgraph.edges {
            let node_ids: HashSet<NodeId> =
                subgraph.nodes.iter().map(|n| n.id).collect();
            assert!(node_ids.contains(&edge.source));
            assert!(node_ids.contains(&edge.target));
        }
    }

    #[test]
    fn test_empty_subgraph() {
        let storage = InMemoryStorage::new();
        let graph = Graph::new(Box::new(storage));

        // Create an isolated node
        let nid = graph
            .create_node(vec!["Solo".into()], serde_json::json!({"name": "Loner"}), None)
            .unwrap();

        let subgraph = extract_subgraph(&graph, nid, 3, 100).unwrap();

        assert_eq!(subgraph.nodes.len(), 1);
        assert_eq!(subgraph.nodes[0].id, nid);
        assert!(subgraph.edges.is_empty());
    }

    #[test]
    fn test_single_hop() {
        let graph = build_chain_graph();

        // 1-hop from n2: discovers n2 (depth 0) and n3 (depth 1, outgoing)
        let subgraph = extract_subgraph(&graph, NodeId(2), 1, 100).unwrap();

        let node_ids: Vec<NodeId> = subgraph.nodes.iter().map(|n| n.id).collect();
        // BFS follows outgoing only, so n2 -> n3
        assert!(node_ids.contains(&NodeId(2)));
        assert!(node_ids.contains(&NodeId(3)));

        // Should include the edge 2->3
        assert!(subgraph.edges.iter().any(|e| e.source == NodeId(2) && e.target == NodeId(3)));
    }

    #[test]
    fn test_extract_subgraph_semantic() {
        let storage = InMemoryStorage::new();
        let vi = Arc::new(HnswVectorIndex::new(3, DistanceMetric::Euclidean));
        let graph = Graph::with_vector_index(Box::new(storage), vi.clone());

        // Create nodes with embeddings
        let _n1 = graph
            .create_node(
                vec!["A".into()],
                serde_json::json!({"name": "Alpha"}),
                Some(vec![1.0, 0.0, 0.0]),
            )
            .unwrap();
        let _n2 = graph
            .create_node(
                vec!["B".into()],
                serde_json::json!({"name": "Beta"}),
                Some(vec![0.0, 1.0, 0.0]),
            )
            .unwrap();
        let _n3 = graph
            .create_node(
                vec!["C".into()],
                serde_json::json!({"name": "Gamma"}),
                Some(vec![0.0, 0.0, 1.0]),
            )
            .unwrap();

        // Edge: n1 -> n2
        graph
            .create_edge(
                NodeId(1),
                NodeId(2),
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        // Semantic search for embedding closest to [1.0, 0.0, 0.0] -> should find n1
        let subgraph =
            extract_subgraph_semantic(&graph, vi.as_ref(), &[1.0, 0.0, 0.0], 1, 100)
                .unwrap();

        // Center should be n1 (closest to query)
        assert_eq!(subgraph.center, NodeId(1));

        // 1-hop from n1: n1 and n2
        let node_ids: Vec<NodeId> = subgraph.nodes.iter().map(|n| n.id).collect();
        assert!(node_ids.contains(&NodeId(1)));
        assert!(node_ids.contains(&NodeId(2)));

        // Edge n1->n2 should be included
        assert_eq!(subgraph.edges.len(), 1);
        assert_eq!(subgraph.edges[0].source, NodeId(1));
        assert_eq!(subgraph.edges[0].target, NodeId(2));
    }
}
