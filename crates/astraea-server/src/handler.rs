use std::sync::Arc;

use astraea_core::traits::{GraphOps, VectorIndex};
use astraea_core::types::*;
use astraea_query::executor::Executor;

use crate::protocol::{Request, Response};

/// Handles incoming requests by dispatching to the graph engine.
pub struct RequestHandler {
    graph: Arc<dyn GraphOps>,
    executor: Executor,
    vector_index: Option<Arc<dyn VectorIndex>>,
}

impl RequestHandler {
    pub fn new(graph: Arc<dyn GraphOps>, vector_index: Option<Arc<dyn VectorIndex>>) -> Self {
        let executor = Executor::new(Arc::clone(&graph));
        Self { graph, executor, vector_index }
    }

    /// Process a single request and return a response.
    pub fn handle(&self, request: Request) -> Response {
        match request {
            Request::CreateNode {
                labels,
                properties,
                embedding,
            } => match self.graph.create_node(labels, properties, embedding) {
                Ok(id) => Response::ok(serde_json::json!({"node_id": id.0})),
                Err(e) => Response::error(e.to_string()),
            },

            Request::CreateEdge {
                source,
                target,
                edge_type,
                properties,
                weight,
                valid_from,
                valid_to,
            } => {
                match self.graph.create_edge(
                    NodeId(source),
                    NodeId(target),
                    edge_type,
                    properties,
                    weight,
                    valid_from,
                    valid_to,
                ) {
                    Ok(id) => Response::ok(serde_json::json!({"edge_id": id.0})),
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::GetNode { id } => match self.graph.get_node(NodeId(id)) {
                Ok(Some(node)) => Response::ok(serde_json::json!({
                    "id": node.id.0,
                    "labels": node.labels,
                    "properties": node.properties,
                    "has_embedding": node.embedding.is_some(),
                })),
                Ok(None) => Response::error(format!("node {id} not found")),
                Err(e) => Response::error(e.to_string()),
            },

            Request::GetEdge { id } => match self.graph.get_edge(EdgeId(id)) {
                Ok(Some(edge)) => Response::ok(serde_json::json!({
                    "id": edge.id.0,
                    "source": edge.source.0,
                    "target": edge.target.0,
                    "edge_type": edge.edge_type,
                    "properties": edge.properties,
                    "weight": edge.weight,
                    "valid_from": edge.validity.valid_from,
                    "valid_to": edge.validity.valid_to,
                })),
                Ok(None) => Response::error(format!("edge {id} not found")),
                Err(e) => Response::error(e.to_string()),
            },

            Request::UpdateNode { id, properties } => {
                match self.graph.update_node(NodeId(id), properties) {
                    Ok(()) => Response::ok(serde_json::json!({"updated": true})),
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::UpdateEdge { id, properties } => {
                match self.graph.update_edge(EdgeId(id), properties) {
                    Ok(()) => Response::ok(serde_json::json!({"updated": true})),
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::DeleteNode { id } => match self.graph.delete_node(NodeId(id)) {
                Ok(()) => Response::ok(serde_json::json!({"deleted": true})),
                Err(e) => Response::error(e.to_string()),
            },

            Request::DeleteEdge { id } => match self.graph.delete_edge(EdgeId(id)) {
                Ok(()) => Response::ok(serde_json::json!({"deleted": true})),
                Err(e) => Response::error(e.to_string()),
            },

            Request::Neighbors {
                id,
                direction,
                edge_type,
            } => {
                let dir = match direction.as_str() {
                    "incoming" => Direction::Incoming,
                    "both" => Direction::Both,
                    _ => Direction::Outgoing,
                };

                let result = if let Some(et) = edge_type {
                    self.graph.neighbors_filtered(NodeId(id), dir, &et)
                } else {
                    self.graph.neighbors(NodeId(id), dir)
                };

                match result {
                    Ok(neighbors) => {
                        let items: Vec<serde_json::Value> = neighbors
                            .into_iter()
                            .map(|(eid, nid)| {
                                serde_json::json!({"edge_id": eid.0, "node_id": nid.0})
                            })
                            .collect();
                        Response::ok(serde_json::json!({"neighbors": items}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::Bfs { start, max_depth } => {
                match self.graph.bfs(NodeId(start), max_depth) {
                    Ok(nodes) => {
                        let items: Vec<serde_json::Value> = nodes
                            .into_iter()
                            .map(|(nid, depth)| {
                                serde_json::json!({"node_id": nid.0, "depth": depth})
                            })
                            .collect();
                        Response::ok(serde_json::json!({"nodes": items}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::ShortestPath { from, to, weighted } => {
                if weighted {
                    match self
                        .graph
                        .shortest_path_weighted(NodeId(from), NodeId(to))
                    {
                        Ok(Some((path, cost))) => {
                            let node_ids: Vec<u64> = path.nodes().into_iter().map(|n| n.0).collect();
                            Response::ok(
                                serde_json::json!({"path": node_ids, "cost": cost, "length": path.len()}),
                            )
                        }
                        Ok(None) => Response::ok(serde_json::json!({"path": null})),
                        Err(e) => Response::error(e.to_string()),
                    }
                } else {
                    match self.graph.shortest_path(NodeId(from), NodeId(to)) {
                        Ok(Some(path)) => {
                            let node_ids: Vec<u64> = path.nodes().into_iter().map(|n| n.0).collect();
                            Response::ok(
                                serde_json::json!({"path": node_ids, "length": path.len()}),
                            )
                        }
                        Ok(None) => Response::ok(serde_json::json!({"path": null})),
                        Err(e) => Response::error(e.to_string()),
                    }
                }
            }

            Request::HybridSearch {
                anchor,
                query,
                max_hops,
                k,
                alpha,
            } => {
                match self
                    .graph
                    .hybrid_search(NodeId(anchor), &query, max_hops, k, alpha)
                {
                    Ok(results) => {
                        let items: Vec<serde_json::Value> = results
                            .into_iter()
                            .map(|(nid, score)| {
                                serde_json::json!({"node_id": nid.0, "score": score})
                            })
                            .collect();
                        Response::ok(serde_json::json!({"results": items}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::SemanticNeighbors {
                id,
                concept,
                direction,
                k,
            } => {
                let dir = match direction.as_str() {
                    "incoming" => Direction::Incoming,
                    "both" => Direction::Both,
                    _ => Direction::Outgoing,
                };
                match self
                    .graph
                    .semantic_neighbors(NodeId(id), &concept, dir, k)
                {
                    Ok(results) => {
                        let items: Vec<serde_json::Value> = results
                            .into_iter()
                            .map(|(nid, dist)| {
                                serde_json::json!({"node_id": nid.0, "distance": dist})
                            })
                            .collect();
                        Response::ok(serde_json::json!({"results": items}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::SemanticWalk {
                start,
                concept,
                max_hops,
            } => {
                match self
                    .graph
                    .semantic_walk(NodeId(start), &concept, max_hops)
                {
                    Ok(path) => {
                        let items: Vec<serde_json::Value> = path
                            .into_iter()
                            .map(|(nid, dist)| {
                                serde_json::json!({"node_id": nid.0, "distance": dist})
                            })
                            .collect();
                        Response::ok(serde_json::json!({"path": items}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::VectorSearch { query, k } => {
                match &self.vector_index {
                    Some(vi) => match vi.search(&query, k) {
                        Ok(results) => {
                            let items: Vec<serde_json::Value> = results
                                .into_iter()
                                .map(|r| serde_json::json!({"node_id": r.node_id.0, "distance": r.distance}))
                                .collect();
                            Response::ok(serde_json::json!({"results": items}))
                        }
                        Err(e) => Response::error(e.to_string()),
                    },
                    None => Response::error("vector index not configured"),
                }
            }

            Request::Query { gql } => {
                // Parse the GQL string into an AST.
                let stmt = match astraea_query::parse(&gql) {
                    Ok(s) => s,
                    Err(e) => return Response::error(format!("parse error: {e}")),
                };

                // Execute the AST against the graph.
                match self.executor.execute(stmt) {
                    Ok(result) => Response::ok(serde_json::json!({
                        "columns": result.columns,
                        "rows": result.rows,
                        "stats": {
                            "nodes_created": result.stats.nodes_created,
                            "edges_created": result.stats.edges_created,
                            "nodes_deleted": result.stats.nodes_deleted,
                            "edges_deleted": result.stats.edges_deleted,
                        },
                    })),
                    Err(e) => Response::error(format!("execution error: {e}")),
                }
            }

            Request::ExtractSubgraph {
                center,
                hops,
                max_nodes,
                format,
            } => {
                let text_format = parse_text_format(&format);
                match astraea_rag::extract_subgraph(
                    &*self.graph,
                    NodeId(center),
                    hops,
                    max_nodes,
                ) {
                    Ok(subgraph) => {
                        let text = astraea_rag::linearize_subgraph(&subgraph, text_format);
                        let tokens = astraea_rag::estimate_tokens(&text);
                        Response::ok(serde_json::json!({
                            "text": text,
                            "nodes_count": subgraph.nodes.len(),
                            "edges_count": subgraph.edges.len(),
                            "estimated_tokens": tokens,
                        }))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::GraphRag {
                question,
                question_embedding,
                anchor,
                hops,
                max_nodes,
                format,
            } => {
                let text_format = parse_text_format(&format);

                let center = if let Some(a) = anchor {
                    NodeId(a)
                } else if let Some(emb) = &question_embedding {
                    match &self.vector_index {
                        Some(vi) => match vi.search(emb, 1) {
                            Ok(results) if !results.is_empty() => results[0].node_id,
                            Ok(_) => return Response::error("no matching nodes found"),
                            Err(e) => return Response::error(e.to_string()),
                        },
                        None => {
                            return Response::error(
                                "vector index not configured and no anchor provided",
                            )
                        }
                    }
                } else {
                    return Response::error(
                        "either anchor or question_embedding must be provided",
                    );
                };

                match astraea_rag::extract_subgraph(&*self.graph, center, hops, max_nodes) {
                    Ok(subgraph) => {
                        let context =
                            astraea_rag::linearize_subgraph(&subgraph, text_format);
                        let tokens = astraea_rag::estimate_tokens(&context);
                        Response::ok(serde_json::json!({
                            "anchor_node_id": center.0,
                            "context": context,
                            "question": question,
                            "nodes_in_context": subgraph.nodes.len(),
                            "edges_in_context": subgraph.edges.len(),
                            "estimated_tokens": tokens,
                            "note": "LLM completion requires server-side provider configuration. Use the context with your own LLM."
                        }))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::Ping => Response::ok(serde_json::json!({
                "pong": true,
                "version": env!("CARGO_PKG_VERSION"),
            })),
        }
    }
}

/// Parse a text format string into a `TextFormat` enum value.
fn parse_text_format(s: &str) -> astraea_rag::TextFormat {
    match s.to_lowercase().as_str() {
        "prose" => astraea_rag::TextFormat::Prose,
        "triples" => astraea_rag::TextFormat::Triples,
        "json" => astraea_rag::TextFormat::Json,
        _ => astraea_rag::TextFormat::Structured,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;
    use astraea_vector::HnswVectorIndex;

    /// Helper: create a handler with a vector index.
    fn handler_with_vector_index(dim: usize) -> (RequestHandler, Arc<dyn VectorIndex>) {
        let storage = InMemoryStorage::new();
        let vector_index: Arc<dyn VectorIndex> =
            Arc::new(HnswVectorIndex::new(dim, DistanceMetric::Cosine));
        let graph = Graph::with_vector_index(Box::new(storage), Arc::clone(&vector_index));
        let handler = RequestHandler::new(Arc::new(graph), Some(Arc::clone(&vector_index)));
        (handler, vector_index)
    }

    /// Helper: create a handler without a vector index.
    fn handler_without_vector_index() -> RequestHandler {
        let storage = InMemoryStorage::new();
        let graph = Graph::new(Box::new(storage));
        RequestHandler::new(Arc::new(graph), None)
    }

    #[test]
    fn test_vector_search_basic() {
        let (handler, vi) = handler_with_vector_index(3);

        // Insert some vectors directly into the index for testing.
        vi.insert(NodeId(100), &[1.0, 0.0, 0.0]).unwrap();
        vi.insert(NodeId(200), &[0.0, 1.0, 0.0]).unwrap();
        vi.insert(NodeId(300), &[0.0, 0.0, 1.0]).unwrap();

        let resp = handler.handle(Request::VectorSearch {
            query: vec![1.0, 0.0, 0.0],
            k: 2,
        });

        match resp {
            Response::Ok { data } => {
                let results = data.get("results").unwrap().as_array().unwrap();
                assert_eq!(results.len(), 2);
                // The closest should be node 100 (exact match).
                assert_eq!(results[0].get("node_id").unwrap().as_u64().unwrap(), 100);
            }
            Response::Error { message } => panic!("expected Ok, got Error: {}", message),
        }
    }

    #[test]
    fn test_vector_search_no_index() {
        let handler = handler_without_vector_index();

        let resp = handler.handle(Request::VectorSearch {
            query: vec![1.0, 0.0, 0.0],
            k: 5,
        });

        match resp {
            Response::Error { message } => {
                assert_eq!(message, "vector index not configured");
            }
            Response::Ok { .. } => panic!("expected Error, got Ok"),
        }
    }

    #[test]
    fn test_auto_index_on_create_node() {
        let (handler, _vi) = handler_with_vector_index(3);

        // Create a node with an embedding via the handler.
        let create_resp = handler.handle(Request::CreateNode {
            labels: vec!["TestNode".into()],
            properties: serde_json::json!({"name": "alpha"}),
            embedding: Some(vec![0.5, 0.5, 0.0]),
        });
        match &create_resp {
            Response::Ok { data } => {
                assert!(data.get("node_id").is_some());
            }
            Response::Error { message } => panic!("create failed: {}", message),
        }

        // Create a second node with a different embedding.
        handler.handle(Request::CreateNode {
            labels: vec!["TestNode".into()],
            properties: serde_json::json!({"name": "beta"}),
            embedding: Some(vec![0.0, 0.0, 1.0]),
        });

        // Now search for the embedding that matches the first node.
        let search_resp = handler.handle(Request::VectorSearch {
            query: vec![0.5, 0.5, 0.0],
            k: 1,
        });

        match search_resp {
            Response::Ok { data } => {
                let results = data.get("results").unwrap().as_array().unwrap();
                assert_eq!(results.len(), 1);
                // The closest should be node 1 (the first created node).
                assert_eq!(results[0].get("node_id").unwrap().as_u64().unwrap(), 1);
            }
            Response::Error { message } => panic!("search failed: {}", message),
        }
    }

    /// Helper: create a handler with nodes and edges for semantic tests.
    /// Returns (handler, n1_id, n2_id, n3_id).
    fn handler_with_semantic_graph() -> RequestHandler {
        let storage = InMemoryStorage::new();
        let vector_index: Arc<dyn VectorIndex> =
            Arc::new(HnswVectorIndex::new(3, DistanceMetric::Euclidean));
        let graph = Graph::with_vector_index(Box::new(storage), Arc::clone(&vector_index));
        let handler = RequestHandler::new(Arc::new(graph), Some(Arc::clone(&vector_index)));

        // Create nodes with embeddings.
        handler.handle(Request::CreateNode {
            labels: vec!["Thing".into()],
            properties: serde_json::json!({"name": "A"}),
            embedding: Some(vec![1.0, 0.0, 0.0]),
        }); // id=1
        handler.handle(Request::CreateNode {
            labels: vec!["Thing".into()],
            properties: serde_json::json!({"name": "closeA"}),
            embedding: Some(vec![0.9, 0.1, 0.0]),
        }); // id=2
        handler.handle(Request::CreateNode {
            labels: vec!["Thing".into()],
            properties: serde_json::json!({"name": "C"}),
            embedding: Some(vec![0.0, 0.0, 1.0]),
        }); // id=3

        // Edges: 1->2, 2->3
        handler.handle(Request::CreateEdge {
            source: 1,
            target: 2,
            edge_type: "LINK".into(),
            properties: serde_json::json!({}),
            weight: 1.0,
            valid_from: None,
            valid_to: None,
        });
        handler.handle(Request::CreateEdge {
            source: 2,
            target: 3,
            edge_type: "LINK".into(),
            properties: serde_json::json!({}),
            weight: 1.0,
            valid_from: None,
            valid_to: None,
        });

        handler
    }

    #[test]
    fn test_hybrid_search_handler() {
        let handler = handler_with_semantic_graph();

        let resp = handler.handle(Request::HybridSearch {
            anchor: 1,
            query: vec![1.0, 0.0, 0.0],
            max_hops: 2,
            k: 10,
            alpha: 0.5,
        });

        match resp {
            Response::Ok { data } => {
                let results = data.get("results").unwrap().as_array().unwrap();
                assert!(!results.is_empty());
                // Node 2 (closeA) should be the best match (close in both graph and vector).
                assert_eq!(results[0].get("node_id").unwrap().as_u64().unwrap(), 2);
                // Each result should have a "score" field.
                assert!(results[0].get("score").is_some());
            }
            Response::Error { message } => panic!("expected Ok, got Error: {message}"),
        }
    }

    #[test]
    fn test_semantic_neighbors_handler() {
        let handler = handler_with_semantic_graph();

        let resp = handler.handle(Request::SemanticNeighbors {
            id: 1,
            concept: vec![1.0, 0.0, 0.0],
            direction: "outgoing".into(),
            k: 10,
        });

        match resp {
            Response::Ok { data } => {
                let results = data.get("results").unwrap().as_array().unwrap();
                assert_eq!(results.len(), 1); // only neighbor of node 1 is node 2
                assert_eq!(results[0].get("node_id").unwrap().as_u64().unwrap(), 2);
                assert!(results[0].get("distance").is_some());
            }
            Response::Error { message } => panic!("expected Ok, got Error: {message}"),
        }
    }

    #[test]
    fn test_semantic_walk_handler() {
        let handler = handler_with_semantic_graph();

        let resp = handler.handle(Request::SemanticWalk {
            start: 1,
            concept: vec![0.0, 0.0, 1.0],
            max_hops: 5,
        });

        match resp {
            Response::Ok { data } => {
                let path = data.get("path").unwrap().as_array().unwrap();
                // Path should include at least start (1) and end (3).
                assert!(path.len() >= 2);
                assert_eq!(path[0].get("node_id").unwrap().as_u64().unwrap(), 1);
                // Last node should be node 3 (closest to concept [0,0,1]).
                assert_eq!(
                    path.last().unwrap().get("node_id").unwrap().as_u64().unwrap(),
                    3
                );
                assert!(path.last().unwrap().get("distance").is_some());
            }
            Response::Error { message } => panic!("expected Ok, got Error: {message}"),
        }
    }

    // ---- GraphRAG handler tests ----

    /// Helper: create a handler with a graph suitable for GraphRAG tests.
    /// Creates: Alice -[KNOWS]-> Bob -[WORKS_AT]-> Acme
    fn handler_with_rag_graph() -> RequestHandler {
        let storage = InMemoryStorage::new();
        let vector_index: Arc<dyn VectorIndex> =
            Arc::new(HnswVectorIndex::new(3, DistanceMetric::Euclidean));
        let graph = Graph::with_vector_index(Box::new(storage), Arc::clone(&vector_index));
        let handler = RequestHandler::new(Arc::new(graph), Some(Arc::clone(&vector_index)));

        // Create nodes with embeddings.
        handler.handle(Request::CreateNode {
            labels: vec!["Person".into()],
            properties: serde_json::json!({"name": "Alice"}),
            embedding: Some(vec![1.0, 0.0, 0.0]),
        }); // id=1
        handler.handle(Request::CreateNode {
            labels: vec!["Person".into()],
            properties: serde_json::json!({"name": "Bob"}),
            embedding: Some(vec![0.0, 1.0, 0.0]),
        }); // id=2
        handler.handle(Request::CreateNode {
            labels: vec!["Company".into()],
            properties: serde_json::json!({"name": "Acme"}),
            embedding: Some(vec![0.0, 0.0, 1.0]),
        }); // id=3

        handler.handle(Request::CreateEdge {
            source: 1,
            target: 2,
            edge_type: "KNOWS".into(),
            properties: serde_json::json!({"since": 2020}),
            weight: 1.0,
            valid_from: None,
            valid_to: None,
        });
        handler.handle(Request::CreateEdge {
            source: 2,
            target: 3,
            edge_type: "WORKS_AT".into(),
            properties: serde_json::json!({}),
            weight: 1.0,
            valid_from: None,
            valid_to: None,
        });

        handler
    }

    #[test]
    fn test_extract_subgraph_handler() {
        let handler = handler_with_rag_graph();

        let resp = handler.handle(Request::ExtractSubgraph {
            center: 1,
            hops: 2,
            max_nodes: 50,
            format: "structured".into(),
        });

        match resp {
            Response::Ok { data } => {
                assert!(data.get("text").unwrap().as_str().unwrap().contains("Alice"));
                let nodes_count = data.get("nodes_count").unwrap().as_u64().unwrap();
                assert!(nodes_count >= 2, "expected at least 2 nodes, got {nodes_count}");
                let edges_count = data.get("edges_count").unwrap().as_u64().unwrap();
                assert!(edges_count >= 1, "expected at least 1 edge, got {edges_count}");
                assert!(data.get("estimated_tokens").unwrap().as_u64().unwrap() > 0);
            }
            Response::Error { message } => panic!("expected Ok, got Error: {message}"),
        }
    }

    #[test]
    fn test_graph_rag_handler_with_anchor() {
        let handler = handler_with_rag_graph();

        let resp = handler.handle(Request::GraphRag {
            question: "Who does Alice know?".into(),
            question_embedding: None,
            anchor: Some(1),
            hops: 2,
            max_nodes: 50,
            format: "structured".into(),
        });

        match resp {
            Response::Ok { data } => {
                assert_eq!(data.get("anchor_node_id").unwrap().as_u64().unwrap(), 1);
                let context = data.get("context").unwrap().as_str().unwrap();
                assert!(context.contains("Alice"));
                assert_eq!(
                    data.get("question").unwrap().as_str().unwrap(),
                    "Who does Alice know?"
                );
                assert!(data.get("nodes_in_context").unwrap().as_u64().unwrap() > 0);
                assert!(data.get("edges_in_context").unwrap().as_u64().unwrap() > 0);
                assert!(data.get("estimated_tokens").unwrap().as_u64().unwrap() > 0);
                assert!(data.get("note").is_some());
            }
            Response::Error { message } => panic!("expected Ok, got Error: {message}"),
        }
    }

    #[test]
    fn test_graph_rag_handler_with_embedding() {
        let handler = handler_with_rag_graph();

        // Query with embedding close to Alice [1,0,0]
        let resp = handler.handle(Request::GraphRag {
            question: "Tell me about Alice".into(),
            question_embedding: Some(vec![1.0, 0.0, 0.0]),
            anchor: None,
            hops: 2,
            max_nodes: 50,
            format: "structured".into(),
        });

        match resp {
            Response::Ok { data } => {
                // Should anchor on node 1 (Alice, closest to [1,0,0]).
                assert_eq!(data.get("anchor_node_id").unwrap().as_u64().unwrap(), 1);
                let context = data.get("context").unwrap().as_str().unwrap();
                assert!(context.contains("Alice"));
            }
            Response::Error { message } => panic!("expected Ok, got Error: {message}"),
        }
    }

    #[test]
    fn test_graph_rag_handler_no_anchor_no_embedding() {
        let handler = handler_with_rag_graph();

        let resp = handler.handle(Request::GraphRag {
            question: "Some question".into(),
            question_embedding: None,
            anchor: None,
            hops: 2,
            max_nodes: 50,
            format: "structured".into(),
        });

        match resp {
            Response::Error { message } => {
                assert_eq!(message, "either anchor or question_embedding must be provided");
            }
            Response::Ok { .. } => panic!("expected Error, got Ok"),
        }
    }

    #[test]
    fn test_auto_remove_on_delete_node() {
        let (handler, _vi) = handler_with_vector_index(3);

        // Create a node with an embedding.
        let create_resp = handler.handle(Request::CreateNode {
            labels: vec!["Temp".into()],
            properties: serde_json::json!({}),
            embedding: Some(vec![1.0, 0.0, 0.0]),
        });
        let node_id = match &create_resp {
            Response::Ok { data } => data.get("node_id").unwrap().as_u64().unwrap(),
            Response::Error { message } => panic!("create failed: {}", message),
        };

        // Verify the node is searchable.
        let search_resp = handler.handle(Request::VectorSearch {
            query: vec![1.0, 0.0, 0.0],
            k: 1,
        });
        match &search_resp {
            Response::Ok { data } => {
                let results = data.get("results").unwrap().as_array().unwrap();
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].get("node_id").unwrap().as_u64().unwrap(), node_id);
            }
            Response::Error { message } => panic!("search failed before delete: {}", message),
        }

        // Delete the node.
        let del_resp = handler.handle(Request::DeleteNode { id: node_id });
        match &del_resp {
            Response::Ok { .. } => {}
            Response::Error { message } => panic!("delete failed: {}", message),
        }

        // Verify the node is no longer in vector search results.
        let search_resp2 = handler.handle(Request::VectorSearch {
            query: vec![1.0, 0.0, 0.0],
            k: 10,
        });
        match search_resp2 {
            Response::Ok { data } => {
                let results = data.get("results").unwrap().as_array().unwrap();
                // Should be empty -- the only node was deleted.
                assert!(
                    results.is_empty(),
                    "expected no results after delete, got: {:?}",
                    results
                );
            }
            Response::Error { message } => panic!("search failed after delete: {}", message),
        }
    }
}
