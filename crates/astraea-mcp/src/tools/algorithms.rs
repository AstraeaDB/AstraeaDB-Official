use serde_json::{json, Value};

use astraea_server::protocol::Request;

use crate::client::ProxyClient;
use crate::errors::McpError;

use super::{CallToolResult, ToolDefinition};

/// Return tool definitions for graph algorithm operations.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "run_pagerank".to_string(),
            description: "Run the PageRank algorithm to compute node importance scores.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "nodes": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Optional subset of node IDs to run PageRank on. If omitted, runs on all nodes."
                    },
                    "damping": {
                        "type": "number",
                        "description": "Damping factor (default 0.85)."
                    },
                    "max_iterations": {
                        "type": "integer",
                        "description": "Maximum number of iterations (default 100)."
                    },
                    "tolerance": {
                        "type": "number",
                        "description": "Convergence tolerance (default 1e-6)."
                    }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "run_louvain".to_string(),
            description: "Run Louvain community detection to find clusters of densely connected nodes.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "nodes": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Optional subset of node IDs. If omitted, runs on all nodes."
                    }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "run_connected_components".to_string(),
            description: "Find connected components in the graph. Supports strong and weak connectivity.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "nodes": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Optional subset of node IDs. If omitted, runs on all nodes."
                    },
                    "strong": {
                        "type": "boolean",
                        "description": "If true, find strongly connected components; otherwise weakly connected (default false)."
                    }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "run_degree_centrality".to_string(),
            description: "Compute degree centrality for nodes (count of connections).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "nodes": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Optional subset of node IDs. If omitted, runs on all nodes."
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["incoming", "outgoing", "both"],
                        "description": "Edge direction to count (default \"outgoing\")."
                    }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "run_betweenness_centrality".to_string(),
            description: "Compute betweenness centrality (how often a node lies on shortest paths between others).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "nodes": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Optional subset of node IDs. If omitted, runs on all nodes."
                    }
                },
                "additionalProperties": false
            }),
        },
    ]
}

/// Run the PageRank algorithm.
pub async fn run_pagerank(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let nodes: Option<Vec<u64>> = args
        .get("nodes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect());

    let damping = args
        .get("damping")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.85);

    let max_iterations = args
        .get("max_iterations")
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as usize;

    let tolerance = args
        .get("tolerance")
        .and_then(|v| v.as_f64())
        .unwrap_or(1e-6);

    let request = Request::RunPageRank {
        nodes,
        damping,
        max_iterations,
        tolerance,
    };

    let data = client.send_and_unwrap(&request).await?;
    Ok(CallToolResult::text(data))
}

/// Run Louvain community detection.
pub async fn run_louvain(client: &ProxyClient, args: Value) -> Result<CallToolResult, McpError> {
    let nodes: Option<Vec<u64>> = args
        .get("nodes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect());

    let request = Request::RunLouvain { nodes };

    let data = client.send_and_unwrap(&request).await?;
    Ok(CallToolResult::text(data))
}

/// Find connected components in the graph.
pub async fn run_connected_components(
    client: &ProxyClient,
    args: Value,
) -> Result<CallToolResult, McpError> {
    let nodes: Option<Vec<u64>> = args
        .get("nodes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect());

    let strong = args
        .get("strong")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let request = Request::RunConnectedComponents { nodes, strong };

    let data = client.send_and_unwrap(&request).await?;
    Ok(CallToolResult::text(data))
}

/// Compute degree centrality for nodes.
pub async fn run_degree_centrality(
    client: &ProxyClient,
    args: Value,
) -> Result<CallToolResult, McpError> {
    let nodes: Option<Vec<u64>> = args
        .get("nodes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect());

    let direction = args
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("outgoing")
        .to_string();

    let request = Request::RunDegreeCentrality { nodes, direction };

    let data = client.send_and_unwrap(&request).await?;
    Ok(CallToolResult::text(data))
}

/// Compute betweenness centrality.
pub async fn run_betweenness_centrality(
    client: &ProxyClient,
    args: Value,
) -> Result<CallToolResult, McpError> {
    let nodes: Option<Vec<u64>> = args
        .get("nodes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect());

    let request = Request::RunBetweennessCentrality { nodes };

    let data = client.send_and_unwrap(&request).await?;
    Ok(CallToolResult::text(data))
}
