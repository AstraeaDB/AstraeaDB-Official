use serde_json::{json, Value};

use astraea_server::protocol::Request;
use crate::client::ProxyClient;
use crate::errors::McpError;
use super::{CallToolResult, ToolDefinition};

/// Return tool definitions for RAG operations.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "graph_rag".to_string(),
            description: "Answer a natural language question using graph-augmented retrieval. Extracts a subgraph around the most relevant node, linearizes it as context, and queries the configured LLM.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The natural language question to answer."
                    },
                    "question_embedding": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Optional pre-computed embedding vector for the question. If omitted, the server uses its configured embedding model."
                    },
                    "anchor": {
                        "type": "integer",
                        "description": "Optional anchor node ID to center the subgraph extraction around."
                    },
                    "hops": {
                        "type": "integer",
                        "description": "Number of hops from the anchor node to include in the subgraph. Defaults to 2."
                    },
                    "max_nodes": {
                        "type": "integer",
                        "description": "Maximum number of nodes to include in the subgraph context. Defaults to 50."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["structured", "prose", "cypher"],
                        "description": "Linearization format for the subgraph context. Defaults to \"structured\"."
                    }
                },
                "required": ["question"]
            }),
        },
        ToolDefinition {
            name: "extract_subgraph".to_string(),
            description: "Extract a subgraph around a center node and linearize it as text. Useful for getting context about a node's neighborhood.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "center": {
                        "type": "integer",
                        "description": "The center node ID to extract the subgraph around."
                    },
                    "hops": {
                        "type": "integer",
                        "description": "Number of hops from the center node to include. Defaults to 3."
                    },
                    "max_nodes": {
                        "type": "integer",
                        "description": "Maximum number of nodes to include. Defaults to 50."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["structured", "prose", "cypher"],
                        "description": "Linearization format for the subgraph. Defaults to \"structured\"."
                    }
                },
                "required": ["center"]
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Answer a natural language question using graph-augmented retrieval.
pub async fn graph_rag(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let question = args
        .get("question")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: question".to_string()))?
        .to_string();

    let question_embedding: Option<Vec<f32>> = args
        .get("question_embedding")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let anchor: Option<u64> = args
        .get("anchor")
        .and_then(|v| v.as_u64());

    let hops = args
        .get("hops")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(2);

    let max_nodes = args
        .get("max_nodes")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(50);

    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("structured")
        .to_string();

    let request = Request::GraphRag {
        question,
        question_embedding,
        anchor,
        hops,
        max_nodes,
        format,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}

/// Extract a subgraph around a center node and linearize it as text.
pub async fn extract_subgraph(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let center = args
        .get("center")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| McpError::InvalidParams("missing required parameter: center".to_string()))?;

    let hops = args
        .get("hops")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(3);

    let max_nodes = args
        .get("max_nodes")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(50);

    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("structured")
        .to_string();

    let request = Request::ExtractSubgraph {
        center,
        hops,
        max_nodes,
        format,
    };

    match client.send_and_unwrap(&request).await {
        Ok(data) => Ok(CallToolResult::text(data)),
        Err(e) => Ok(CallToolResult::error(e.to_string())),
    }
}
