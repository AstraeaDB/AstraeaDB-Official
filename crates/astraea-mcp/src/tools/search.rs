use serde_json::{json, Value};

use astraea_server::protocol::Request;

use crate::client::ProxyClient;
use crate::errors::McpError;
use super::{CallToolResult, ToolDefinition};

/// Return MCP tool definitions for search operations.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "vector_search".to_string(),
            description: "Search for nodes by vector similarity using the HNSW index.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Query embedding vector (array of floats)."
                    },
                    "k": {
                        "type": "integer",
                        "description": "Number of nearest neighbors to return (default 10).",
                        "default": 10
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "hybrid_search".to_string(),
            description: "Combine graph proximity and vector similarity to find relevant nodes.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "anchor": {
                        "type": "integer",
                        "description": "Anchor node ID to start the graph walk from."
                    },
                    "query": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Query embedding vector (array of floats)."
                    },
                    "max_hops": {
                        "type": "integer",
                        "description": "Maximum graph hops from the anchor (default 3).",
                        "default": 3
                    },
                    "k": {
                        "type": "integer",
                        "description": "Number of results to return (default 10).",
                        "default": 10
                    },
                    "alpha": {
                        "type": "number",
                        "description": "Weight between graph proximity (0.0) and vector similarity (1.0). Default 0.5.",
                        "default": 0.5
                    }
                },
                "required": ["anchor", "query"]
            }),
        },
        ToolDefinition {
            name: "find_by_label".to_string(),
            description: "Find all nodes with a given label.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "label": {
                        "type": "string",
                        "description": "The label to search for."
                    }
                },
                "required": ["label"]
            }),
        },
    ]
}

/// Search for nodes by vector similarity.
pub async fn vector_search(
    client: &ProxyClient,
    args: Value,
) -> Result<CallToolResult, McpError> {
    let query: Vec<f32> = args
        .get("query")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            McpError::InvalidParams("missing required field: query (array of numbers)".into())
        })?
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect();

    let k = args
        .get("k")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(10);

    let request = Request::VectorSearch { query, k };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Combine graph proximity and vector similarity to find relevant nodes.
pub async fn hybrid_search(
    client: &ProxyClient,
    args: Value,
) -> Result<CallToolResult, McpError> {
    let anchor = args
        .get("anchor")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            McpError::InvalidParams("missing required field: anchor (integer)".into())
        })?;

    let query: Vec<f32> = args
        .get("query")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            McpError::InvalidParams("missing required field: query (array of numbers)".into())
        })?
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect();

    let max_hops = args
        .get("max_hops")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(3);

    let k = args
        .get("k")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(10);

    let alpha = args
        .get("alpha")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(0.5);

    let request = Request::HybridSearch {
        anchor,
        query,
        max_hops,
        k,
        alpha,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Find all nodes with a given label.
pub async fn find_by_label(
    client: &ProxyClient,
    args: Value,
) -> Result<CallToolResult, McpError> {
    let label = args
        .get("label")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            McpError::InvalidParams("missing required field: label (string)".into())
        })?
        .to_string();

    let request = Request::FindByLabel { label };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}
