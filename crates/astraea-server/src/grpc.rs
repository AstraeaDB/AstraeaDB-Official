//! gRPC transport layer for AstraeaDB.
//!
//! This module provides a thin adapter that converts gRPC requests into the
//! existing [`Request`] enum, delegates to [`RequestHandler::handle`], and
//! converts the [`Response`] back into gRPC response types.
//!
//! It does **not** duplicate any business logic -- all processing goes through
//! the same handler that the TCP server uses.

use std::sync::Arc;

use tonic::{self, Status};
use tracing::info;

use crate::handler::RequestHandler;
use crate::protocol::{Request, Response};

// Pull in the generated protobuf / gRPC types.
pub mod proto {
    tonic::include_proto!("astraea");
}

use proto::astraea_service_server::{AstraeaService, AstraeaServiceServer};
use proto::*;

// ---------------------------------------------------------------------------
// Service implementation
// ---------------------------------------------------------------------------

/// The gRPC service implementation. Holds a shared reference to the same
/// [`RequestHandler`] that the TCP server uses.
pub struct AstraeaGrpcService {
    handler: Arc<RequestHandler>,
}

impl AstraeaGrpcService {
    pub fn new(handler: Arc<RequestHandler>) -> Self {
        Self { handler }
    }

    /// Build a `tonic` service that can be added to a [`tonic::transport::Server`].
    pub fn into_service(self) -> AstraeaServiceServer<Self> {
        AstraeaServiceServer::new(self)
    }
}

// ---------------------------------------------------------------------------
// Helper: extract data / error from Response
// ---------------------------------------------------------------------------

/// Convert our internal [`Response`] to `(success, result_json, error)`.
fn response_to_parts(resp: Response) -> (bool, String, String) {
    match resp {
        Response::Ok { data } => {
            let json = serde_json::to_string(&data).unwrap_or_default();
            (true, json, String::new())
        }
        Response::Error { message } => (false, String::new(), message),
    }
}

// ---------------------------------------------------------------------------
// Trait implementation
// ---------------------------------------------------------------------------

#[tonic::async_trait]
impl AstraeaService for AstraeaGrpcService {
    // -- Node CRUD ---------------------------------------------------------

