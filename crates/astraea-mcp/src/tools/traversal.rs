use serde_json::{json, Value};

use astraea_server::protocol::Request;
use crate::client::ProxyClient;
use crate::errors::McpError;
use super::{CallToolResult, ToolDefinition};

/// Return tool definitions for all traversal operations.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "neighbors".to_string(),
            description: "Get the neighbors of a node, optionally filtered by direction and edge type.".to_string(),
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
                    "edge_type": {
                        "type": "string",
                        "description": "Optional edge type filter."
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "bfs".to_string(),
            description: "Breadth-first search traversal from a starting node, returning nodes and their depths.".to_string(),
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
                    }
                },
                "required": ["start"]
            }),
        },
        ToolDefinition {
            name: "dfs".to_string(),
            description: "Depth-first search traversal from a starting node.".to_string(),
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
                    }
                },
                "required": ["start"]
            }),
        },
        ToolDefinition {
            name: "shortest_path".to_string(),
            description: "Find the shortest path between two nodes. Supports weighted (Dijkstra) and unweighted (BFS) modes.".to_string(),
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
                    "weighted": {
                        "type": "boolean",
                        "description": "Whether to use weighted (Dijkstra) or unweighted (BFS) shortest path. Defaults to false."
                    }
                },
                "required": ["from", "to"]
            }),
        },
    ]
}

/// Get the neighbors of a node.
pub async fn neighbors(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let id = args
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: id".to_string()))?;

    let direction = args
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("outgoing")
        .to_string();

    let edge_type = args
        .get("edge_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let request = Request::Neighbors {
        id,
        direction,
        edge_type,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Breadth-first search traversal from a starting node.
pub async fn bfs(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let start = args
        .get("start")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: start".to_string()))?;

    let max_depth = args
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(3);

    let request = Request::Bfs { start, max_depth };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Depth-first search traversal from a starting node.
pub async fn dfs(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let start = args
        .get("start")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: start".to_string()))?;

    let max_depth = args
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(3);

    let request = Request::Dfs { start, max_depth };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Find the shortest path between two nodes.
pub async fn shortest_path(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let from = args
        .get("from")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: from".to_string()))?;

    let to = args
        .get("to")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: to".to_string()))?;

    let weighted = args
        .get("weighted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let request = Request::ShortestPath { from, to, weighted };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}
