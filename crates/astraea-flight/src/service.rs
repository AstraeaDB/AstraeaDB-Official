//! Arrow Flight service implementation for AstraeaDB.
//!
//! Provides a gRPC-based Flight server that enables:
//! - **`do_get`**: Execute GQL queries and stream results as Arrow RecordBatches
//!   (zero-copy for Python/Polars/Pandas consumers).
//! - **`do_put`**: Bulk import nodes and edges from Arrow RecordBatches sent by clients.

use std::pin::Pin;
use std::sync::Arc;

use arrow::array::{
    Array, ArrayRef, Float64Array, Int64Array, RecordBatch, StringArray, UInt64Array,
};
use arrow_flight::{
    decode::FlightRecordBatchStream,
    encode::FlightDataEncoderBuilder,
    error::FlightError,
    flight_service_server::{FlightService, FlightServiceServer},
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor, FlightInfo,
    HandshakeRequest, HandshakeResponse, PollInfo, PutResult, SchemaResult, Ticket,
};
use futures::stream::{self, StreamExt};
use tonic::{Request, Response, Status, Streaming};

use astraea_core::traits::GraphOps;
use astraea_core::types::NodeId;
use astraea_query::executor::Executor;

use crate::schemas;

// ---------------------------------------------------------------------------
// Service struct
// ---------------------------------------------------------------------------

/// Arrow Flight service for AstraeaDB.
///
/// Wraps a [`GraphOps`] implementation and a GQL [`Executor`] to serve
/// graph data over the Arrow Flight protocol.
pub struct AstraeaFlightService {
    graph: Arc<dyn GraphOps>,
    executor: Executor,
}

impl AstraeaFlightService {
    /// Create a new Flight service backed by the given graph.
    pub fn new(graph: Arc<dyn GraphOps>) -> Self {
        let executor = Executor::new(Arc::clone(&graph));
        Self { graph, executor }
    }

    /// Convert this service into a tonic [`FlightServiceServer`] ready to
    /// be added to a tonic router.
    pub fn into_server(self) -> FlightServiceServer<Self> {
        FlightServiceServer::new(self)
    }
}

// ---------------------------------------------------------------------------
// FlightService trait implementation
// ---------------------------------------------------------------------------

