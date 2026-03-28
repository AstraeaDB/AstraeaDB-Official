use serde_json::{json, Value};

use astraea_server::protocol::Request;
use crate::client::ProxyClient;
use crate::errors::McpError;
use super::{CallToolResult, ToolDefinition};

/// Return tool definitions for admin/utility operations.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "query".to_string(),
            description: "Execute a GQL (Graph Query Language) query string.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "gql": {
                        "type": "string",
                        "description": "The GQL query string to execute."
                    }
                },
                "required": ["gql"]
            }),
        },
        ToolDefinition {
            name: "graph_stats".to_string(),
            description: "Get graph statistics including node count, edge count, and labels.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "ping".to_string(),
            description: "Check if the AstraeaDB server is reachable.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Execute a GQL query string.
pub async fn query(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let gql = args
        .get("gql")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: gql".to_string()))?
        .to_string();

    let request = Request::Query { gql };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Get graph statistics.
pub async fn graph_stats(client: &ProxyClient, _args: Value) -> Result<CallToolResult, McpError> {
    let request = Request::GraphStats;

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Check if the AstraeaDB server is reachable.
pub async fn ping(client: &ProxyClient, _args: Value) -> Result<CallToolResult, McpError> {
    let request = Request::Ping;

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}
