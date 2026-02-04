use serde::{Deserialize, Serialize};

/// Client request sent over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    /// Create a new node.
    CreateNode {
        labels: Vec<String>,
        properties: serde_json::Value,
        #[serde(default)]
        embedding: Option<Vec<f32>>,
    },
    /// Create a new edge between two nodes.
    CreateEdge {
        source: u64,
        target: u64,
        edge_type: String,
        #[serde(default = "default_properties")]
        properties: serde_json::Value,
        #[serde(default = "default_weight")]
        weight: f64,
        /// Optional temporal validity start (epoch milliseconds, inclusive).
        #[serde(default)]
        valid_from: Option<i64>,
        /// Optional temporal validity end (epoch milliseconds, exclusive).
        #[serde(default)]
        valid_to: Option<i64>,
    },
    /// Get a node by ID.
    GetNode { id: u64 },
    /// Get an edge by ID.
    GetEdge { id: u64 },
    /// Update a node's properties.
    UpdateNode {
        id: u64,
        properties: serde_json::Value,
    },
    /// Update an edge's properties.
    UpdateEdge {
        id: u64,
        properties: serde_json::Value,
    },
    /// Delete a node (and its edges).
    DeleteNode { id: u64 },
    /// Delete an edge.
    DeleteEdge { id: u64 },
    /// Get neighbors of a node.
    Neighbors {
        id: u64,
        direction: String, // "outgoing", "incoming", "both"
        #[serde(default)]
        edge_type: Option<String>,
    },
    /// Run a BFS traversal.
    Bfs {
        start: u64,
        #[serde(default = "default_max_depth")]
        max_depth: usize,
    },
    /// Find shortest path between two nodes.
    ShortestPath {
        from: u64,
        to: u64,
        #[serde(default)]
        weighted: bool,
    },
    /// Vector similarity search.
    VectorSearch {
        query: Vec<f32>,
        #[serde(default = "default_k")]
        k: usize,
    },
    /// Execute a GQL query string.
    Query { gql: String },
    /// Server status / health check.
    Ping,
}

/// Server response sent back to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum Response {
    /// Successful operation with a result payload.
    #[serde(rename = "ok")]
    Ok { data: serde_json::Value },
    /// Operation failed with an error message.
    #[serde(rename = "error")]
    Error { message: String },
}

impl Response {
    pub fn ok(data: impl Serialize) -> Self {
        Self::Ok {
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self::Error {
            message: msg.into(),
        }
    }
}

fn default_properties() -> serde_json::Value {
    serde_json::json!({})
}

fn default_weight() -> f64 {
    1.0
}

fn default_max_depth() -> usize {
    3
}

fn default_k() -> usize {
    10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_create_node_request() {
        let req = Request::CreateNode {
            labels: vec!["Person".into()],
            properties: serde_json::json!({"name": "Alice"}),
            embedding: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("CreateNode"));
        assert!(json.contains("Alice"));
    }

    #[test]
    fn deserialize_create_node_request() {
        let json = r#"{"type":"CreateNode","labels":["Person"],"properties":{"name":"Bob"}}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::CreateNode {
                labels, properties, ..
            } => {
                assert_eq!(labels, vec!["Person"]);
                assert_eq!(properties["name"], "Bob");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn response_ok() {
        let resp = Response::ok(serde_json::json!({"id": 42}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("ok"));
        assert!(json.contains("42"));
    }

    #[test]
    fn response_error() {
        let resp = Response::error("not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("error"));
        assert!(json.contains("not found"));
    }

    #[test]
    fn deserialize_ping() {
        let json = r#"{"type":"Ping"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert!(matches!(req, Request::Ping));
    }
}
