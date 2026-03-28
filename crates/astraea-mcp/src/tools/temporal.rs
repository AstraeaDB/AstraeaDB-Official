use serde_json::{json, Value};

use astraea_server::protocol::Request;
use crate::client::ProxyClient;
use crate::errors::McpError;
use super::{CallToolResult, ToolDefinition};

/// Return tool definitions for all temporal query operations.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "neighbors_at".to_string(),
            description: "Get the neighbors of a node at a specific point in time, filtering edges by their temporal validity.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The node ID to get neighbors of."
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["outgoing", "incoming", "both"],
                        "description": "Direction of edges to follow. Defaults to \"outgoing\"."
                    },
                    "timestamp": {
                        "type": "integer",
                        "description": "Point in time (epoch milliseconds) to evaluate edge validity."
                    },
                    "edge_type": {
                        "type": "string",
                        "description": "Optional edge type filter."
                    }
                },
                "required": ["id", "timestamp"]
            }),
        },
        ToolDefinition {
            name: "bfs_at".to_string(),
            description: "Breadth-first search traversal from a starting node at a specific point in time, only following temporally valid edges.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "start": {
                        "type": "integer",
                        "description": "The starting node ID."
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum traversal depth. Defaults to 3."
                    },
                    "timestamp": {
                        "type": "integer",
                        "description": "Point in time (epoch milliseconds) to evaluate edge validity."
                    }
                },
                "required": ["start", "timestamp"]
            }),
        },
        ToolDefinition {
            name: "dfs_at".to_string(),
            description: "Depth-first search traversal from a starting node at a specific point in time, only following temporally valid edges.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "start": {
                        "type": "integer",
                        "description": "The starting node ID."
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum traversal depth. Defaults to 3."
                    },
                    "timestamp": {
                        "type": "integer",
                        "description": "Point in time (epoch milliseconds) to evaluate edge validity."
                    }
                },
                "required": ["start", "timestamp"]
            }),
        },
        ToolDefinition {
            name: "shortest_path_at".to_string(),
            description: "Find the shortest path between two nodes at a specific point in time, only following temporally valid edges.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "from": {
                        "type": "integer",
                        "description": "The source node ID."
                    },
                    "to": {
                        "type": "integer",
                        "description": "The target node ID."
                    },
                    "timestamp": {
                        "type": "integer",
                        "description": "Point in time (epoch milliseconds) to evaluate edge validity."
                    },
                    "weighted": {
                        "type": "boolean",
                        "description": "Whether to use weighted (Dijkstra) or unweighted (BFS) shortest path. Defaults to false."
                    }
                },
                "required": ["from", "to", "timestamp"]
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Get the neighbors of a node at a specific point in time.
pub async fn neighbors_at(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let id = args
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: id".to_string()))?;

    let direction = args
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("outgoing")
        .to_string();

    let timestamp = args
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: timestamp".to_string()))?;

    let edge_type = args
        .get("edge_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let request = Request::NeighborsAt {
        id,
        direction,
        timestamp,
        edge_type,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Breadth-first search traversal at a specific point in time.
pub async fn bfs_at(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let start = args
        .get("start")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: start".to_string()))?;

    let max_depth = args
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(3);

    let timestamp = args
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: timestamp".to_string()))?;

    let request = Request::BfsAt {
        start,
        max_depth,
        timestamp,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Depth-first search traversal at a specific point in time.
pub async fn dfs_at(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let start = args
        .get("start")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: start".to_string()))?;

    let max_depth = args
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(3);

    let timestamp = args
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: timestamp".to_string()))?;

    let request = Request::DfsAt {
        start,
        max_depth,
        timestamp,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Find the shortest path between two nodes at a specific point in time.
pub async fn shortest_path_at(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let from = args
        .get("from")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: from".to_string()))?;

    let to = args
        .get("to")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: to".to_string()))?;

    let timestamp = args
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: timestamp".to_string()))?;

    let weighted = args
        .get("weighted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let request = Request::ShortestPathAt {
        from,
        to,
        timestamp,
        weighted,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}
