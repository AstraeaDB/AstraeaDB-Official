use crate::types::{EdgeId, NodeId, PageId};

/// Top-level error type for AstraeaDB.
#[derive(Debug, thiserror::Error)]
pub enum AstraeaError {
    // --- Storage errors ---
    #[error("page {0} not found")]
    PageNotFound(PageId),

    #[error("page {0} is corrupted: {1}")]
    PageCorrupted(PageId, String),

    #[error("buffer pool is full, cannot pin page {0}")]
    BufferPoolFull(PageId),

    #[error("storage I/O error: {0}")]
    StorageIo(#[from] std::io::Error),

    // --- Graph errors ---
    #[error("node {0} not found")]
    NodeNotFound(NodeId),

    #[error("edge {0} not found")]
    EdgeNotFound(EdgeId),

    #[error("duplicate node {0}")]
    DuplicateNode(NodeId),

    #[error("duplicate edge {0}")]
    DuplicateEdge(EdgeId),

    // --- Transaction errors ---
    #[error("transaction aborted: write-write conflict on entity {0}")]
    WriteConflict(u64),

    #[error("transaction not active")]
    TransactionNotActive,

    // --- Query errors ---
    #[error("parse error at position {position}: {message}")]
    ParseError { position: usize, message: String },

    #[error("query execution error: {0}")]
    QueryExecution(String),

    #[error("unknown label: {0}")]
    UnknownLabel(String),

    #[error("unknown edge type: {0}")]
    UnknownEdgeType(String),

    // --- Vector errors ---
    #[error("embedding dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },

    #[error("node {0} has no embedding")]
    NoEmbedding(NodeId),

    #[error("vector index not built")]
    IndexNotBuilt,

    // --- Serialization errors ---
    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("deserialization error: {0}")]
    Deserialization(String),

    // --- Configuration errors ---
    #[error("configuration error: {0}")]
    Config(String),

    // --- Authentication errors ---
    #[error("authentication required")]
    AuthenticationRequired,

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("access denied: {0}")]
    AccessDenied(String),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, AstraeaError>;
