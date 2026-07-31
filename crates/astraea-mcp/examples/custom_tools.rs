//! Wiring custom tools into `astraea-mcp`'s `ToolRegistry` (astraeadb-issues #2).
//!
//! This only *constructs* the server; it doesn't call `McpServer::run`,
//! since that needs a live `astraea-server` and a real stdio transport.
//!
//! Run with: `cargo run --example custom_tools -p astraea-mcp`

use astraea_mcp::tools::{CallToolResult, ToolDefinition, ToolFuture, ToolRegistry};
use astraea_mcp::{McpServer, ProxyClient};
use serde_json::{Value, json};

/// A custom tool handler just echoes its `message` argument back. Handlers
/// have the shape `for<'a> Fn(&'a ProxyClient, Value) -> ToolFuture<'a>`;
/// a plain fn with a matching signature satisfies it directly.
fn echo(_client: &ProxyClient, args: Value) -> ToolFuture<'_> {
    Box::pin(async move {
        let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
        Ok(CallToolResult::text(json!({ "echo": message })))
    })
}

/// A second custom tool, to show `register` composes with more than one
/// addition. Real handlers would call out through `_client`, same as the
/// built-in tools in `astraea_mcp::tools::crud` etc. do.
fn word_count(_client: &ProxyClient, args: Value) -> ToolFuture<'_> {
    Box::pin(async move {
        let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
        Ok(CallToolResult::text(
            json!({ "words": text.split_whitespace().count() }),
        ))
    })
}

fn main() {
    // `ToolRegistry::new` seeds the built-in tools (crud, traversal, ...).
    let client = ProxyClient::new("127.0.0.1:7687".to_string(), None);
    let mut registry = ToolRegistry::new(client);

    registry.register(
        ToolDefinition {
            name: "echo".to_string(),
            description: "Echo a message back (example custom tool).".to_string(),
            input_schema: json!({"type": "object", "properties": {"message": {"type": "string"}}}),
        },
        echo,
    );
    registry.register(
        ToolDefinition {
            name: "word_count".to_string(),
            description: "Count words in a string (example custom tool).".to_string(),
            input_schema: json!({"type": "object", "properties": {"text": {"type": "string"}}}),
        },
        word_count,
    );

    let builtin_count = registry.list().len() - 2; // minus the 2 we just registered
    let tool_count = registry.list().len();

    // Wire the augmented registry into the server. `McpServer::new(config)`
    // would build the default (built-ins-only) registry instead.
    let _server = McpServer::new_with_tools(registry);

    eprintln!(
        "constructed McpServer with {tool_count} tools ({builtin_count} built-in + 2 custom)"
    );
}
