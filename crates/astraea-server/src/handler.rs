use std::collections::{HashMap, HashSet};
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
                            // astraeadb-issues.md #6: the TCP path emitted
                            // `distance`, the proto declared `score`, and
                            // the gRPC bridge silently defaulted to 0.0
                            // when it couldn't find `score`. Emit BOTH
                            // keys with the same value (lower = more
                            // similar) so every client sees real numbers,
                            // regardless of which name it expects.
                            let items: Vec<serde_json::Value> = results
                                .into_iter()
                                .map(|r| {
                                    serde_json::json!({
                                        "node_id": r.node_id.0,
                                        "distance": r.distance,
                                        "score": r.distance,
                                    })
                                })
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

            Request::NeighborsAt {
                id,
                direction,
                timestamp,
                edge_type,
            } => {
                let dir = match direction.as_str() {
                    "incoming" => Direction::Incoming,
                    "both" => Direction::Both,
                    _ => Direction::Outgoing,
                };

                match self.graph.neighbors_at(NodeId(id), dir, timestamp) {
                    Ok(neighbors) => {
                        let items: Vec<serde_json::Value> = if let Some(et) = edge_type {
                            // Post-filter by edge type (get the edge to check type)
                            neighbors
                                .into_iter()
                                .filter(|(eid, _)| {
                                    self.graph
                                        .get_edge(*eid)
                                        .ok()
                                        .flatten()
                                        .is_some_and(|e| e.edge_type == et)
                                })
                                .map(|(eid, nid)| {
                                    serde_json::json!({"edge_id": eid.0, "node_id": nid.0})
                                })
                                .collect()
                        } else {
                            neighbors
                                .into_iter()
                                .map(|(eid, nid)| {
                                    serde_json::json!({"edge_id": eid.0, "node_id": nid.0})
                                })
                                .collect()
                        };
                        Response::ok(serde_json::json!({"neighbors": items, "timestamp": timestamp}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::BfsAt {
                start,
                max_depth,
                timestamp,
            } => match self.graph.bfs_at(NodeId(start), max_depth, timestamp) {
                Ok(nodes) => {
                    let items: Vec<serde_json::Value> = nodes
                        .into_iter()
                        .map(|(nid, depth)| {
                            serde_json::json!({"node_id": nid.0, "depth": depth})
                        })
                        .collect();
                    Response::ok(serde_json::json!({"nodes": items, "timestamp": timestamp}))
                }
                Err(e) => Response::error(e.to_string()),
            },

            Request::ShortestPathAt {
                from,
                to,
                timestamp,
                weighted,
            } => {
                if weighted {
                    match self
                        .graph
                        .shortest_path_weighted_at(NodeId(from), NodeId(to), timestamp)
                    {
                        Ok(Some((path, cost))) => {
                            let node_ids: Vec<u64> =
                                path.nodes().into_iter().map(|n| n.0).collect();
                            Response::ok(serde_json::json!({
                                "path": node_ids, "cost": cost, "length": path.len(),
                                "timestamp": timestamp
                            }))
                        }
                        Ok(None) => {
                            Response::ok(serde_json::json!({"path": null, "timestamp": timestamp}))
                        }
                        Err(e) => Response::error(e.to_string()),
                    }
                } else {
                    match self
                        .graph
                        .shortest_path_at(NodeId(from), NodeId(to), timestamp)
                    {
                        Ok(Some(path)) => {
                            let node_ids: Vec<u64> =
                                path.nodes().into_iter().map(|n| n.0).collect();
                            Response::ok(serde_json::json!({
                                "path": node_ids, "length": path.len(),
                                "timestamp": timestamp
                            }))
                        }
                        Ok(None) => {
                            Response::ok(serde_json::json!({"path": null, "timestamp": timestamp}))
                        }
                        Err(e) => Response::error(e.to_string()),
                    }
                }
            }

            Request::Dfs { start, max_depth } => {
                match self.graph.dfs(NodeId(start), max_depth) {
                    Ok(nodes) => {
                        let ids: Vec<u64> = nodes.into_iter().map(|n| n.0).collect();
                        Response::ok(serde_json::json!({"nodes": ids}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::DfsAt {
                start,
                max_depth,
                timestamp,
            } => {
                // DFS at a point in time using neighbors_at iteratively.
                match dfs_at_impl(&*self.graph, NodeId(start), max_depth, timestamp) {
                    Ok(nodes) => {
                        let ids: Vec<u64> = nodes.into_iter().map(|n| n.0).collect();
                        Response::ok(serde_json::json!({"nodes": ids, "timestamp": timestamp}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::FindByLabel { label } => {
                match self.graph.find_by_label(&label) {
                    Ok(ids) => {
                        let node_ids: Vec<u64> = ids.into_iter().map(|n| n.0).collect();
                        Response::ok(serde_json::json!({"node_ids": node_ids}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            // astraeadb-issues.md #4. Bulk-delete every node with the
            // given label (and its edges). Previously clients had to
            // FindByLabel -> per-node DeleteNode, which round-tripped
            // once per match.
            Request::DeleteByLabel { label } => {
                match self.graph.find_by_label(&label) {
                    Ok(ids) => {
                        let total = ids.len();
                        let mut deleted = 0u64;
                        for id in ids {
                            match self.graph.delete_node(id) {
                                Ok(()) => deleted += 1,
                                Err(e) => {
                                    return Response::error(format!(
                                        "DeleteByLabel '{label}': deleted {deleted}/{total} before error at {id}: {e}"
                                    ));
                                }
                            }
                        }
                        Response::ok(serde_json::json!({ "deleted": deleted }))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            // astraeadb-issues.md #3. Find all edges whose edge_type matches
            // the given string.
            Request::FindEdgeByType { edge_type } => {
                match self.graph.find_edges_by_type(&edge_type) {
                    Ok(triples) => {
                        let edges: Vec<serde_json::Value> = triples
                            .into_iter()
                            .map(|(eid, src, tgt)| {
                                serde_json::json!({
                                    "edge_id": eid.0,
                                    "source": src.0,
                                    "target": tgt.0,
                                })
                            })
                            .collect();
                        Response::ok(serde_json::json!({ "edges": edges }))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::RunPageRank {
                nodes,
                damping,
                max_iterations,
                tolerance,
            } => {
                let node_ids = match resolve_node_ids(&*self.graph, nodes) {
                    Ok(ids) => ids,
                    Err(e) => return Response::error(e.to_string()),
                };

                let config = astraea_algorithms::pagerank::PageRankConfig {
                    damping,
                    max_iterations,
                    tolerance,
                };

                match astraea_algorithms::pagerank::pagerank(&*self.graph, &node_ids, &config) {
                    Ok(scores) => {
                        let map: HashMap<String, f64> =
                            scores.into_iter().map(|(k, v)| (k.0.to_string(), v)).collect();
                        Response::ok(serde_json::json!({"scores": map}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::RunLouvain { nodes } => {
                let node_ids = match resolve_node_ids(&*self.graph, nodes) {
                    Ok(ids) => ids,
                    Err(e) => return Response::error(e.to_string()),
                };

                match astraea_algorithms::community::louvain(&*self.graph, &node_ids) {
                    Ok(communities) => {
                        let map: HashMap<String, usize> =
                            communities.into_iter().map(|(k, v)| (k.0.to_string(), v)).collect();
                        let num_communities = map.values().collect::<HashSet<_>>().len();
                        Response::ok(serde_json::json!({
                            "communities": map,
                            "num_communities": num_communities,
                        }))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::RunConnectedComponents { nodes, strong } => {
                let node_ids = match resolve_node_ids(&*self.graph, nodes) {
                    Ok(ids) => ids,
                    Err(e) => return Response::error(e.to_string()),
                };

                let result = if strong {
                    astraea_algorithms::components::strongly_connected_components(
                        &*self.graph,
                        &node_ids,
                    )
                } else {
                    astraea_algorithms::components::connected_components(&*self.graph, &node_ids)
                };

                match result {
                    Ok(components) => {
                        let count = components.len();
                        let comps: Vec<Vec<u64>> = components
                            .into_iter()
                            .map(|c| c.into_iter().map(|n| n.0).collect())
                            .collect();
                        Response::ok(serde_json::json!({
                            "components": comps,
                            "count": count,
                        }))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::RunDegreeCentrality { nodes, direction } => {
                let node_ids = match resolve_node_ids(&*self.graph, nodes) {
                    Ok(ids) => ids,
                    Err(e) => return Response::error(e.to_string()),
                };

                let dir = parse_direction(&direction);

                match astraea_algorithms::centrality::degree_centrality(
                    &*self.graph,
                    &node_ids,
                    dir,
                ) {
                    Ok(scores) => {
                        let map: HashMap<String, f64> =
                            scores.into_iter().map(|(k, v)| (k.0.to_string(), v)).collect();
                        Response::ok(serde_json::json!({"scores": map}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::RunBetweennessCentrality { nodes } => {
                let node_ids = match resolve_node_ids(&*self.graph, nodes) {
                    Ok(ids) => ids,
                    Err(e) => return Response::error(e.to_string()),
                };

                match astraea_algorithms::centrality::betweenness_centrality(
                    &*self.graph,
                    &node_ids,
                ) {
                    Ok(scores) => {
                        let map: HashMap<String, f64> =
                            scores.into_iter().map(|(k, v)| (k.0.to_string(), v)).collect();
                        Response::ok(serde_json::json!({"scores": map}))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::GraphStats => {
                match collect_graph_stats(&*self.graph, self.vector_index.as_deref()) {
                    Ok(stats) => Response::ok(stats),
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::GetSubgraph {
                center,
                hops,
                max_nodes,
            } => {
                match astraea_rag::extract_subgraph(&*self.graph, NodeId(center), hops, max_nodes) {
                    Ok(subgraph) => {
                        let nodes: Vec<serde_json::Value> = subgraph
                            .nodes
                            .iter()
                            .map(|n| {
                                serde_json::json!({
                                    "id": n.id.0,
                                    "labels": n.labels,
                                    "properties": n.properties,
                                    "has_embedding": n.embedding.is_some(),
                                })
                            })
                            .collect();
                        let edges: Vec<serde_json::Value> = subgraph
                            .edges
                            .iter()
                            .map(|e| {
                                serde_json::json!({
                                    "id": e.id.0,
                                    "source": e.source.0,
                                    "target": e.target.0,
                                    "edge_type": e.edge_type,
                                    "properties": e.properties,
                                    "weight": e.weight,
                                    "valid_from": e.validity.valid_from,
                                    "valid_to": e.validity.valid_to,
                                })
                            })
                            .collect();
                        Response::ok(serde_json::json!({
                            "nodes": nodes,
                            "edges": edges,
                        }))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }

            Request::Ping => {
                // Expose the vector-index dimension and metric here (in
                // addition to GraphStats) so clients can check before
                // attempting an insert with the wrong-size embedding.
                // astraeadb-issues.md #7.
                let mut payload = serde_json::json!({
                    "pong": true,
                    "version": env!("CARGO_PKG_VERSION"),
                });
                if let Some(vi) = self.vector_index.as_deref() {
                    payload["vector_dim"] = serde_json::json!(vi.dimension());
                    payload["vector_metric"] = serde_json::json!(format!("{:?}", vi.metric()));
                }
                Response::ok(payload)
            }
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

/// Parse a direction string into a `Direction` enum value.
fn parse_direction(s: &str) -> Direction {
    match s {
        "incoming" => Direction::Incoming,
        "both" => Direction::Both,
        _ => Direction::Outgoing,
    }
}

/// Resolve node IDs for algorithm operations.
///
/// If `nodes` is `Some`, convert the provided IDs.
/// If `None`, scan the graph for all existing node IDs (by probing IDs 1..10_000).
fn resolve_node_ids(
    graph: &dyn GraphOps,
    nodes: Option<Vec<u64>>,
) -> astraea_core::error::Result<Vec<NodeId>> {
    if let Some(ids) = nodes {
        Ok(ids.into_iter().map(NodeId).collect())
    } else {
        // Scan for existing nodes. This is a simple approach that probes
        // sequential IDs. For large graphs, a dedicated scan method would be better.
        let mut found = Vec::new();
        for id in 1..10_000u64 {
            if graph.get_node(NodeId(id))?.is_some() {
                found.push(NodeId(id));
            }
        }
        Ok(found)
    }
}

/// Collect graph statistics by scanning node/edge ID space.
fn collect_graph_stats(
    graph: &dyn GraphOps,
    vector_index: Option<&dyn VectorIndex>,
) -> astraea_core::error::Result<serde_json::Value> {
    let mut total_nodes: u64 = 0;
    let mut total_edges: u64 = 0;
    let mut label_counts: HashMap<String, u64> = HashMap::new();
    let mut edge_type_counts: HashMap<String, u64> = HashMap::new();

    // Scan nodes
    for id in 1..10_000u64 {
        if let Some(node) = graph.get_node(NodeId(id))? {
            total_nodes += 1;
            for label in &node.labels {
                *label_counts.entry(label.clone()).or_insert(0) += 1;
            }
        }
    }

    // Scan edges
    for id in 1..10_000u64 {
        if let Some(edge) = graph.get_edge(EdgeId(id))? {
            total_edges += 1;
            *edge_type_counts.entry(edge.edge_type.clone()).or_insert(0) += 1;
        }
    }

    let mut stats = serde_json::json!({
        "total_nodes": total_nodes,
        "total_edges": total_edges,
        "labels": label_counts,
        "edge_types": edge_type_counts,
    });

    if let Some(vi) = vector_index {
        stats["vector_index"] = serde_json::json!({
            "dimension": vi.dimension(),
            "metric": format!("{:?}", vi.metric()),
            "size": vi.len(),
        });
    }

    Ok(stats)
}

/// DFS traversal at a specific point in time (temporal).
///
/// Uses `neighbors_at` to only follow edges valid at the given timestamp.
fn dfs_at_impl(
    graph: &dyn GraphOps,
    start: NodeId,
    max_depth: usize,
    timestamp: i64,
) -> astraea_core::error::Result<Vec<NodeId>> {
    let mut visited = HashSet::new();
    let mut result = Vec::new();
    let mut stack: Vec<(NodeId, usize)> = vec![(start, 0)];

    while let Some((node, depth)) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        result.push(node);

        if depth < max_depth {
            let neighbors = graph.neighbors_at(node, Direction::Outgoing, timestamp)?;
            for (_eid, nid) in neighbors {
                if !visited.contains(&nid) {
                    stack.push((nid, depth + 1));
                }
            }
        }
    }

    Ok(result)
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

    #[test]
    fn test_delete_by_label_removes_matches() {
        // astraeadb-issues.md #4.
        let (handler, _vi) = handler_with_vector_index(2);

        // Create three Widget nodes and one Gadget node.
        for _ in 0..3 {
            match handler.handle(Request::CreateNode {
                labels: vec!["Widget".into()],
                properties: serde_json::json!({}),
                embedding: None,
            }) {
                Response::Ok { .. } => {}
                Response::Error { message } => panic!("create failed: {message}"),
            }
        }
        match handler.handle(Request::CreateNode {
            labels: vec!["Gadget".into()],
            properties: serde_json::json!({}),
            embedding: None,
        }) {
            Response::Ok { .. } => {}
            Response::Error { message } => panic!("create failed: {message}"),
        }

        // Delete by Widget — expect 3 deleted.
        let resp = handler.handle(Request::DeleteByLabel {
            label: "Widget".into(),
        });
        match resp {
            Response::Ok { data } => {
                assert_eq!(data.get("deleted").unwrap().as_u64().unwrap(), 3);
            }
            Response::Error { message } => panic!("DeleteByLabel failed: {message}"),
        }

        // Widgets gone; Gadget still there.
        match handler.handle(Request::FindByLabel {
            label: "Widget".into(),
        }) {
            Response::Ok { data } => {
                let ids = data.get("node_ids").unwrap().as_array().unwrap();
                assert!(ids.is_empty(), "expected no Widgets, got {:?}", ids);
            }
            Response::Error { message } => panic!("FindByLabel failed: {message}"),
        }
        match handler.handle(Request::FindByLabel {
            label: "Gadget".into(),
        }) {
            Response::Ok { data } => {
                let ids = data.get("node_ids").unwrap().as_array().unwrap();
                assert_eq!(ids.len(), 1, "Gadget should still be present");
            }
            Response::Error { message } => panic!("FindByLabel failed: {message}"),
        }

        // DeleteByLabel of an absent label returns deleted=0 cleanly.
        match handler.handle(Request::DeleteByLabel {
            label: "Nonexistent".into(),
        }) {
            Response::Ok { data } => {
                assert_eq!(data.get("deleted").unwrap().as_u64().unwrap(), 0);
            }
            Response::Error { message } => panic!("unexpected error: {message}"),
        }
    }

    #[test]
    fn test_find_edges_by_type() {
        // astraeadb-issues.md #3.
        let (handler, _vi) = handler_with_vector_index(2);

        // Create two nodes to wire edges between.
        let mut node_ids = Vec::new();
        for _ in 0..2 {
            match handler.handle(Request::CreateNode {
                labels: vec![],
                properties: serde_json::json!({}),
                embedding: None,
            }) {
                Response::Ok { data } => {
                    node_ids.push(data.get("node_id").unwrap().as_u64().unwrap());
                }
                Response::Error { message } => panic!("create node failed: {message}"),
            }
        }
        let (src, tgt) = (node_ids[0], node_ids[1]);

        // Create 3 edges of type T1 and 2 of type T2.
        let mut t1_ids: Vec<u64> = Vec::new();
        for _ in 0..3 {
            match handler.handle(Request::CreateEdge {
                source: src,
                target: tgt,
                edge_type: "T1".into(),
                properties: serde_json::json!({}),
                weight: 1.0,
                valid_from: None,
                valid_to: None,
            }) {
                Response::Ok { data } => {
                    t1_ids.push(data.get("edge_id").unwrap().as_u64().unwrap());
                }
                Response::Error { message } => panic!("create edge T1 failed: {message}"),
            }
        }
        for _ in 0..2 {
            match handler.handle(Request::CreateEdge {
                source: src,
                target: tgt,
                edge_type: "T2".into(),
                properties: serde_json::json!({}),
                weight: 1.0,
                valid_from: None,
                valid_to: None,
            }) {
                Response::Ok { .. } => {}
                Response::Error { message } => panic!("create edge T2 failed: {message}"),
            }
        }

        // Happy path: FindEdgeByType("T1") must return exactly the 3 T1 edges
        // with correct source/target fields.
        let resp = handler.handle(Request::FindEdgeByType {
            edge_type: "T1".into(),
        });
        match resp {
            Response::Ok { data } => {
                let edges = data.get("edges").unwrap().as_array().unwrap();
                assert_eq!(edges.len(), 3, "expected 3 T1 edges, got {}", edges.len());
                // All returned edges must have the correct source/target.
                for e in edges {
                    assert_eq!(
                        e.get("source").unwrap().as_u64().unwrap(),
                        src,
                        "wrong source"
                    );
                    assert_eq!(
                        e.get("target").unwrap().as_u64().unwrap(),
                        tgt,
                        "wrong target"
                    );
                }
                // The edge_ids returned must be exactly the three we created.
                let mut returned_ids: Vec<u64> = edges
                    .iter()
                    .map(|e| e.get("edge_id").unwrap().as_u64().unwrap())
                    .collect();
                returned_ids.sort_unstable();
                let mut expected = t1_ids.clone();
                expected.sort_unstable();
                assert_eq!(returned_ids, expected, "T1 edge_ids mismatch");
            }
            Response::Error { message } => panic!("FindEdgeByType T1 failed: {message}"),
        }

        // Empty result: a nonexistent type returns an empty list, not an error.
        match handler.handle(Request::FindEdgeByType {
            edge_type: "nonexistent".into(),
        }) {
            Response::Ok { data } => {
                let edges = data.get("edges").unwrap().as_array().unwrap();
                assert!(
                    edges.is_empty(),
                    "expected empty list for unknown type, got {:?}",
                    edges
                );
            }
            Response::Error { message } => panic!("FindEdgeByType nonexistent returned error: {message}"),
        }
    }
}
