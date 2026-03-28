pub mod admin;
pub mod algorithms;
pub mod crud;
pub mod rag;
pub mod search;
pub mod temporal;
pub mod traversal;

use serde::Serialize;
use serde_json::Value;

use crate::client::ProxyClient;
use crate::errors::McpError;

/// A single MCP tool definition returned by `tools/list`.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Result of a tool call, per MCP spec.
#[derive(Debug, Clone, Serialize)]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    #[serde(rename = "isError", skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl CallToolResult {
    pub fn text(data: impl Serialize) -> Self {
        let text = match serde_json::to_string_pretty(&data) {
            Ok(s) => s,
            Err(e) => format!("{{\"error\": \"serialization failed: {e}\"}}"),
        };
        Self {
            content: vec![ToolContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent {
                content_type: "text".to_string(),
                text: message.into(),
            }],
            is_error: true,
        }
    }
}

/// The tool registry: holds all tool definitions and dispatches calls.
pub struct ToolRegistry {
    client: ProxyClient,
}

impl ToolRegistry {
    pub fn new(client: ProxyClient) -> Self {
        Self { client }
    }

    /// Return all tool definitions for `tools/list`.
    pub fn list(&self) -> Vec<ToolDefinition> {
        let mut tools = Vec::new();
        tools.extend(crud::definitions());
        tools.extend(traversal::definitions());
        tools.extend(search::definitions());
        tools.extend(algorithms::definitions());
        tools.extend(temporal::definitions());
        tools.extend(rag::definitions());
        tools.extend(admin::definitions());
        tools
    }

    /// Dispatch a tool call by name.
    pub async fn call(&self, name: &str, args: Value) -> Result<CallToolResult, McpError> {
        match name {
            // CRUD
            "create_node" => crud::create_node(&self.client, args).await,
            "create_edge" => crud::create_edge(&self.client, args).await,
            "get_node" => crud::get_node(&self.client, args).await,
            "get_edge" => crud::get_edge(&self.client, args).await,
            "update_node" => crud::update_node(&self.client, args).await,
            "update_edge" => crud::update_edge(&self.client, args).await,
            "delete_node" => crud::delete_node(&self.client, args).await,
            "delete_edge" => crud::delete_edge(&self.client, args).await,

            // Traversal
            "neighbors" => traversal::neighbors(&self.client, args).await,
            "bfs" => traversal::bfs(&self.client, args).await,
            "dfs" => traversal::dfs(&self.client, args).await,
            "shortest_path" => traversal::shortest_path(&self.client, args).await,

            // Search
            "vector_search" => search::vector_search(&self.client, args).await,
            "hybrid_search" => search::hybrid_search(&self.client, args).await,
            "find_by_label" => search::find_by_label(&self.client, args).await,

            // Algorithms
            "run_pagerank" => algorithms::run_pagerank(&self.client, args).await,
            "run_louvain" => algorithms::run_louvain(&self.client, args).await,
            "run_connected_components" => algorithms::run_connected_components(&self.client, args).await,
            "run_degree_centrality" => algorithms::run_degree_centrality(&self.client, args).await,
            "run_betweenness_centrality" => algorithms::run_betweenness_centrality(&self.client, args).await,

            // Temporal
            "neighbors_at" => temporal::neighbors_at(&self.client, args).await,
            "bfs_at" => temporal::bfs_at(&self.client, args).await,
            "dfs_at" => temporal::dfs_at(&self.client, args).await,
            "shortest_path_at" => temporal::shortest_path_at(&self.client, args).await,

            // RAG
            "graph_rag" => rag::graph_rag(&self.client, args).await,
            "extract_subgraph" => rag::extract_subgraph(&self.client, args).await,

            // Admin
            "query" => admin::query(&self.client, args).await,
            "graph_stats" => admin::graph_stats(&self.client, args).await,
            "ping" => admin::ping(&self.client, args).await,

            _ => Err(McpError::ToolNotFound(name.to_string())),
        }
    }
}