type BoxedFlightStream<T> =
    Pin<Box<dyn futures::Stream<Item = Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl FlightService for AstraeaFlightService {
    // -- Stream associated types ------------------------------------------

    type HandshakeStream = BoxedFlightStream<HandshakeResponse>;
    type ListFlightsStream = BoxedFlightStream<FlightInfo>;
    type DoGetStream = BoxedFlightStream<FlightData>;
    type DoPutStream = BoxedFlightStream<PutResult>;
    type DoExchangeStream = BoxedFlightStream<FlightData>;
    type DoActionStream = BoxedFlightStream<arrow_flight::Result>;
    type ListActionsStream = BoxedFlightStream<ActionType>;

    // -- do_get: execute a GQL query and return Arrow RecordBatches --------

    async fn do_get(
        &self,
        request: Request<Ticket>,
    ) -> Result<Response<Self::DoGetStream>, Status> {
        let ticket = request.into_inner();
        let gql = String::from_utf8(ticket.ticket.to_vec())
            .map_err(|e| Status::invalid_argument(format!("invalid UTF-8 in ticket: {e}")))?;

        tracing::info!(query = %gql, "do_get: executing GQL query");

        // Parse and execute the GQL query.
        let stmt = astraea_query::parse(&gql)
            .map_err(|e| Status::invalid_argument(format!("parse error: {e}")))?;
        let result = self
            .executor
            .execute(stmt)
            .map_err(|e| Status::internal(format!("execution error: {e}")))?;

        // Build the Arrow schema from the query result columns.
        let schema = Arc::new(schemas::query_result_schema(&result.columns));

        // Convert the QueryResult into a RecordBatch.
        let batch = if result.rows.is_empty() {
            RecordBatch::new_empty(schema.clone())
        } else {
            let arrays: Vec<ArrayRef> = (0..result.columns.len())
                .map(|col_idx| {
                    let values: Vec<Option<String>> = result
                        .rows
                        .iter()
                        .map(|row| {
                            row.get(col_idx).map(|v| match v {
                                serde_json::Value::Null => None,
                                serde_json::Value::String(s) => Some(s.clone()),
                                other => Some(other.to_string()),
                            }).unwrap_or(None)
                        })
                        .collect();
                    Arc::new(StringArray::from(values)) as ArrayRef
                })
                .collect();

            RecordBatch::try_new(schema.clone(), arrays)
                .map_err(|e| Status::internal(format!("failed to create record batch: {e}")))?
        };

        // Encode as a FlightData stream.
        let flight_data_stream = FlightDataEncoderBuilder::new()
            .with_schema(schema)
            .build(futures::stream::iter(vec![Ok(batch)]))
            .map(|result| result.map_err(|e| Status::internal(e.to_string())));

        Ok(Response::new(Box::pin(flight_data_stream)))
    }

    // -- do_put: bulk import nodes/edges from Arrow RecordBatches ----------

    async fn do_put(
        &self,
        request: Request<Streaming<FlightData>>,
    ) -> Result<Response<Self::DoPutStream>, Status> {
        let stream = request.into_inner();
        let mut nodes_created: u64 = 0;
        let mut edges_created: u64 = 0;

        // Decode the incoming FlightData stream into RecordBatches.
        let flight_stream = FlightRecordBatchStream::new_from_flight_data(
            stream.map(|r| r.map_err(|e| FlightError::Tonic(Box::new(e)))),
        );

        tokio::pin!(flight_stream);

        while let Some(batch_result) = flight_stream.next().await {
            let batch = batch_result
                .map_err(|e| Status::internal(format!("decode error: {e}")))?;
            let schema = batch.schema();

            // Detect data type by looking at schema field names.
            let field_names: Vec<&str> =
                schema.fields().iter().map(|f| f.name().as_str()).collect();

            if field_names.contains(&"labels") {
                nodes_created += self.import_nodes(&batch)?;
            } else if field_names.contains(&"edge_type") {
                edges_created += self.import_edges(&batch)?;
            } else {
                return Err(Status::invalid_argument(
                    "unrecognized schema: expected node data (with 'labels' column) \
                     or edge data (with 'edge_type' column)",
                ));
            }
        }

        tracing::info!(
            nodes_created,
            edges_created,
            "do_put: bulk import complete"
        );

        let result = serde_json::json!({
            "nodes_created": nodes_created,
            "edges_created": edges_created,
        });

        let put_result = PutResult {
            app_metadata: result.to_string().into_bytes().into(),
        };

        Ok(Response::new(Box::pin(stream::once(
            async move { Ok(put_result) },
        ))))
    }

    // -- Unimplemented methods --------------------------------------------

    async fn handshake(
        &self,
        _request: Request<Streaming<HandshakeRequest>>,
    ) -> Result<Response<Self::HandshakeStream>, Status> {
        Err(Status::unimplemented("handshake not implemented"))
    }

    async fn list_flights(
        &self,
        _request: Request<Criteria>,
    ) -> Result<Response<Self::ListFlightsStream>, Status> {
        Err(Status::unimplemented("list_flights not implemented"))
    }

    async fn get_flight_info(
        &self,
        _request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        Err(Status::unimplemented("get_flight_info not implemented"))
    }

    async fn poll_flight_info(
        &self,
        _request: Request<FlightDescriptor>,
    ) -> Result<Response<PollInfo>, Status> {
        Err(Status::unimplemented("poll_flight_info not implemented"))
    }

    async fn get_schema(
        &self,
        _request: Request<FlightDescriptor>,
    ) -> Result<Response<SchemaResult>, Status> {
        Err(Status::unimplemented("get_schema not implemented"))
    }

    async fn do_exchange(
        &self,
        _request: Request<Streaming<FlightData>>,
    ) -> Result<Response<Self::DoExchangeStream>, Status> {
        Err(Status::unimplemented("do_exchange not implemented"))
    }

    async fn do_action(
        &self,
        _request: Request<Action>,
    ) -> Result<Response<Self::DoActionStream>, Status> {
        Err(Status::unimplemented("do_action not implemented"))
    }

    async fn list_actions(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::ListActionsStream>, Status> {
        Err(Status::unimplemented("list_actions not implemented"))
    }
}

// ---------------------------------------------------------------------------
// Import helpers
// ---------------------------------------------------------------------------

impl AstraeaFlightService {
    /// Import nodes from a RecordBatch with the node schema.
    ///
    /// Expected columns: `id` (ignored -- we auto-assign), `labels` (Utf8 JSON array),
    /// `properties` (Utf8 JSON), `has_embedding` (ignored for now).
    fn import_nodes(&self, batch: &RecordBatch) -> Result<u64, Status> {
        let labels_col = batch
            .column_by_name("labels")
            .ok_or_else(|| Status::invalid_argument("missing 'labels' column"))?;
        let labels_arr = labels_col
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| Status::invalid_argument("'labels' column must be Utf8"))?;

        let props_col = batch
            .column_by_name("properties")
            .ok_or_else(|| Status::invalid_argument("missing 'properties' column"))?;
        let props_arr = props_col
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| Status::invalid_argument("'properties' column must be Utf8"))?;

        let mut count = 0u64;
        for row_idx in 0..batch.num_rows() {
            // Parse labels from JSON array string.
            let labels: Vec<String> = if labels_arr.is_null(row_idx) {
                Vec::new()
            } else {
                let labels_json = labels_arr.value(row_idx);
                serde_json::from_str(labels_json).map_err(|e| {
                    Status::invalid_argument(format!(
                        "invalid JSON in 'labels' at row {row_idx}: {e}"
                    ))
                })?
            };

            // Parse properties from JSON string.
            let properties: serde_json::Value = if props_arr.is_null(row_idx) {
                serde_json::json!({})
            } else {
                let props_json = props_arr.value(row_idx);
                serde_json::from_str(props_json).map_err(|e| {
                    Status::invalid_argument(format!(
                        "invalid JSON in 'properties' at row {row_idx}: {e}"
                    ))
                })?
            };

            self.graph
                .create_node(labels, properties, None)
                .map_err(|e| Status::internal(format!("failed to create node: {e}")))?;
            count += 1;
        }

        Ok(count)
    }

    /// Import edges from a RecordBatch with the edge schema.
    ///
    /// Expected columns: `source` (UInt64), `target` (UInt64), `edge_type` (Utf8),
    /// `properties` (Utf8 JSON), `weight` (Float64), `valid_from` (Int64 nullable),
    /// `valid_to` (Int64 nullable).
    fn import_edges(&self, batch: &RecordBatch) -> Result<u64, Status> {
        let source_col = batch
            .column_by_name("source")
            .ok_or_else(|| Status::invalid_argument("missing 'source' column"))?;
        let source_arr = source_col
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| Status::invalid_argument("'source' column must be UInt64"))?;

        let target_col = batch
            .column_by_name("target")
            .ok_or_else(|| Status::invalid_argument("missing 'target' column"))?;
        let target_arr = target_col
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| Status::invalid_argument("'target' column must be UInt64"))?;

        let edge_type_col = batch
            .column_by_name("edge_type")
            .ok_or_else(|| Status::invalid_argument("missing 'edge_type' column"))?;
        let edge_type_arr = edge_type_col
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| Status::invalid_argument("'edge_type' column must be Utf8"))?;

        let props_col = batch
            .column_by_name("properties")
            .ok_or_else(|| Status::invalid_argument("missing 'properties' column"))?;
        let props_arr = props_col
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| Status::invalid_argument("'properties' column must be Utf8"))?;

        let weight_col = batch
            .column_by_name("weight")
            .ok_or_else(|| Status::invalid_argument("missing 'weight' column"))?;
        let weight_arr = weight_col
            .as_any()
            .downcast_ref::<Float64Array>()
            .ok_or_else(|| Status::invalid_argument("'weight' column must be Float64"))?;

        // Optional temporal columns.
        let valid_from_arr = batch.column_by_name("valid_from").and_then(|c| {
            c.as_any().downcast_ref::<Int64Array>()
        });
        let valid_to_arr = batch.column_by_name("valid_to").and_then(|c| {
            c.as_any().downcast_ref::<Int64Array>()
        });

        let mut count = 0u64;
        for row_idx in 0..batch.num_rows() {
            let source = NodeId(source_arr.value(row_idx));
            let target = NodeId(target_arr.value(row_idx));

            let edge_type = edge_type_arr.value(row_idx).to_string();

            let properties: serde_json::Value = if props_arr.is_null(row_idx) {
                serde_json::json!({})
            } else {
                let props_json = props_arr.value(row_idx);
                serde_json::from_str(props_json).map_err(|e| {
                    Status::invalid_argument(format!(
                        "invalid JSON in 'properties' at row {row_idx}: {e}"
                    ))
                })?
            };

            let weight = weight_arr.value(row_idx);

            let valid_from = valid_from_arr.and_then(|arr| {
                if arr.is_null(row_idx) {
                    None
                } else {
                    Some(arr.value(row_idx))
                }
            });

            let valid_to = valid_to_arr.and_then(|arr| {
                if arr.is_null(row_idx) {
                    None
                } else {
                    Some(arr.value(row_idx))
                }
            });

            self.graph
                .create_edge(source, target, edge_type, properties, weight, valid_from, valid_to)
                .map_err(|e| Status::internal(format!("failed to create edge: {e}")))?;
            count += 1;
        }

        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Server launcher helper
