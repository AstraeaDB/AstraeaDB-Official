pub mod admin;
pub mod algorithms;
pub mod crud;
pub mod rag;
pub mod search;
pub mod temporal;
pub mod traversal;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

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

/// Boxed future returned by a [`ToolHandler`] call. Mirrors the shape an
/// `async fn(&ProxyClient, Value) -> Result<CallToolResult, McpError>` would
/// have once boxed, since `ToolHandler` can't use `async fn` directly and
/// stay object-safe.
pub type ToolFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CallToolResult, McpError>> + Send + 'a>>;

/// A custom tool's handler: given the registry's shared [`ProxyClient`] and
/// the call's JSON arguments, resolves to a [`CallToolResult`].
///
/// Implemented automatically for any function or closure of the shape
/// `for<'a> Fn(&'a ProxyClient, Value) -> ToolFuture<'a>` (`Send + Sync`), so
/// callers normally don't implement this trait by hand — see
/// `examples/custom_tools.rs`.
pub trait ToolHandler: Send + Sync {
    fn call<'a>(&'a self, client: &'a ProxyClient, args: Value) -> ToolFuture<'a>;
}

impl<F> ToolHandler for F
where
    F: for<'a> Fn(&'a ProxyClient, Value) -> ToolFuture<'a> + Send + Sync,
{
    fn call<'a>(&'a self, client: &'a ProxyClient, args: Value) -> ToolFuture<'a> {
        self(client, args)
    }
}

/// The tool registry: holds all tool definitions and dispatches calls.
///
/// `ToolRegistry::new` always seeds the built-in tools from `crud`,
/// `traversal`, `search`, `algorithms`, `temporal`, `rag`, and `admin`. Callers
/// that need additional tools can layer them on top with [`Self::register`];
/// a custom tool whose name matches a built-in shadows it in both
/// [`Self::list`] and [`Self::call`], so `register` also covers "replace a
/// built-in" use cases. See `examples/custom_tools.rs` for a worked example
/// and [`crate::McpServer::new_with_tools`] for wiring a custom registry
/// into the server.
pub struct ToolRegistry {
    client: ProxyClient,
    custom_definitions: Vec<ToolDefinition>,
    custom_handlers: HashMap<String, Arc<dyn ToolHandler>>,
}

impl ToolRegistry {
    pub fn new(client: ProxyClient) -> Self {
        Self {
            client,
            custom_definitions: Vec::new(),
            custom_handlers: HashMap::new(),
        }
    }

    /// The `ProxyClient` this registry (and any custom handlers registered
    /// on it) dispatch through.
    pub fn client(&self) -> &ProxyClient {
        &self.client
    }

    /// Register a custom tool. `definition.name` is the key `tools/call`
    /// dispatches on; if it matches a built-in tool's name, the custom
    /// handler takes priority over (shadows) the built-in in both `list()`
    /// and `call()`.
    pub fn register(&mut self, definition: ToolDefinition, handler: impl ToolHandler + 'static) {
        self.custom_handlers
            .insert(definition.name.clone(), Arc::new(handler));
        self.custom_definitions.push(definition);
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

        // Custom tools shadow built-ins of the same name.
        tools.retain(|t| !self.custom_handlers.contains_key(&t.name));
        tools.extend(self.custom_definitions.clone());
        tools
    }

    /// Dispatch a tool call by name. Custom tools registered via
    /// [`Self::register`] take priority over built-ins with the same name.
    pub async fn call(&self, name: &str, args: Value) -> Result<CallToolResult, McpError> {
        if let Some(handler) = self.custom_handlers.get(name) {
            return handler.call(&self.client, args).await;
        }

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
            "run_connected_components" => {
                algorithms::run_connected_components(&self.client, args).await
            }
            "run_degree_centrality" => algorithms::run_degree_centrality(&self.client, args).await,
            "run_betweenness_centrality" => {
                algorithms::run_betweenness_centrality(&self.client, args).await
            }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> ProxyClient {
        ProxyClient::new("127.0.0.1:1".to_string(), None)
    }

    fn echo(_client: &ProxyClient, args: Value) -> ToolFuture<'_> {
        Box::pin(async move { Ok(CallToolResult::text(args)) })
    }

    #[tokio::test]
    async fn register_adds_a_custom_tool_alongside_builtins() {
        let mut registry = ToolRegistry::new(test_client());
        let builtin_count = registry.list().len();

        registry.register(
            ToolDefinition {
                name: "echo".to_string(),
                description: "echoes args back".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            echo,
        );

        let tools = registry.list();
        assert_eq!(tools.len(), builtin_count + 1);
        assert!(tools.iter().any(|t| t.name == "echo"));

        let result = registry
            .call("echo", serde_json::json!({"hello": "world"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("world"));
    }

    #[tokio::test]
    async fn register_shadows_a_builtin_of_the_same_name() {
        let mut registry = ToolRegistry::new(test_client());
        let builtin_count = registry.list().len();

        registry.register(
            ToolDefinition {
                name: "ping".to_string(),
                description: "custom ping override".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            echo,
        );

        // Shadowing doesn't change the tool count -- the built-in "ping"
        // definition is dropped in favor of the custom one.
        let tools = registry.list();
        assert_eq!(tools.len(), builtin_count);
        let ping_defs: Vec<_> = tools.iter().filter(|t| t.name == "ping").collect();
        assert_eq!(ping_defs.len(), 1);
        assert_eq!(ping_defs[0].description, "custom ping override");

        // Dispatch goes to the custom handler, not `admin::ping`.
        let result = registry
            .call("ping", serde_json::json!({"marker": "shadowed"}))
            .await
            .unwrap();
        assert!(result.content[0].text.contains("shadowed"));
    }

    #[tokio::test]
    async fn call_unknown_tool_still_errors() {
        let registry = ToolRegistry::new(test_client());
        let err = registry
            .call("nonexistent_tool", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::ToolNotFound(_)));
    }
}
