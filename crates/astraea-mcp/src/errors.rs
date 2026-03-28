use serde::Serialize;

/// Standard JSON-RPC 2.0 error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// MCP-level error type.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("parse error: {0}")]
    ParseError(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("method not found: {0}")]
    MethodNotFound(String),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("tool not found: {0}")]
    ToolNotFound(String),

    #[error("resource not found: {0}")]
    ResourceNotFound(String),

    #[error("prompt not found: {0}")]
    PromptNotFound(String),

    #[error("connection error: {0}")]
    Connection(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl McpError {
    pub fn code(&self) -> i64 {
        match self {
            McpError::ParseError(_) => PARSE_ERROR,
            McpError::InvalidRequest(_) => INVALID_REQUEST,
            McpError::MethodNotFound(_) => METHOD_NOT_FOUND,
            McpError::InvalidParams(_) | McpError::ToolNotFound(_) => INVALID_PARAMS,
            McpError::ResourceNotFound(_) | McpError::PromptNotFound(_) => INVALID_PARAMS,
            McpError::Internal(_) | McpError::Connection(_) | McpError::Io(_) => INTERNAL_ERROR,
        }
    }

    pub fn to_json_rpc_error(&self) -> JsonRpcError {
        JsonRpcError {
            code: self.code(),
            message: self.to_string(),
            data: None,
        }
    }
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
