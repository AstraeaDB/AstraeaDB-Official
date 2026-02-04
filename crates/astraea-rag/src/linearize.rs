//! Subgraph linearization for LLM context windows.
//!
//! Converts a [`Subgraph`] into a textual representation suitable for
//! inclusion in an LLM prompt. Four formats are supported:
//!
//! - **Prose**: natural-language paragraphs
//! - **Structured**: indented tree with arrows
//! - **Triples**: `(subject, predicate, object)` triples
//! - **Json**: compact JSON with `nodes` and `edges` arrays

use std::collections::HashMap;

use astraea_core::types::NodeId;

use crate::subgraph::Subgraph;

/// Output format for subgraph linearization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFormat {
    /// Natural language paragraphs describing each node and its outgoing edges.
    Prose,
    /// Indented tree format with arrow notation for edges.
    Structured,
    /// `(subject, predicate, object)` triples, one per line.
    Triples,
    /// Compact JSON object with `nodes` and `edges` arrays.
    Json,
}

/// Linearize a subgraph into a textual representation.
///
/// The output is a single string suitable for inclusion in an LLM context
/// window. The format is determined by the `format` parameter.
pub fn linearize_subgraph(subgraph: &Subgraph, format: TextFormat) -> String {
    match format {
        TextFormat::Prose => linearize_prose(subgraph),
        TextFormat::Structured => linearize_structured(subgraph),
        TextFormat::Triples => linearize_triples(subgraph),
        TextFormat::Json => linearize_json(subgraph),
    }
}

/// Get a display name for a node: first label or "Node", plus the "name"
/// property if available, otherwise the node ID.
fn node_display_name(node: &astraea_core::types::Node) -> String {
    if let Some(name) = node.properties.get("name").and_then(|v| v.as_str()) {
        name.to_string()
    } else {
        format!("{}", node.id)
    }
}

/// Get the primary label of a node, or "Node" if unlabeled.
fn node_label(node: &astraea_core::types::Node) -> &str {
    node.labels.first().map(|s| s.as_str()).unwrap_or("Node")
}

/// Format a node's properties as a compact string, excluding the "name" field
/// (which is used as the display name).
fn format_properties(props: &serde_json::Value) -> String {
    if let Some(obj) = props.as_object() {
        let pairs: Vec<String> = obj
            .iter()
            .filter(|(k, _)| k.as_str() != "name")
            .map(|(k, v)| {
                let val_str = match v {
                    serde_json::Value::String(s) => format!("\"{}\"", s),
                    other => other.to_string(),
                };
                format!("{}: {}", k, val_str)
            })
            .collect();
        if pairs.is_empty() {
            String::new()
        } else {
            format!("({})", pairs.join(", "))
        }
    } else {
        String::new()
    }
}

/// Format edge properties as a compact string.
fn format_edge_properties(props: &serde_json::Value) -> String {
    if let Some(obj) = props.as_object() {
        if obj.is_empty() {
            return String::new();
        }
        let pairs: Vec<String> = obj
            .iter()
            .map(|(k, v)| {
                let val_str = match v {
                    serde_json::Value::String(s) => format!("\"{}\"", s),
                    other => other.to_string(),
                };
                format!("{}: {}", k, val_str)
            })
            .collect();
        format!(" {{{}}}", pairs.join(", "))
    } else {
        String::new()
    }
}

/// Prose format: natural-language paragraphs.
///
/// Example:
/// ```text
/// Alice is a Person (age: 30, role: "engineer"). Alice KNOWS Bob (since: 2020).
/// Bob is a Person (age: 35, role: "manager"). Bob MANAGES Alice.
/// ```
fn linearize_prose(subgraph: &Subgraph) -> String {
    let node_map: HashMap<NodeId, &astraea_core::types::Node> =
        subgraph.nodes.iter().map(|n| (n.id, n)).collect();

    let mut lines: Vec<String> = Vec::new();

    for node in &subgraph.nodes {
        let name = node_display_name(node);
        let label = node_label(node);
        let props = format_properties(&node.properties);

        let mut parts: Vec<String> = Vec::new();

        // Node description
        if props.is_empty() {
            parts.push(format!("{} is a {}", name, label));
        } else {
            parts.push(format!("{} is a {} {}", name, label, props));
        }

        // Outgoing edges from this node
        for edge in &subgraph.edges {
            if edge.source == node.id {
                if let Some(target) = node_map.get(&edge.target) {
                    let target_name = node_display_name(target);
                    let edge_props = format_edge_properties(&edge.properties);
                    parts.push(format!(
                        "{} {} {}{}",
                        name, edge.edge_type, target_name, edge_props
                    ));
                }
            }
        }

        lines.push(format!("{}.", parts.join(". ")));
    }

    lines.join("\n")
}

