pub mod stdio;

/// Trait for MCP transports (stdio, SSE, etc.).
///
/// A transport reads JSON-RPC messages from the client and writes responses back.
pub trait Transport: Send + Sync {
    /// Read the next JSON-RPC message from the client.
    /// Returns `None` when the transport is closed (e.g., stdin EOF).
    fn read_message(
        &mut self,
    ) -> impl std::future::Future<Output = std::io::Result<Option<String>>> + Send;

    /// Write a JSON-RPC message to the client.
    fn write_message(
        &mut self,
        message: &str,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send;
}
