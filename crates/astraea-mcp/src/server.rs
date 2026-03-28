use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::ProxyClient;
use crate::errors::{JsonRpcError, McpError, PARSE_ERROR};
use crate::prompts;
use crate::resources;
use crate::tools::ToolRegistry;
use crate::transport::Transport;

const PROTOCOL_VERSION: &str = "2025-03-26";
const SERVER_NAME: &str = "astraea-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

/// Configuration for the MCP server.
pub struct McpConfig {
    pub address: String,
    pub auth_token: Option<String>,
}

/// The MCP server. Reads JSON-RPC messages from a transport, dispatches them,
/// and writes responses back.
pub struct McpServer {
    tools: ToolRegistry,
    client: ProxyClient,
    initialized: bool,
}

impl McpServer {
    pub fn new(config: McpConfig) -> Self {
        let client = ProxyClient::new(config.address, config.auth_token);
        // The ToolRegistry needs its own client. Since ProxyClient is cheap
        // (just holds address + token), we create a second one.
        let tools_client = ProxyClient::new(
            client.address().to_string(),
            client.auth_token().cloned(),
        );
        let tools = ToolRegistry::new(tools_client);

        Self {
            tools,
            client,
            initialized: false,
        }
    }

    /// Run the MCP server loop on the given transport until the transport closes.
    pub async fn run<T: Transport>(&mut self, transport: &mut T) -> Result<(), McpError> {
        loop {
            let message = match transport.read_message().await {
                Ok(Some(msg)) => msg,
                Ok(None) => {
                    tracing::info!("transport closed, shutting down");
                    return Ok(());
                }
                Err(e) => {
                    tracing::error!("transport read error: {e}");
                    return Err(McpError::Io(e));
                }
            };

            let trimmed = message.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse JSON-RPC request.
            let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
                Ok(req) => req,
                Err(e) => {
                    let resp = JsonRpcResponse::error(
                        Value::Null,
                        JsonRpcError {
                            code: PARSE_ERROR,
                            message: format!("parse error: {e}"),
                            data: None,
                        },
                    );
                    let out = serde_json::to_string(&resp).unwrap_or_default();
                    let _ = transport.write_message(&out).await;
                    continue;
                }
            };

            // Notifications (no id) don't get responses.
            let is_notification = request.id.is_none();
            let id = request.id.clone().unwrap_or(Value::Null);

            // Dispatch.
            let result = self.dispatch(&request).await;

            // Notifications: handle `initialized` etc. silently.
            if is_notification {
                continue;
            }

            let resp = match result {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(id, e.to_json_rpc_error()),
            };

            let out = serde_json::to_string(&resp).unwrap_or_default();
            transport.write_message(&out).await?;
        }
    }

    async fn dispatch(&mut self, req: &JsonRpcRequest) -> Result<Value, McpError> {
        let params = req.params.clone().unwrap_or(Value::Object(Default::default()));

        match req.method.as_str() {
            // ----- Lifecycle -----
            "initialize" => self.handle_initialize(&params),
            "initialized" => {
                // Notification: mark as initialized.
                self.initialized = true;
                Ok(Value::Null)
            }
            "ping" => Ok(serde_json::json!({})),

            // ----- Tools -----
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(&params).await,

            // ----- Resources -----
            "resources/list" => self.handle_resources_list(),
            "resources/templates/list" => self.handle_resource_templates_list(),
            "resources/read" => self.handle_resources_read(&params).await,

            // ----- Prompts -----
            "prompts/list" => self.handle_prompts_list(),
            "prompts/get" => self.handle_prompts_get(&params),

            // ----- Unknown -----
            method => Err(McpError::MethodNotFound(method.to_string())),
        }
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    fn handle_initialize(&mut self, _params: &Value) -> Result<Value, McpError> {
        self.initialized = true;

        Ok(serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false },
                "prompts": { "listChanged": false }
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            }
        }))
    }

    // -----------------------------------------------------------------------
    // Tools
    // -----------------------------------------------------------------------

    fn handle_tools_list(&self) -> Result<Value, McpError> {
        let tools = self.tools.list();
        Ok(serde_json::json!({ "tools": tools }))
    }

    async fn handle_tools_call(&self, params: &Value) -> Result<Value, McpError> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidRequest("tools/call missing 'name'".into()))?;

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        let result = self.tools.call(name, arguments).await?;
        Ok(serde_json::to_value(result).unwrap_or(Value::Null))
    }

    // -----------------------------------------------------------------------
    // Resources
    // -----------------------------------------------------------------------

    fn handle_resources_list(&self) -> Result<Value, McpError> {
        let resources = resources::static_resources();
        Ok(serde_json::json!({ "resources": resources }))
    }

    fn handle_resource_templates_list(&self) -> Result<Value, McpError> {
        let templates = resources::resource_templates();
        Ok(serde_json::json!({ "resourceTemplates": templates }))
    }

    async fn handle_resources_read(&self, params: &Value) -> Result<Value, McpError> {
        let uri = params
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidRequest("resources/read missing 'uri'".into()))?;

        let content = resources::read_resource(&self.client, uri).await?;
        Ok(serde_json::json!({ "contents": [content] }))
    }

    // -----------------------------------------------------------------------
    // Prompts
    // -----------------------------------------------------------------------

    fn handle_prompts_list(&self) -> Result<Value, McpError> {
        let prompts = prompts::definitions();
        Ok(serde_json::json!({ "prompts": prompts }))
    }

    fn handle_prompts_get(&self, params: &Value) -> Result<Value, McpError> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidRequest("prompts/get missing 'name'".into()))?;

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        let messages = prompts::get_prompt(name, &arguments)?;
        Ok(serde_json::json!({ "messages": messages }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    // --- JSON-RPC serialization tests ---

    #[test]
    fn json_rpc_response_success_serializes() {
        let resp = JsonRpcResponse::success(
            Value::Number(1.into()),
            serde_json::json!({"status": "ok"}),
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn json_rpc_response_error_serializes() {
        let resp = JsonRpcResponse::error(
            Value::Number(2.into()),
            JsonRpcError {
                code: -32601,
                message: "method not found".into(),
                data: None,
            },
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"error\""));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn parse_json_rpc_request() {
        let input = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let req: JsonRpcRequest = serde_json::from_str(input).unwrap();
        assert_eq!(req.method, "initialize");
        assert_eq!(req.id, Some(Value::Number(1.into())));
    }

    #[test]
    fn parse_notification() {
        let input = r#"{"jsonrpc":"2.0","method":"initialized"}"#;
        let req: JsonRpcRequest = serde_json::from_str(input).unwrap();
        assert_eq!(req.method, "initialized");
        assert!(req.id.is_none());
    }

    // --- Tool definition tests ---

    #[test]
    fn tool_definitions_have_valid_schemas() {
        let config = McpConfig {
            address: "127.0.0.1:7687".into(),
            auth_token: None,
        };
        let server = McpServer::new(config);
        let tools = server.tools.list();

        // We expect at least 28 tools (8 CRUD + 4 traversal + 3 search +
        // 5 algorithm + 4 temporal + 2 RAG + 3 admin)
        assert!(tools.len() >= 28, "expected >= 28 tools, got {}", tools.len());

        for tool in &tools {
            assert!(!tool.name.is_empty(), "tool name must not be empty");
            assert!(!tool.description.is_empty(), "tool '{}' has empty description", tool.name);

            // inputSchema must be an object with "type": "object"
            let schema = &tool.input_schema;
            assert_eq!(
                schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "tool '{}' inputSchema must have type: object",
                tool.name
            );

            // Must have a "properties" key
            assert!(
                schema.get("properties").is_some(),
                "tool '{}' inputSchema must have properties",
                tool.name
            );
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let config = McpConfig {
            address: "127.0.0.1:7687".into(),
            auth_token: None,
        };
        let server = McpServer::new(config);
        let tools = server.tools.list();

        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate tool names found");
    }

    // --- Prompt tests ---

    #[test]
    fn prompt_definitions_are_complete() {
        let prompts = crate::prompts::definitions();
        assert_eq!(prompts.len(), 6);

        let names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"analyze-node"));
        assert!(names.contains(&"explain-path"));
        assert!(names.contains(&"explore-community"));
        assert!(names.contains(&"summarize-graph"));
        assert!(names.contains(&"temporal-diff"));
        assert!(names.contains(&"rag-query"));
    }

    #[test]
    fn prompt_get_analyze_node() {
        let args = serde_json::json!({"node_id": 42});
        let messages = crate::prompts::get_prompt("analyze-node", &args).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert!(messages[0].content.text.contains("42"));
    }

    #[test]
    fn prompt_get_summarize_graph_no_args() {
        let args = serde_json::json!({});
        let messages = crate::prompts::get_prompt("summarize-graph", &args).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].content.text.contains("graph_stats"));
    }

    #[test]
    fn prompt_get_missing_required_arg() {
        let args = serde_json::json!({});
        let result = crate::prompts::get_prompt("analyze-node", &args);
        assert!(result.is_err());
    }

    #[test]
    fn prompt_get_unknown_name() {
        let result = crate::prompts::get_prompt("nonexistent", &serde_json::json!({}));
        assert!(result.is_err());
    }

    // --- Mock transport for MCP session tests ---

    struct MockTransport {
        incoming: Mutex<VecDeque<String>>,
        outgoing: Mutex<Vec<String>>,
    }

    impl MockTransport {
        fn new(messages: Vec<&str>) -> Self {
            Self {
                incoming: Mutex::new(messages.into_iter().map(String::from).collect()),
                outgoing: Mutex::new(Vec::new()),
            }
        }

        fn responses(&self) -> Vec<String> {
            self.outgoing.lock().unwrap().clone()
        }
    }

    impl Transport for MockTransport {
        async fn read_message(&mut self) -> std::io::Result<Option<String>> {
            let mut incoming = self.incoming.lock().unwrap();
            Ok(incoming.pop_front())
        }

        async fn write_message(&mut self, message: &str) -> std::io::Result<()> {
            self.outgoing.lock().unwrap().push(message.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn mcp_session_initialize_and_list_tools() {
        let config = McpConfig {
            address: "127.0.0.1:9999".into(),
            auth_token: None,
        };
        let mut server = McpServer::new(config);

        let mut transport = MockTransport::new(vec![
            // 1. Initialize
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#,
            // 2. Initialized notification (no response expected)
            r#"{"jsonrpc":"2.0","method":"initialized"}"#,
            // 3. List tools
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            // 4. List resources
            r#"{"jsonrpc":"2.0","id":3,"method":"resources/list","params":{}}"#,
            // 5. List prompts
            r#"{"jsonrpc":"2.0","id":4,"method":"prompts/list","params":{}}"#,
            // 6. Ping
            r#"{"jsonrpc":"2.0","id":5,"method":"ping","params":{}}"#,
            // 7. Unknown method
            r#"{"jsonrpc":"2.0","id":6,"method":"unknown/method","params":{}}"#,
        ]);

        server.run(&mut transport).await.unwrap();

        let responses = transport.responses();

        // Should have 6 responses (notification has no response).
        assert_eq!(responses.len(), 6, "expected 6 responses, got {}", responses.len());

        // 1. Initialize response
        let init: Value = serde_json::from_str(&responses[0]).unwrap();
        assert_eq!(init["id"], 1);
        assert!(init["result"]["protocolVersion"].is_string());
        assert!(init["result"]["capabilities"]["tools"].is_object());
        assert_eq!(init["result"]["serverInfo"]["name"], "astraea-mcp");

        // 2. Tools list
        let tools_resp: Value = serde_json::from_str(&responses[1]).unwrap();
        assert_eq!(tools_resp["id"], 2);
        let tools = tools_resp["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 28);

        // 3. Resources list
        let res_resp: Value = serde_json::from_str(&responses[2]).unwrap();
        assert_eq!(res_resp["id"], 3);
        let resources = res_resp["result"]["resources"].as_array().unwrap();
        assert!(!resources.is_empty());

        // 4. Prompts list
        let prompts_resp: Value = serde_json::from_str(&responses[3]).unwrap();
        assert_eq!(prompts_resp["id"], 4);
        let prompts = prompts_resp["result"]["prompts"].as_array().unwrap();
        assert_eq!(prompts.len(), 6);

        // 5. Ping
        let ping_resp: Value = serde_json::from_str(&responses[4]).unwrap();
        assert_eq!(ping_resp["id"], 5);
        assert!(ping_resp["result"].is_object());

        // 6. Unknown method -> error
        let err_resp: Value = serde_json::from_str(&responses[5]).unwrap();
        assert_eq!(err_resp["id"], 6);
        assert!(err_resp["error"].is_object());
        assert_eq!(err_resp["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn mcp_session_malformed_json() {
        let config = McpConfig {
            address: "127.0.0.1:9999".into(),
            auth_token: None,
        };
        let mut server = McpServer::new(config);

        let mut transport = MockTransport::new(vec![
            "this is not json",
        ]);

        server.run(&mut transport).await.unwrap();

        let responses = transport.responses();
        assert_eq!(responses.len(), 1);

        let resp: Value = serde_json::from_str(&responses[0]).unwrap();
        assert_eq!(resp["error"]["code"], -32700); // Parse error
    }

    #[tokio::test]
    async fn mcp_session_prompts_get() {
        let config = McpConfig {
            address: "127.0.0.1:9999".into(),
            auth_token: None,
        };
        let mut server = McpServer::new(config);

        let mut transport = MockTransport::new(vec![
            r#"{"jsonrpc":"2.0","id":1,"method":"prompts/get","params":{"name":"summarize-graph","arguments":{}}}"#,
        ]);

        server.run(&mut transport).await.unwrap();

        let responses = transport.responses();
        assert_eq!(responses.len(), 1);

        let resp: Value = serde_json::from_str(&responses[0]).unwrap();
        assert_eq!(resp["id"], 1);
        let messages = resp["result"]["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[tokio::test]
    async fn mcp_session_resource_templates() {
        let config = McpConfig {
            address: "127.0.0.1:9999".into(),
            auth_token: None,
        };
        let mut server = McpServer::new(config);

        let mut transport = MockTransport::new(vec![
            r#"{"jsonrpc":"2.0","id":1,"method":"resources/templates/list","params":{}}"#,
        ]);

        server.run(&mut transport).await.unwrap();

        let responses = transport.responses();
        let resp: Value = serde_json::from_str(&responses[0]).unwrap();
        let templates = resp["result"]["resourceTemplates"].as_array().unwrap();
        assert_eq!(templates.len(), 4);
    }
}