/// Structured format: indented tree with arrows.
///
/// Example:
/// ```text
/// Node [Person: Alice] (age: 30, role: "engineer")
///   -[KNOWS {since: 2020}]-> [Person: Bob] (age: 35)
///   -[WORKS_AT]-> [Company: Acme] (industry: "tech")
/// ```
fn linearize_structured(subgraph: &Subgraph) -> String {
    let node_map: HashMap<NodeId, &astraea_core::types::Node> =
        subgraph.nodes.iter().map(|n| (n.id, n)).collect();

    let mut lines: Vec<String> = Vec::new();

    for node in &subgraph.nodes {
        let name = node_display_name(node);
        let label = node_label(node);
        let props = format_properties(&node.properties);

        if props.is_empty() {
            lines.push(format!("Node [{}: {}]", label, name));
        } else {
            lines.push(format!("Node [{}: {}] {}", label, name, props));
        }

        // Outgoing edges from this node
        for edge in &subgraph.edges {
            if edge.source == node.id {
                if let Some(target) = node_map.get(&edge.target) {
                    let target_name = node_display_name(target);
                    let target_label = node_label(target);
                    let target_props = format_properties(&target.properties);
                    let edge_props = format_edge_properties(&edge.properties);

                    if target_props.is_empty() {
                        lines.push(format!(
                            "  -[{}{}]-> [{}: {}]",
                            edge.edge_type, edge_props, target_label, target_name
                        ));
                    } else {
                        lines.push(format!(
                            "  -[{}{}]-> [{}: {}] {}",
                            edge.edge_type, edge_props, target_label, target_name, target_props
                        ));
                    }
                }
            }
        }
    }

    lines.join("\n")
}

/// Triples format: `(subject, predicate, object)` one per line.
///
/// Example:
/// ```text
/// (Alice, KNOWS, Bob)
/// (Alice, WORKS_AT, Acme)
/// ```
fn linearize_triples(subgraph: &Subgraph) -> String {
    let node_map: HashMap<NodeId, &astraea_core::types::Node> =
        subgraph.nodes.iter().map(|n| (n.id, n)).collect();

    let mut lines: Vec<String> = Vec::new();

    for edge in &subgraph.edges {
        if let (Some(source), Some(target)) =
            (node_map.get(&edge.source), node_map.get(&edge.target))
        {
            let src_name = node_display_name(source);
            let tgt_name = node_display_name(target);
            lines.push(format!("({}, {}, {})", src_name, edge.edge_type, tgt_name));
        }
    }

    lines.join("\n")
}

/// JSON format: compact JSON with `nodes` and `edges` arrays.
fn linearize_json(subgraph: &Subgraph) -> String {
    let nodes: Vec<serde_json::Value> = subgraph
        .nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id.0,
                "labels": n.labels,
                "properties": n.properties,
            })
        })
        .collect();

    let edges: Vec<serde_json::Value> = subgraph
        .edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "source": e.source.0,
                "target": e.target.0,
                "type": e.edge_type,
                "properties": e.properties,
            })
        })
        .collect();

    let doc = serde_json::json!({
        "nodes": nodes,
        "edges": edges,
    });

    doc.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::traits::GraphOps;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;

    /// Build a small test graph for linearization tests.
    /// Alice -[KNOWS {since: 2020}]-> Bob -[WORKS_AT]-> Acme
    fn build_linearize_graph() -> (Graph, Subgraph) {
        let storage = InMemoryStorage::new();
        let graph = Graph::new(Box::new(storage));

        graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Alice", "age": 30}),
                None,
            )
            .unwrap();
        graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Bob", "age": 35}),
                None,
            )
            .unwrap();
        graph
            .create_node(
                vec!["Company".into()],
                serde_json::json!({"name": "Acme", "industry": "tech"}),
                None,
            )
            .unwrap();

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
                "WORKS_AT".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        let subgraph =
            crate::subgraph::extract_subgraph(&graph, NodeId(1), 2, 100).unwrap();

        (graph, subgraph)
    }

    #[test]
    fn test_linearize_prose() {
        let (_graph, subgraph) = build_linearize_graph();
        let text = linearize_subgraph(&subgraph, TextFormat::Prose);

        // Should mention Alice, Bob, Acme
        assert!(text.contains("Alice"));
        assert!(text.contains("Bob"));
        assert!(text.contains("Acme"));

        // Should mention relationships
        assert!(text.contains("KNOWS"));
        assert!(text.contains("WORKS_AT"));

        // Should mention the "Person" label
        assert!(text.contains("Person"));
    }

    #[test]
    fn test_linearize_structured() {
        let (_graph, subgraph) = build_linearize_graph();
        let text = linearize_subgraph(&subgraph, TextFormat::Structured);

        // Should have Node [...] headers
        assert!(text.contains("Node [Person: Alice]"));
        assert!(text.contains("Node [Person: Bob]"));
        assert!(text.contains("Node [Company: Acme]"));

        // Should have arrow notation for edges
        assert!(text.contains("-[KNOWS"));
        assert!(text.contains("]->"));
        assert!(text.contains("-[WORKS_AT]->"));
    }

    #[test]
    fn test_linearize_triples() {
        let (_graph, subgraph) = build_linearize_graph();
        let text = linearize_subgraph(&subgraph, TextFormat::Triples);

        // Should have triple format
        assert!(text.contains("(Alice, KNOWS, Bob)"));
        assert!(text.contains("(Bob, WORKS_AT, Acme)"));

        // Should have exactly 2 lines
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_linearize_json() {
        let (_graph, subgraph) = build_linearize_graph();
        let text = linearize_subgraph(&subgraph, TextFormat::Json);

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();

        // Should have nodes and edges arrays
        let nodes = parsed["nodes"].as_array().unwrap();
        let edges = parsed["edges"].as_array().unwrap();

        assert_eq!(nodes.len(), 3);
        assert_eq!(edges.len(), 2);

        // Verify node structure
        assert!(nodes.iter().any(|n| n["properties"]["name"] == "Alice"));
        assert!(nodes.iter().any(|n| n["properties"]["name"] == "Bob"));
        assert!(nodes.iter().any(|n| n["properties"]["name"] == "Acme"));

        // Verify edge structure
        assert!(edges.iter().any(|e| e["type"] == "KNOWS"));
        assert!(edges.iter().any(|e| e["type"] == "WORKS_AT"));
    }
}
