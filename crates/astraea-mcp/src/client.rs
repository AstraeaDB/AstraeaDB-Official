use astraea_server::protocol::{Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::errors::McpError;

/// TCP client that proxies MCP tool calls to a running AstraeaDB server.
pub struct ProxyClient {
    address: String,
    auth_token: Option<String>,
}

impl ProxyClient {
    pub fn new(address: String, auth_token: Option<String>) -> Self {
        Self {
            address,
            auth_token,
        }
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn auth_token(&self) -> Option<&String> {
        self.auth_token.as_ref()
    }

    /// Send a `Request` to the AstraeaDB TCP server and return the `Response`.
    pub async fn send(&self, request: &Request) -> Result<Response, McpError> {
        let mut stream = TcpStream::connect(&self.address)
            .await
            .map_err(|e| McpError::Connection(format!("failed to connect to {}: {e}", self.address)))?;

        let (reader, mut writer) = stream.split();

        // Serialize request, optionally injecting auth token.
        let mut json_value = serde_json::to_value(request)
            .map_err(|e| McpError::Internal(format!("failed to serialize request: {e}")))?;

        if let Some(ref token) = self.auth_token {
            if let Some(obj) = json_value.as_object_mut() {
                obj.insert("auth_token".to_string(), serde_json::Value::String(token.clone()));
            }
        }

        let mut msg = serde_json::to_string(&json_value)
            .map_err(|e| McpError::Internal(format!("failed to serialize request: {e}")))?;
        msg.push('\n');

        writer
            .write_all(msg.as_bytes())
            .await
            .map_err(|e| McpError::Connection(format!("failed to write to server: {e}")))?;

        let mut reader = BufReader::new(reader);
        let mut response_str = String::new();
        reader
            .read_line(&mut response_str)
            .await
            .map_err(|e| McpError::Connection(format!("failed to read from server: {e}")))?;

        let response: Response = serde_json::from_str(response_str.trim())
            .map_err(|e| McpError::Internal(format!("failed to parse server response: {e}")))?;

        Ok(response)
    }

    /// Send a raw JSON value (already a Request) and return the response data.
    /// Convenience wrapper that extracts the data from `Response::Ok` or returns an error.
    pub async fn send_and_unwrap(&self, request: &Request) -> Result<serde_json::Value, McpError> {
        match self.send(request).await? {
            Response::Ok { data } => Ok(data),
            Response::Error { message } => Err(McpError::Internal(message)),
        }
    }
}
