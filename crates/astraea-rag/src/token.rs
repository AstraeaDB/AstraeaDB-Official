//! Token budget estimation and budget-aware subgraph extraction.
//!
//! Provides a rough token count estimation (approximately 4 characters per
//! token) and a budget-constrained extraction that incrementally builds a
//! subgraph, linearizing after each node addition, stopping when the
//! estimated token count would exceed the budget.

use std::collections::HashSet;

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, NodeId};

use crate::linearize::{TextFormat, linearize_subgraph};
use crate::subgraph::Subgraph;

/// Estimate the number of tokens for a string.
///
/// Uses the common approximation of ~4 characters per token, which is a
/// reasonable average for English text with typical LLM tokenizers.
pub fn estimate_tokens(text: &str) -> usize {
    // Ceiling division to avoid underestimating by 1 for short strings.
    (text.len() + 3) / 4
}

/// Extract a subgraph with a token budget constraint.
///
/// Performs BFS from `center` up to `hops`, then incrementally adds nodes
/// (in BFS order, closest first) to the subgraph. After each node addition,
/// the subgraph is linearized in the given `format` and the token count is
/// estimated. Extraction stops when adding the next node would cause the
/// estimated token count to exceed `token_budget`.
///
/// Returns the final subgraph and its linearized text representation.
pub fn extract_with_budget(
    graph: &dyn GraphOps,
    center: NodeId,
    hops: usize,
    token_budget: usize,
    format: TextFormat,
) -> Result<(Subgraph, String)> {
    // Step 1: BFS to discover all candidate nodes.
    let bfs_results = graph.bfs(center, hops)?;

    let mut included_nodes: Vec<astraea_core::types::Node> = Vec::new();
    let mut included_ids: HashSet<NodeId> = HashSet::new();
    let mut last_text = String::new();

    // Step 2: Incrementally add nodes, checking budget after each addition.
    for (nid, _depth) in &bfs_results {
        let node = match graph.get_node(*nid)? {
            Some(n) => n,
            None => continue,
        };

        // Tentatively add this node.
        included_nodes.push(node);
        included_ids.insert(*nid);

        // Collect edges between currently included nodes.
        let edges = collect_edges_for_nodes(graph, &included_ids)?;

        let tentative_subgraph = Subgraph {
            center,
            nodes: included_nodes.clone(),
            edges,
        };

        let text = linearize_subgraph(&tentative_subgraph, format);
        let tokens = estimate_tokens(&text);

        if tokens > token_budget && included_nodes.len() > 1 {
            // Adding this node exceeds the budget. Remove it and stop.
            included_nodes.pop();
            included_ids.remove(nid);
            break;
        }

        last_text = text;

        // If we are already at exactly the budget, stop adding more.
        if tokens >= token_budget {
            break;
        }
    }

    // Build the final subgraph.
    let final_edges = collect_edges_for_nodes(graph, &included_ids)?;
    let subgraph = Subgraph {
        center,
        nodes: included_nodes,
        edges: final_edges,
    };

    // Re-linearize in case we broke out before updating last_text
    // (e.g., if the very first node already exceeds the budget).
    if last_text.is_empty() {
        last_text = linearize_subgraph(&subgraph, format);
    }

    Ok((subgraph, last_text))
}

/// Collect all edges between the given set of node IDs.
fn collect_edges_for_nodes(
    graph: &dyn GraphOps,
    node_ids: &HashSet<NodeId>,
) -> Result<Vec<astraea_core::types::Edge>> {
    let mut edges = Vec::new();
    let mut seen_edges: HashSet<astraea_core::types::EdgeId> = HashSet::new();

    for &nid in node_ids {
        let neighbors = graph.neighbors(nid, Direction::Outgoing)?;
        for (edge_id, neighbor_id) in neighbors {
            if node_ids.contains(&neighbor_id) && seen_edges.insert(edge_id) {
                if let Some(edge) = graph.get_edge(edge_id)? {
                    edges.push(edge);
                }
            }
        }
    }

    Ok(edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::traits::GraphOps;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;

    #[test]
    fn test_estimate_tokens() {
        // Empty string -> 0 tokens
        assert_eq!(estimate_tokens(""), 0);

        // 4 chars -> 1 token
        assert_eq!(estimate_tokens("abcd"), 1);

        // 5 chars -> 2 tokens (ceiling)
        assert_eq!(estimate_tokens("abcde"), 2);

        // 8 chars -> 2 tokens
        assert_eq!(estimate_tokens("abcdefgh"), 2);

        // 100 chars -> 25 tokens
        let text = "a".repeat(100);
        assert_eq!(estimate_tokens(&text), 25);
    }

    #[test]
    fn test_extract_with_budget() {
        let storage = InMemoryStorage::new();
        let graph = Graph::new(Box::new(storage));

        // Create a chain: n1 -> n2 -> n3 -> n4 -> n5
        for i in 1..=5 {
            graph
                .create_node(
                    vec!["Thing".into()],
                    serde_json::json!({"name": format!("Node{}", i)}),
                    None,
                )
                .unwrap();
        }
        for i in 1..=4 {
            graph
                .create_edge(
                    NodeId(i),
                    NodeId(i + 1),
                    "NEXT".into(),
                    serde_json::json!({}),
                    1.0,
                    None,
                    None,
                )
                .unwrap();
        }

        // Extract with a very large budget -- should get all nodes
        let (subgraph_big, text_big) =
            extract_with_budget(&graph, NodeId(1), 10, 100_000, TextFormat::Triples)
                .unwrap();
        assert_eq!(subgraph_big.nodes.len(), 5);
        assert!(!text_big.is_empty());

        // Extract with a very small budget -- should get fewer nodes
        // The triples format for 1 node is empty (no edges from just 1 node),
        // so with a tiny budget we should still get at least the center node.
        let (subgraph_small, _text_small) =
            extract_with_budget(&graph, NodeId(1), 10, 1, TextFormat::Structured)
                .unwrap();
        assert!(subgraph_small.nodes.len() < 5);
        // Must include at least the center node
        assert!(subgraph_small.nodes.len() >= 1);
        assert_eq!(subgraph_small.center, NodeId(1));

        // With a moderate budget, should get more nodes than tiny but fewer than max
        let (subgraph_med, text_med) =
            extract_with_budget(&graph, NodeId(1), 10, 30, TextFormat::Triples).unwrap();
        let token_count = estimate_tokens(&text_med);
        // Token count should be within budget (or at budget for single-node case)
        assert!(
            token_count <= 30 || subgraph_med.nodes.len() == 1,
            "token count {} exceeds budget 30 with {} nodes",
            token_count,
            subgraph_med.nodes.len()
        );
    }
}
