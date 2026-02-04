use std::sync::Arc;

use astraea_core::traits::GraphOps;
use astraea_core::types::*;

use crate::protocol::{Request, Response};

/// Handles incoming requests by dispatching to the graph engine.
pub struct RequestHandler {
    graph: Arc<dyn GraphOps>,
}

impl RequestHandler {
    pub fn new(graph: Arc<dyn GraphOps>) -> Self {
        Self { graph }
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

            Request::VectorSearch { .. } => {
                Response::error("vector search not yet integrated with server")
            }

            Request::Query { .. } => Response::error("GQL query execution not yet integrated"),

            Request::Ping => Response::ok(serde_json::json!({
                "pong": true,
                "version": env!("CARGO_PKG_VERSION"),
            })),
        }
    }
}
