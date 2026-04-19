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
    /// Hybrid search combining graph proximity and vector similarity.
    HybridSearch {
        anchor: u64,
        query: Vec<f32>,
        #[serde(default = "default_max_depth")]
        max_hops: usize,
        #[serde(default = "default_k")]
        k: usize,
        #[serde(default = "default_alpha")]
        alpha: f32,
    },
    /// Rank neighbors by semantic similarity to a concept embedding.
    SemanticNeighbors {
        id: u64,
        concept: Vec<f32>,
        #[serde(default = "default_direction")]
        direction: String,
        #[serde(default = "default_k")]
        k: usize,
    },
    /// Greedy multi-hop walk toward a semantic concept.
    SemanticWalk {
        start: u64,
        concept: Vec<f32>,
        #[serde(default = "default_max_depth")]
        max_hops: usize,
    },
    /// Execute a GQL query string.
    Query { gql: String },
    /// Extract a subgraph around a node and linearize it.
    ExtractSubgraph {
        center: u64,
        #[serde(default = "default_max_depth")]
        hops: usize,
        #[serde(default = "default_max_context_nodes")]
        max_nodes: usize,
        #[serde(default = "default_text_format")]
        format: String,
    },
    /// Execute a GraphRAG query (requires LLM provider configuration).
    GraphRag {
        question: String,
        #[serde(default)]
        question_embedding: Option<Vec<f32>>,
        #[serde(default)]
        anchor: Option<u64>,
        #[serde(default = "default_max_depth")]
        hops: usize,
        #[serde(default = "default_max_context_nodes")]
        max_nodes: usize,
        #[serde(default = "default_text_format")]
        format: String,
    },
    /// Get neighbors of a node at a specific point in time.
    NeighborsAt {
        id: u64,
        direction: String,
        timestamp: i64,
        #[serde(default)]
        edge_type: Option<String>,
    },
    /// Run a BFS traversal at a specific point in time.
    BfsAt {
        start: u64,
        #[serde(default = "default_max_depth")]
        max_depth: usize,
        timestamp: i64,
    },
    /// Find shortest path at a specific point in time.
    ShortestPathAt {
        from: u64,
        to: u64,
        timestamp: i64,
        #[serde(default)]
        weighted: bool,
    },
    /// Depth-first search traversal.
    Dfs {
        start: u64,
        #[serde(default = "default_max_depth")]
        max_depth: usize,
    },
    /// Depth-first search traversal at a specific point in time.
    DfsAt {
        start: u64,
        #[serde(default = "default_max_depth")]
        max_depth: usize,
        timestamp: i64,
    },
    /// Find nodes by label.
    FindByLabel { label: String },
    /// Delete every node carrying the given label (and all its edges).
    /// Returns `{"deleted": N}`. astraeadb-issues.md #4.
    DeleteByLabel { label: String },
    /// Find all edges whose edge_type matches the given string.
    /// Returns `{"edges": [{"edge_id": N, "source": N, "target": N}, ...]}`.
    /// astraeadb-issues.md #3.
    FindEdgeByType { edge_type: String },
    /// Run PageRank algorithm.
    RunPageRank {
        #[serde(default)]
        nodes: Option<Vec<u64>>,
        #[serde(default = "default_damping")]
        damping: f64,
        #[serde(default = "default_max_iterations")]
        max_iterations: usize,
        #[serde(default = "default_tolerance")]
        tolerance: f64,
    },
    /// Run Louvain community detection.
    RunLouvain {
        #[serde(default)]
        nodes: Option<Vec<u64>>,
    },
    /// Run connected components detection.
    RunConnectedComponents {
        #[serde(default)]
        nodes: Option<Vec<u64>>,
        #[serde(default)]
        strong: bool,
    },
    /// Run degree centrality.
    RunDegreeCentrality {
        #[serde(default)]
        nodes: Option<Vec<u64>>,
        #[serde(default = "default_direction")]
        direction: String,
    },
    /// Run betweenness centrality.
    RunBetweennessCentrality {
        #[serde(default)]
        nodes: Option<Vec<u64>>,
    },
    /// Get graph statistics.
    GraphStats,
    /// Get raw subgraph (nodes + edges) for visualization.
    GetSubgraph {
        center: u64,
        #[serde(default = "default_max_depth")]
        hops: usize,
        #[serde(default = "default_max_context_nodes")]
        max_nodes: usize,
    },
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

fn default_alpha() -> f32 {
    0.5
}

fn default_direction() -> String {
    "outgoing".to_string()
}

fn default_max_context_nodes() -> usize {
    50
}

fn default_damping() -> f64 {
    0.85
}

fn default_max_iterations() -> usize {
    100
}

fn default_tolerance() -> f64 {
    1e-6
}

fn default_text_format() -> String {
    "structured".to_string()
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