    async fn create_node(
        &self,
        request: tonic::Request<CreateNodeRequest>,
    ) -> Result<tonic::Response<MutationResponse>, Status> {
        let req = request.into_inner();

        let properties: serde_json::Value = if req.properties_json.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&req.properties_json)
                .map_err(|e| Status::invalid_argument(format!("invalid properties JSON: {e}")))?
        };

        let embedding = if req.embedding.is_empty() {
            None
        } else {
            Some(req.embedding)
        };

        let internal = Request::CreateNode {
            labels: req.labels,
            properties,
            embedding,
        };

        let (success, result_json, error) = response_to_parts(self.handler.handle(internal));
        Ok(tonic::Response::new(MutationResponse {
            success,
            result_json,
            error,
        }))
    }

    async fn get_node(
        &self,
        request: tonic::Request<GetNodeRequest>,
    ) -> Result<tonic::Response<GetNodeResponse>, Status> {
        let req = request.into_inner();
        let internal = Request::GetNode { id: req.id };
        let resp = self.handler.handle(internal);

        match resp {
            Response::Ok { data } => {
                let id = data.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                let labels: Vec<String> = data
                    .get("labels")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let properties_json = data
                    .get("properties")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "{}".into());
                let has_embedding = data
                    .get("has_embedding")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                Ok(tonic::Response::new(GetNodeResponse {
                    found: true,
                    id,
                    labels,
                    properties_json,
                    has_embedding,
                    error: String::new(),
                }))
            }
            Response::Error { message } => Ok(tonic::Response::new(GetNodeResponse {
                found: false,
                id: 0,
                labels: vec![],
                properties_json: String::new(),
                has_embedding: false,
                error: message,
            })),
        }
    }

    async fn update_node(
        &self,
        request: tonic::Request<UpdateNodeRequest>,
    ) -> Result<tonic::Response<MutationResponse>, Status> {
        let req = request.into_inner();

        let properties: serde_json::Value = if req.properties_json.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&req.properties_json)
                .map_err(|e| Status::invalid_argument(format!("invalid properties JSON: {e}")))?
        };

        let internal = Request::UpdateNode {
            id: req.id,
            properties,
        };
        let (success, result_json, error) = response_to_parts(self.handler.handle(internal));
        Ok(tonic::Response::new(MutationResponse {
            success,
            result_json,
            error,
        }))
    }

    async fn delete_node(
        &self,
        request: tonic::Request<DeleteNodeRequest>,
    ) -> Result<tonic::Response<MutationResponse>, Status> {
        let req = request.into_inner();
        let internal = Request::DeleteNode { id: req.id };
        let (success, result_json, error) = response_to_parts(self.handler.handle(internal));
        Ok(tonic::Response::new(MutationResponse {
            success,
            result_json,
            error,
        }))
    }

    // -- Edge CRUD ---------------------------------------------------------

    async fn create_edge(
        &self,
        request: tonic::Request<CreateEdgeRequest>,
    ) -> Result<tonic::Response<MutationResponse>, Status> {
        let req = request.into_inner();

        let properties: serde_json::Value = if req.properties_json.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&req.properties_json)
                .map_err(|e| Status::invalid_argument(format!("invalid properties JSON: {e}")))?
        };

        let weight = if req.weight == 0.0 { 1.0 } else { req.weight };

        let internal = Request::CreateEdge {
            source: req.source,
            target: req.target,
            edge_type: req.edge_type,
            properties,
            weight,
            valid_from: req.valid_from,
            valid_to: req.valid_to,
        };

        let (success, result_json, error) = response_to_parts(self.handler.handle(internal));
        Ok(tonic::Response::new(MutationResponse {
            success,
            result_json,
            error,
        }))
    }

    async fn get_edge(
        &self,
        request: tonic::Request<GetEdgeRequest>,
    ) -> Result<tonic::Response<GetEdgeResponse>, Status> {
        let req = request.into_inner();
        let internal = Request::GetEdge { id: req.id };
        let resp = self.handler.handle(internal);

        match resp {
            Response::Ok { data } => {
                let id = data.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                let source = data.get("source").and_then(|v| v.as_u64()).unwrap_or(0);
                let target = data.get("target").and_then(|v| v.as_u64()).unwrap_or(0);
                let edge_type = data
                    .get("edge_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let properties_json = data
                    .get("properties")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "{}".into());
                let weight = data.get("weight").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let valid_from = data.get("valid_from").and_then(|v| v.as_i64());
                let valid_to = data.get("valid_to").and_then(|v| v.as_i64());

                Ok(tonic::Response::new(GetEdgeResponse {
                    found: true,
                    id,
                    source,
                    target,
                    edge_type,
                    properties_json,
                    weight,
                    valid_from,
                    valid_to,
                    error: String::new(),
                }))
            }
            Response::Error { message } => Ok(tonic::Response::new(GetEdgeResponse {
                found: false,
                id: 0,
                source: 0,
                target: 0,
                edge_type: String::new(),
                properties_json: String::new(),
                weight: 0.0,
                valid_from: None,
                valid_to: None,
                error: message,
            })),
        }
    }

    async fn update_edge(
        &self,
        request: tonic::Request<UpdateEdgeRequest>,
    ) -> Result<tonic::Response<MutationResponse>, Status> {
        let req = request.into_inner();

        let properties: serde_json::Value = if req.properties_json.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&req.properties_json)
                .map_err(|e| Status::invalid_argument(format!("invalid properties JSON: {e}")))?
        };

        let internal = Request::UpdateEdge {
            id: req.id,
            properties,
        };
        let (success, result_json, error) = response_to_parts(self.handler.handle(internal));
        Ok(tonic::Response::new(MutationResponse {
            success,
            result_json,
            error,
        }))
    }

    async fn delete_edge(
        &self,
        request: tonic::Request<DeleteEdgeRequest>,
    ) -> Result<tonic::Response<MutationResponse>, Status> {
        let req = request.into_inner();
        let internal = Request::DeleteEdge { id: req.id };
        let (success, result_json, error) = response_to_parts(self.handler.handle(internal));
        Ok(tonic::Response::new(MutationResponse {
            success,
            result_json,
            error,
        }))
    }

    // -- Traversal ---------------------------------------------------------

    async fn neighbors(
        &self,
        request: tonic::Request<NeighborsRequest>,
    ) -> Result<tonic::Response<NeighborsResponse>, Status> {
        let req = request.into_inner();

        let edge_type = if req.edge_type.is_empty() {
            None
        } else {
            Some(req.edge_type)
        };

        let internal = Request::Neighbors {
            id: req.id,
            direction: if req.direction.is_empty() {
                "outgoing".into()
            } else {
                req.direction
            },
            edge_type,
        };

        let resp = self.handler.handle(internal);

        match resp {
            Response::Ok { data } => {
                let neighbors = data
                    .get("neighbors")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|entry| NeighborEntry {
                                edge_id: entry
                                    .get("edge_id")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                node_id: entry
                                    .get("node_id")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(tonic::Response::new(NeighborsResponse {
                    neighbors,
                    error: String::new(),
                }))
            }
            Response::Error { message } => Ok(tonic::Response::new(NeighborsResponse {
                neighbors: vec![],
                error: message,
            })),
        }
    }

    async fn bfs(
        &self,
        request: tonic::Request<BfsRequest>,
    ) -> Result<tonic::Response<BfsResponse>, Status> {
        let req = request.into_inner();

        let max_depth = if req.max_depth == 0 {
            3
        } else {
            req.max_depth as usize
        };

        let internal = Request::Bfs {
            start: req.start,
            max_depth,
        };

        let resp = self.handler.handle(internal);

        match resp {
            Response::Ok { data } => {
                let nodes = data
                    .get("nodes")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|entry| BfsEntry {
                                node_id: entry
                                    .get("node_id")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                depth: entry
                                    .get("depth")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32,
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(tonic::Response::new(BfsResponse {
                    nodes,
                    error: String::new(),
                }))
            }
            Response::Error { message } => Ok(tonic::Response::new(BfsResponse {
                nodes: vec![],
                error: message,
            })),
        }
    }

    async fn shortest_path(
        &self,
        request: tonic::Request<ShortestPathRequest>,
    ) -> Result<tonic::Response<ShortestPathResponse>, Status> {
        let req = request.into_inner();

        let internal = Request::ShortestPath {
            from: req.from,
            to: req.to,
            weighted: req.weighted,
        };

        let resp = self.handler.handle(internal);

        match resp {
            Response::Ok { data } => {
                let path: Vec<u64> = data
                    .get("path")
                    .and_then(|v| {
                        if v.is_null() {
                            None
                        } else {
                            v.as_array().map(|arr| {
                                arr.iter().filter_map(|val| val.as_u64()).collect()
                            })
                        }
                    })
                    .unwrap_or_default();

                let found = !path.is_empty();
                let length = data
                    .get("length")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let cost = data.get("cost").and_then(|v| v.as_f64());

                Ok(tonic::Response::new(ShortestPathResponse {
                    found,
                    path,
                    length,
                    cost,
                    error: String::new(),
                }))
            }
            Response::Error { message } => Ok(tonic::Response::new(ShortestPathResponse {
                found: false,
                path: vec![],
                length: 0,
                cost: None,
                error: message,
            })),
        }
    }

    // -- Vector search -----------------------------------------------------

    async fn vector_search(
        &self,
        request: tonic::Request<VectorSearchRequest>,
    ) -> Result<tonic::Response<VectorSearchResponse>, Status> {
        let req = request.into_inner();

        let k = if req.k == 0 { 10 } else { req.k as usize };

        let internal = Request::VectorSearch {
            query: req.query,
            k,
        };

        let resp = self.handler.handle(internal);

        match resp {
            Response::Ok { data } => {
                let results = data
                    .get("results")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|entry| VectorSearchResult {
                                node_id: entry
                                    .get("node_id")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                score: entry
                                    .get("score")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(0.0)
                                    as f32,
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(tonic::Response::new(VectorSearchResponse {
                    results,
                    error: String::new(),
                }))
            }
            Response::Error { message } => Ok(tonic::Response::new(VectorSearchResponse {
                results: vec![],
                error: message,
            })),
        }
    }

    // -- GQL query ---------------------------------------------------------

    async fn query(
        &self,
        request: tonic::Request<QueryRequest>,
    ) -> Result<tonic::Response<QueryResponse>, Status> {
        let req = request.into_inner();
        let internal = Request::Query { gql: req.gql };
        let (success, result_json, error) = response_to_parts(self.handler.handle(internal));

        Ok(tonic::Response::new(QueryResponse {
            success,
            result_json,
            error,
        }))
    }

    // -- Health check ------------------------------------------------------

    async fn ping(
        &self,
        _request: tonic::Request<PingRequest>,
    ) -> Result<tonic::Response<PingResponse>, Status> {
        let internal = Request::Ping;
        let resp = self.handler.handle(internal);

        match resp {
            Response::Ok { data } => {
                let pong = data
                    .get("pong")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let version = data
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                Ok(tonic::Response::new(PingResponse { pong, version }))
            }
            Response::Error { message } => Err(Status::internal(message)),
        }
    }
}

// ---------------------------------------------------------------------------
// Server startup helper
// ---------------------------------------------------------------------------

/// Start the gRPC server on the given address.
///
/// This function runs until the server is shut down (e.g. by dropping the
/// tokio runtime or sending a signal).
pub async fn run_grpc_server(
    bind_addr: impl Into<String>,
    port: u16,
    handler: Arc<RequestHandler>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = format!("{}:{}", bind_addr.into(), port).parse()?;

    let service = AstraeaGrpcService::new(handler);

    info!("AstraeaDB gRPC server listening on {}", addr);

    tonic::transport::Server::builder()
        .add_service(service.into_service())
        .serve(addr)
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::proto::astraea_service_server::AstraeaService;

    /// Build a handler backed by an in-memory graph for testing.
    fn test_handler() -> Arc<RequestHandler> {
        let storage = astraea_graph::test_utils::InMemoryStorage::new();
        let graph = astraea_graph::Graph::new(Box::new(storage));
        let graph = std::sync::Arc::new(graph);
        Arc::new(RequestHandler::new(graph))
    }

    #[tokio::test]
    async fn grpc_ping() {
        let svc = AstraeaGrpcService::new(test_handler());
        let resp = svc
            .ping(tonic::Request::new(PingRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.pong);
        assert!(!resp.version.is_empty());
    }

    #[tokio::test]
    async fn grpc_create_and_get_node() {
        let svc = AstraeaGrpcService::new(test_handler());

        // Create
        let create_resp = svc
            .create_node(tonic::Request::new(CreateNodeRequest {
                labels: vec!["Person".into()],
                properties_json: r#"{"name":"Alice"}"#.into(),
                embedding: vec![],
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(create_resp.success, "create failed: {}", create_resp.error);

        // Parse the created node_id from the result JSON.
        let result: serde_json::Value =
            serde_json::from_str(&create_resp.result_json).unwrap();
        let node_id = result.get("node_id").and_then(|v| v.as_u64()).unwrap();

        // Get
        let get_resp = svc
            .get_node(tonic::Request::new(GetNodeRequest { id: node_id }))
            .await
            .unwrap()
            .into_inner();
        assert!(get_resp.found);
        assert_eq!(get_resp.id, node_id);
        assert_eq!(get_resp.labels, vec!["Person"]);
        assert!(get_resp.properties_json.contains("Alice"));
    }

    #[tokio::test]
    async fn grpc_create_and_get_edge() {
        let svc = AstraeaGrpcService::new(test_handler());

        // Create two nodes first.
        let resp1 = svc
            .create_node(tonic::Request::new(CreateNodeRequest {
                labels: vec!["A".into()],
                properties_json: "{}".into(),
                embedding: vec![],
            }))
            .await
            .unwrap()
            .into_inner();
        let n1: serde_json::Value = serde_json::from_str(&resp1.result_json).unwrap();
        let nid1 = n1["node_id"].as_u64().unwrap();

        let resp2 = svc
            .create_node(tonic::Request::new(CreateNodeRequest {
                labels: vec!["B".into()],
                properties_json: "{}".into(),
                embedding: vec![],
            }))
            .await
            .unwrap()
            .into_inner();
        let n2: serde_json::Value = serde_json::from_str(&resp2.result_json).unwrap();
        let nid2 = n2["node_id"].as_u64().unwrap();

        // Create edge.
        let edge_resp = svc
            .create_edge(tonic::Request::new(CreateEdgeRequest {
                source: nid1,
                target: nid2,
                edge_type: "KNOWS".into(),
                properties_json: r#"{"since":2024}"#.into(),
                weight: 1.0,
                valid_from: None,
                valid_to: None,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(edge_resp.success, "create edge failed: {}", edge_resp.error);

        let result: serde_json::Value =
            serde_json::from_str(&edge_resp.result_json).unwrap();
        let edge_id = result["edge_id"].as_u64().unwrap();

        // Get edge.
        let get_resp = svc
            .get_edge(tonic::Request::new(GetEdgeRequest { id: edge_id }))
            .await
            .unwrap()
            .into_inner();
        assert!(get_resp.found);
        assert_eq!(get_resp.source, nid1);
        assert_eq!(get_resp.target, nid2);
        assert_eq!(get_resp.edge_type, "KNOWS");
    }

    #[tokio::test]
    async fn grpc_delete_node() {
        let svc = AstraeaGrpcService::new(test_handler());

        // Create node.
        let resp = svc
            .create_node(tonic::Request::new(CreateNodeRequest {
                labels: vec!["Temp".into()],
                properties_json: "{}".into(),
                embedding: vec![],
            }))
            .await
            .unwrap()
            .into_inner();
        let result: serde_json::Value = serde_json::from_str(&resp.result_json).unwrap();
        let nid = result["node_id"].as_u64().unwrap();

        // Delete.
        let del_resp = svc
            .delete_node(tonic::Request::new(DeleteNodeRequest { id: nid }))
            .await
            .unwrap()
            .into_inner();
        assert!(del_resp.success);

        // Verify it is gone.
        let get_resp = svc
            .get_node(tonic::Request::new(GetNodeRequest { id: nid }))
            .await
            .unwrap()
            .into_inner();
        assert!(!get_resp.found);
    }

    #[tokio::test]
    async fn grpc_neighbors() {
        let svc = AstraeaGrpcService::new(test_handler());

        // Create two nodes and an edge.
        let r1 = svc
            .create_node(tonic::Request::new(CreateNodeRequest {
                labels: vec![],
                properties_json: "{}".into(),
                embedding: vec![],
            }))
            .await
            .unwrap()
            .into_inner();
        let nid1 = serde_json::from_str::<serde_json::Value>(&r1.result_json).unwrap()
            ["node_id"]
            .as_u64()
            .unwrap();

        let r2 = svc
            .create_node(tonic::Request::new(CreateNodeRequest {
                labels: vec![],
                properties_json: "{}".into(),
                embedding: vec![],
            }))
            .await
            .unwrap()
            .into_inner();
        let nid2 = serde_json::from_str::<serde_json::Value>(&r2.result_json).unwrap()
            ["node_id"]
            .as_u64()
            .unwrap();

        svc.create_edge(tonic::Request::new(CreateEdgeRequest {
            source: nid1,
            target: nid2,
            edge_type: "LINK".into(),
            properties_json: "{}".into(),
            weight: 1.0,
            valid_from: None,
            valid_to: None,
        }))
        .await
        .unwrap();

        // Query neighbors.
        let resp = svc
            .neighbors(tonic::Request::new(NeighborsRequest {
                id: nid1,
                direction: "outgoing".into(),
                edge_type: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.error.is_empty());
        assert_eq!(resp.neighbors.len(), 1);
        assert_eq!(resp.neighbors[0].node_id, nid2);
    }

    #[tokio::test]
    async fn grpc_query() {
        let svc = AstraeaGrpcService::new(test_handler());

        // Create a node via GQL.
        let resp = svc
            .query(tonic::Request::new(QueryRequest {
                gql: "CREATE (n:Test {name: 'GrpcTest'}) RETURN n".into(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success, "query failed: {}", resp.error);
    }

    #[tokio::test]
    async fn grpc_get_nonexistent_node() {
        let svc = AstraeaGrpcService::new(test_handler());

        let resp = svc
            .get_node(tonic::Request::new(GetNodeRequest { id: 99999 }))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.found);
        assert!(!resp.error.is_empty());
    }
}
