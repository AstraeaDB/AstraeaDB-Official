use serde_json::{json, Value};

use astraea_server::protocol::Request;

use crate::client::ProxyClient;
use crate::errors::McpError;

use super::{CallToolResult, ToolDefinition};

/// Return MCP tool definitions for all CRUD operations.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "create_node".to_string(),
            description: "Create a new node in the graph with labels, properties, and an optional embedding vector.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "One or more labels for the node (e.g. [\"Person\", \"Employee\"])."
                    },
                    "properties": {
                        "type": "object",
                        "description": "Arbitrary key-value properties for the node. Defaults to {}."
                    },
                    "embedding": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Optional embedding vector for vector similarity search."
                    }
                },
                "required": ["labels"]
            }),
        },
        ToolDefinition {
            name: "create_edge".to_string(),
            description: "Create a directed edge between two nodes with a type, properties, weight, and optional temporal validity.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": {
                        "type": "integer",
                        "description": "ID of the source node."
                    },
                    "target": {
                        "type": "integer",
                        "description": "ID of the target node."
                    },
                    "edge_type": {
                        "type": "string",
                        "description": "The relationship type (e.g. \"KNOWS\", \"WORKS_AT\")."
                    },
                    "properties": {
                        "type": "object",
                        "description": "Arbitrary key-value properties for the edge. Defaults to {}."
                    },
                    "weight": {
                        "type": "number",
                        "description": "Numeric weight of the edge. Defaults to 1.0."
                    },
                    "valid_from": {
                        "type": "integer",
                        "description": "Optional temporal validity start (epoch milliseconds, inclusive)."
                    },
                    "valid_to": {
                        "type": "integer",
                        "description": "Optional temporal validity end (epoch milliseconds, exclusive)."
                    }
                },
                "required": ["source", "target", "edge_type"]
            }),
        },
        ToolDefinition {
            name: "get_node".to_string(),
            description: "Retrieve a node by its ID, returning its labels, properties, and embedding.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The node ID to look up."
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "get_edge".to_string(),
            description: "Retrieve an edge by its ID, returning its source, target, type, properties, and weight.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The edge ID to look up."
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "update_node".to_string(),
            description: "Update the properties of an existing node. The new properties are merged with (or replace) the existing ones.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The node ID to update."
                    },
                    "properties": {
                        "type": "object",
                        "description": "New properties to set on the node."
                    }
                },
                "required": ["id", "properties"]
            }),
        },
        ToolDefinition {
            name: "update_edge".to_string(),
            description: "Update the properties of an existing edge. The new properties are merged with (or replace) the existing ones.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The edge ID to update."
                    },
                    "properties": {
                        "type": "object",
                        "description": "New properties to set on the edge."
                    }
                },
                "required": ["id", "properties"]
            }),
        },
        ToolDefinition {
            name: "delete_node".to_string(),
            description: "Delete a node and all of its connected edges from the graph.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The node ID to delete."
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "delete_edge".to_string(),
            description: "Delete an edge from the graph by its ID.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The edge ID to delete."
                    }
                },
                "required": ["id"]
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn create_node(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let labels: Vec<String> = args
        .get("labels")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| McpError::InvalidParams("missing required field: labels".into()))?;

    let properties = args
        .get("properties")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let embedding: Option<Vec<f32>> = args
        .get("embedding")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let request = Request::CreateNode {
        labels,
        properties,
        embedding,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

pub async fn create_edge(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let source = args
        .get("source")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required field: source".into()))?;

    let target = args
        .get("target")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required field: target".into()))?;

    let edge_type = args
        .get("edge_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("missing required field: edge_type".into()))?
        .to_string();

    let properties = args
        .get("properties")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let weight = args
        .get("weight")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);

    let valid_from: Option<i64> = args.get("valid_from").and_then(|v| v.as_i64());

    let valid_to: Option<i64> = args.get("valid_to").and_then(|v| v.as_i64());

    let request = Request::CreateEdge {
        source,
        target,
        edge_type,
        properties,
        weight,
        valid_from,
        valid_to,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

pub async fn get_node(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let id = args
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required field: id".into()))?;

    let request = Request::GetNode { id };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

pub async fn get_edge(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let id = args
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required field: id".into()))?;

    let request = Request::GetEdge { id };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

pub async fn update_node(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let id = args
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required field: id".into()))?;

    let properties = args
        .get("properties")
        .cloned()
        .ok_or_else(|| McpError::InvalidParams("missing required field: properties".into()))?;

    let request = Request::UpdateNode { id, properties };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

pub async fn update_edge(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let id = args
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required field: id".into()))?;

    let properties = args
        .get("properties")
        .cloned()
        .ok_or_else(|| McpError::InvalidParams("missing required field: properties".into()))?;

    let request = Request::UpdateEdge { id, properties };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

pub async fn delete_node(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let id = args
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required field: id".into()))?;

    let request = Request::DeleteNode { id };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

pub async fn delete_edge(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let id = args
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required field: id".into()))?;

    let request = Request::DeleteEdge { id };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}