// ---------------------------------------------------------------------------

/// Start an Arrow Flight server on the given address.
///
/// This is a convenience function that builds a tonic transport server and
/// serves the `AstraeaFlightService`.
pub async fn run_flight_server(
    service: AstraeaFlightService,
    addr: std::net::SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Arrow Flight server listening on {}", addr);
    tonic::transport::Server::builder()
        .add_service(service.into_server())
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
    use arrow::array::BooleanArray;
    use astraea_core::error::{AstraeaError, Result as AstraeaResult};
    use astraea_core::types::*;
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    // -- In-memory GraphOps for testing -----------------------------------

    struct TestGraph {
        nodes: RwLock<HashMap<NodeId, Node>>,
        edges: RwLock<HashMap<EdgeId, Edge>>,
        next_node_id: AtomicU64,
        next_edge_id: AtomicU64,
    }

    impl TestGraph {
        fn new() -> Self {
            Self {
                nodes: RwLock::new(HashMap::new()),
                edges: RwLock::new(HashMap::new()),
                next_node_id: AtomicU64::new(1),
                next_edge_id: AtomicU64::new(1),
            }
        }
    }

    impl GraphOps for TestGraph {
        fn create_node(
            &self,
            labels: Vec<String>,
            properties: serde_json::Value,
            embedding: Option<Vec<f32>>,
        ) -> AstraeaResult<NodeId> {
            let id = NodeId(self.next_node_id.fetch_add(1, Ordering::Relaxed));
            let node = Node { id, labels, properties, embedding };
            self.nodes.write().insert(id, node);
            Ok(id)
        }

        fn create_edge(
            &self,
            source: NodeId,
            target: NodeId,
            edge_type: String,
            properties: serde_json::Value,
            weight: f64,
            valid_from: Option<i64>,
            valid_to: Option<i64>,
        ) -> AstraeaResult<EdgeId> {
            if !self.nodes.read().contains_key(&source) {
                return Err(AstraeaError::NodeNotFound(source));
            }
            if !self.nodes.read().contains_key(&target) {
                return Err(AstraeaError::NodeNotFound(target));
            }
            let id = EdgeId(self.next_edge_id.fetch_add(1, Ordering::Relaxed));
            let edge = Edge {
                id, source, target, edge_type, properties, weight,
                validity: ValidityInterval { valid_from, valid_to },
            };
            self.edges.write().insert(id, edge);
            Ok(id)
        }

        fn get_node(&self, id: NodeId) -> AstraeaResult<Option<Node>> {
            Ok(self.nodes.read().get(&id).cloned())
        }

        fn get_edge(&self, id: EdgeId) -> AstraeaResult<Option<Edge>> {
            Ok(self.edges.read().get(&id).cloned())
        }

        fn update_node(&self, id: NodeId, properties: serde_json::Value) -> AstraeaResult<()> {
            let mut nodes = self.nodes.write();
            let node = nodes.get_mut(&id).ok_or(AstraeaError::NodeNotFound(id))?;
            if let (Some(target_map), serde_json::Value::Object(patch_map)) =
                (node.properties.as_object_mut(), &properties)
            {
                for (k, v) in patch_map {
                    let key: String = k.clone();
                    let val: serde_json::Value = v.clone();
                    target_map.insert(key, val);
                }
            }
            Ok(())
        }

        fn update_edge(&self, id: EdgeId, properties: serde_json::Value) -> AstraeaResult<()> {
            let mut edges = self.edges.write();
            let edge = edges.get_mut(&id).ok_or(AstraeaError::EdgeNotFound(id))?;
            if let (Some(target_map), serde_json::Value::Object(patch_map)) =
                (edge.properties.as_object_mut(), &properties)
            {
                for (k, v) in patch_map {
                    let key: String = k.clone();
                    let val: serde_json::Value = v.clone();
                    target_map.insert(key, val);
                }
            }
            Ok(())
        }

        fn delete_node(&self, id: NodeId) -> AstraeaResult<()> {
            let edge_ids: Vec<EdgeId> = self.edges.read()
                .values()
                .filter(|e| e.source == id || e.target == id)
                .map(|e| e.id)
                .collect();
            for eid in edge_ids {
                self.edges.write().remove(&eid);
            }
            self.nodes.write().remove(&id);
            Ok(())
        }

        fn delete_edge(&self, id: EdgeId) -> AstraeaResult<()> {
            self.edges.write().remove(&id);
            Ok(())
        }

        fn neighbors(&self, node_id: NodeId, direction: Direction) -> AstraeaResult<Vec<(EdgeId, NodeId)>> {
            let edges = self.edges.read();
            Ok(edges.values()
                .filter(|e| match direction {
                    Direction::Outgoing => e.source == node_id,
                    Direction::Incoming => e.target == node_id,
                    Direction::Both => e.source == node_id || e.target == node_id,
                })
                .map(|e| {
                    let neighbor = if e.source == node_id { e.target } else { e.source };
                    (e.id, neighbor)
                })
                .collect())
        }

        fn neighbors_filtered(
            &self,
            node_id: NodeId,
            direction: Direction,
            edge_type: &str,
        ) -> AstraeaResult<Vec<(EdgeId, NodeId)>> {
            let edges = self.edges.read();
            Ok(edges.values()
                .filter(|e| {
                    e.edge_type == edge_type && match direction {
                        Direction::Outgoing => e.source == node_id,
                        Direction::Incoming => e.target == node_id,
                        Direction::Both => e.source == node_id || e.target == node_id,
                    }
                })
                .map(|e| {
                    let neighbor = if e.source == node_id { e.target } else { e.source };
                    (e.id, neighbor)
                })
                .collect())
        }

        fn bfs(&self, _start: NodeId, _max_depth: usize) -> AstraeaResult<Vec<(NodeId, usize)>> {
            Ok(Vec::new())
        }

        fn dfs(&self, _start: NodeId, _max_depth: usize) -> AstraeaResult<Vec<NodeId>> {
            Ok(Vec::new())
        }

        fn shortest_path(&self, _from: NodeId, _to: NodeId) -> AstraeaResult<Option<GraphPath>> {
            Ok(None)
        }

        fn shortest_path_weighted(
            &self,
            _from: NodeId,
            _to: NodeId,
        ) -> AstraeaResult<Option<(GraphPath, f64)>> {
            Ok(None)
        }

        fn find_by_label(&self, label: &str) -> AstraeaResult<Vec<NodeId>> {
            let nodes = self.nodes.read();
            if label.is_empty() {
                Ok(nodes.keys().copied().collect())
            } else {
                Ok(nodes.values()
                    .filter(|n| n.labels.contains(&label.to_string()))
                    .map(|n| n.id)
                    .collect())
            }
        }
    }

    // -- Test helpers -----------------------------------------------------

    fn make_service() -> (Arc<TestGraph>, AstraeaFlightService) {
        let graph = Arc::new(TestGraph::new());
        let service = AstraeaFlightService::new(graph.clone() as Arc<dyn GraphOps>);
        (graph, service)
    }

    fn make_populated_service() -> (Arc<TestGraph>, AstraeaFlightService) {
        let (graph, service) = make_service();
        graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Alice", "age": 25}),
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
        (graph, service)
    }

    // -- do_get tests -----------------------------------------------------

    #[tokio::test]
    async fn test_do_get_basic_query() {
        let (_graph, service) = make_populated_service();

        let ticket = Ticket {
            ticket: "MATCH (n:Person) RETURN n.name".as_bytes().into(),
        };
        let response = service
            .do_get(Request::new(ticket))
            .await
            .expect("do_get failed");

        let mut stream = response.into_inner();
        let mut flight_data_items = Vec::new();
        while let Some(item) = stream.next().await {
            flight_data_items.push(item.expect("stream error"));
        }

        // Should have received at least one FlightData message (schema + data).
        assert!(!flight_data_items.is_empty());
    }

    #[tokio::test]
    async fn test_do_get_empty_result() {
        let (_graph, service) = make_populated_service();

        let ticket = Ticket {
            ticket: "MATCH (n:Person) WHERE n.age > 100 RETURN n.name"
                .as_bytes()
                .into(),
        };
        let response = service
            .do_get(Request::new(ticket))
            .await
            .expect("do_get failed");

        let mut stream = response.into_inner();
        let mut flight_data_items = Vec::new();
        while let Some(item) = stream.next().await {
            flight_data_items.push(item.expect("stream error"));
        }

        // Even empty results should produce at least the schema message.
        assert!(!flight_data_items.is_empty());
    }

    #[tokio::test]
    async fn test_do_get_invalid_query() {
        let (_graph, service) = make_service();

        let ticket = Ticket {
            ticket: "THIS IS NOT VALID GQL".as_bytes().into(),
        };
        let result = service.do_get(Request::new(ticket)).await;
        assert!(result.is_err());
        let status = result.err().expect("expected error");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_do_get_query_with_where() {
        let (_graph, service) = make_populated_service();

        let ticket = Ticket {
            ticket: "MATCH (n:Person) WHERE n.age > 30 RETURN n.name"
                .as_bytes()
                .into(),
        };
        let response = service
            .do_get(Request::new(ticket))
            .await
            .expect("do_get failed");

        let mut stream = response.into_inner();
        let mut flight_data_items = Vec::new();
        while let Some(item) = stream.next().await {
            flight_data_items.push(item.expect("stream error"));
        }

        assert!(!flight_data_items.is_empty());
    }

    // -- import_nodes tests (unit, no gRPC) -------------------------------

    #[test]
    fn test_import_nodes_from_batch() {
        let (graph, service) = make_service();

        let schema = Arc::new(schemas::node_schema());
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![0, 0])) as ArrayRef,
                Arc::new(StringArray::from(vec![
                    r#"["Person"]"#,
                    r#"["Company"]"#,
                ])) as ArrayRef,
                Arc::new(StringArray::from(vec![
                    r#"{"name":"Charlie"}"#,
                    r#"{"name":"Acme"}"#,
                ])) as ArrayRef,
                Arc::new(BooleanArray::from(vec![false, false])) as ArrayRef,
            ],
        )
        .unwrap();

        let count = service.import_nodes(&batch).unwrap();
        assert_eq!(count, 2);

        // Verify the nodes were created.
        let n1 = graph.get_node(NodeId(1)).unwrap().unwrap();
        assert_eq!(n1.labels, vec!["Person"]);
        assert_eq!(n1.properties["name"], "Charlie");

        let n2 = graph.get_node(NodeId(2)).unwrap().unwrap();
        assert_eq!(n2.labels, vec!["Company"]);
        assert_eq!(n2.properties["name"], "Acme");
    }

    // -- import_edges tests (unit, no gRPC) -------------------------------

    #[test]
    fn test_import_edges_from_batch() {
        let (graph, service) = make_service();

        // First create two nodes so edges have valid endpoints.
        graph
            .create_node(vec!["A".into()], serde_json::json!({}), None)
            .unwrap();
        graph
            .create_node(vec!["B".into()], serde_json::json!({}), None)
            .unwrap();

        let schema = Arc::new(schemas::edge_schema());
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![0u64])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![1u64])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![2u64])) as ArrayRef,
                Arc::new(StringArray::from(vec!["KNOWS"])) as ArrayRef,
                Arc::new(StringArray::from(vec![r#"{"since":2024}"#])) as ArrayRef,
                Arc::new(Float64Array::from(vec![0.75])) as ArrayRef,
                Arc::new(Int64Array::from(vec![None::<i64>])) as ArrayRef,
                Arc::new(Int64Array::from(vec![None::<i64>])) as ArrayRef,
            ],
        )
        .unwrap();

        let count = service.import_edges(&batch).unwrap();
        assert_eq!(count, 1);

        // Verify the edge was created.
        let e = graph.get_edge(EdgeId(1)).unwrap().unwrap();
        assert_eq!(e.source, NodeId(1));
        assert_eq!(e.target, NodeId(2));
        assert_eq!(e.edge_type, "KNOWS");
        assert_eq!(e.weight, 0.75);
        assert_eq!(e.properties["since"], 2024);
    }

    #[test]
    fn test_import_edges_with_temporal() {
        let (graph, service) = make_service();

        graph
            .create_node(vec!["X".into()], serde_json::json!({}), None)
            .unwrap();
        graph
            .create_node(vec!["Y".into()], serde_json::json!({}), None)
            .unwrap();

        let schema = Arc::new(schemas::edge_schema());
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![0u64])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![1u64])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![2u64])) as ArrayRef,
                Arc::new(StringArray::from(vec!["LINKED"])) as ArrayRef,
                Arc::new(StringArray::from(vec![r#"{}"#])) as ArrayRef,
                Arc::new(Float64Array::from(vec![1.0])) as ArrayRef,
                Arc::new(Int64Array::from(vec![Some(1000i64)])) as ArrayRef,
                Arc::new(Int64Array::from(vec![Some(2000i64)])) as ArrayRef,
            ],
        )
        .unwrap();

        let count = service.import_edges(&batch).unwrap();
        assert_eq!(count, 1);

        let e = graph.get_edge(EdgeId(1)).unwrap().unwrap();
        assert_eq!(e.validity.valid_from, Some(1000));
        assert_eq!(e.validity.valid_to, Some(2000));
    }
}
