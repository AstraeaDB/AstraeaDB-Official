use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use crate::auth::AuthManager;
use crate::connection::{ConnectionConfig, ConnectionManager};
use crate::handler::RequestHandler;
use crate::metrics::ServerMetrics;
use crate::protocol::{Request, Response};

/// Configuration for the AstraeaDB server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
    pub connection: ConnectionConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".into(),
            port: 7687,
            connection: ConnectionConfig::default(),
        }
    }
}

/// TCP server that accepts newline-delimited JSON requests.
///
/// Protocol: each request is a single JSON line, each response is a single JSON line.
/// Supports connection limits, request timeouts, idle timeouts, metrics, auth, and graceful shutdown.
pub struct AstraeaServer {
    config: ServerConfig,
    handler: Arc<RequestHandler>,
    auth: Arc<AuthManager>,
    metrics: Arc<ServerMetrics>,
    connection_manager: Arc<ConnectionManager>,
}

impl AstraeaServer {
    pub fn new(config: ServerConfig, handler: RequestHandler) -> Self {
        let connection_manager =
            Arc::new(ConnectionManager::new(config.connection.clone()));
        Self {
            config,
            handler: Arc::new(handler),
            auth: Arc::new(AuthManager::disabled()),
            metrics: Arc::new(ServerMetrics::new()),
            connection_manager,
        }
    }

    /// Create a server with authentication enabled.
    pub fn with_auth(mut self, auth: AuthManager) -> Self {
        self.auth = Arc::new(auth);
        self
    }

    /// Get a reference to the metrics collector.
    pub fn metrics(&self) -> &Arc<ServerMetrics> {
        &self.metrics
    }

    /// Get a reference to the connection manager (for external shutdown).
    pub fn connection_manager(&self) -> &Arc<ConnectionManager> {
        &self.connection_manager
    }

    /// Run the server, accepting connections until shutdown is initiated.
    pub async fn run(&self) -> std::io::Result<()> {
        let addr = format!("{}:{}", self.config.bind_address, self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        info!("AstraeaDB server listening on {}", addr);

        loop {
            // Check for shutdown.
            if self.connection_manager.is_shutting_down() {
                info!("Server shutting down, stopping accept loop");
                break;
            }

            // Accept with a timeout so we can check shutdown periodically.
            let accept_result =
                tokio::time::timeout(std::time::Duration::from_secs(1), listener.accept()).await;

            let (stream, peer_addr) = match accept_result {
                Ok(Ok(conn)) => conn,
                Ok(Err(e)) => {
                    error!("Accept error: {}", e);
                    continue;
                }
                Err(_) => continue, // timeout, loop to check shutdown
            };

            // Check connection limits.
            let guard = match self.connection_manager.try_accept() {
                Some(g) => g,
                None => {
                    warn!("Connection limit reached, rejecting {}", peer_addr);
                    // Send rejection and close.
                    let mut stream = stream;
                    let msg = r#"{"status":"error","message":"server connection limit reached"}"#;
                    let _ = tokio::io::AsyncWriteExt::write_all(
                        &mut stream,
                        format!("{}\n", msg).as_bytes(),
                    )
                    .await;
                    continue;
                }
            };

            self.metrics.connection_opened();
            info!("New connection from {}", peer_addr);

            let handler = Arc::clone(&self.handler);
            let auth = Arc::clone(&self.auth);
            let metrics = Arc::clone(&self.metrics);
            let idle_timeout = self.connection_manager.idle_timeout();
            let request_timeout = self.connection_manager.request_timeout();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(
                    stream,
                    handler,
                    auth,
                    metrics.clone(),
                    idle_timeout,
                    request_timeout,
                )
                .await
                {
                    error!("Connection error from {}: {}", peer_addr, e);
                }
                metrics.connection_closed();
                info!("Connection closed: {}", peer_addr);
                drop(guard); // explicitly release the connection slot
            });
        }

        // Graceful shutdown: wait for in-flight connections.
        info!("Waiting for in-flight connections to drain...");
        self.connection_manager.wait_for_drain().await;
        info!("All connections drained. Server stopped.");

        Ok(())
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    handler: Arc<RequestHandler>,
    auth: Arc<AuthManager>,
    metrics: Arc<ServerMetrics>,
    idle_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
) -> std::io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();

        // Apply idle timeout on reading.
        let read_result = tokio::time::timeout(idle_timeout, reader.read_line(&mut line)).await;

        let bytes_read = match read_result {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                // Idle timeout expired.
                let msg = r#"{"status":"error","message":"idle timeout"}"#;
                let _ = writer.write_all(format!("{}\n", msg).as_bytes()).await;
                return Ok(());
            }
        };

        if bytes_read == 0 {
            break; // client disconnected
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let start = Instant::now();

        // Parse the request.
        let request = match serde_json::from_str::<Request>(trimmed) {
            Ok(req) => req,
            Err(e) => {
                let response = Response::error(format!("invalid request: {e}"));
                let mut response_json = serde_json::to_string(&response).unwrap_or_else(|_| {
                    r#"{"status":"error","message":"serialization failed"}"#.into()
                });
                response_json.push('\n');
                writer.write_all(response_json.as_bytes()).await?;
                continue;
            }
        };

        let request_type = request_type_name(&request);

        // Authentication check.
        if auth.is_enabled() {
            // Extract auth_token from the raw JSON (simple approach).
            let auth_token = extract_auth_token(trimmed);
            match auth_token {
                Some(token) => {
                    if let Some(role) = auth.authenticate(token) {
                        if !AuthManager::authorize(role, request_type) {
                            auth.audit(token, role, request_type, false);
                            metrics.record_request(request_type);
                            metrics.record_error(request_type);
                            let response = Response::error(format!(
                                "access denied: role '{}' cannot perform '{}'",
                                role, request_type
                            ));
                            let mut rj = serde_json::to_string(&response).unwrap_or_default();
                            rj.push('\n');
                            writer.write_all(rj.as_bytes()).await?;
                            continue;
                        }
                        auth.audit(token, role, request_type, true);
                    } else {
                        metrics.record_request(request_type);
                        metrics.record_error(request_type);
                        let response = Response::error("invalid credentials");
                        let mut rj = serde_json::to_string(&response).unwrap_or_default();
                        rj.push('\n');
                        writer.write_all(rj.as_bytes()).await?;
                        continue;
                    }
                }
                None => {
                    metrics.record_request(request_type);
                    metrics.record_error(request_type);
                    let response = Response::error("authentication required: provide auth_token");
                    let mut rj = serde_json::to_string(&response).unwrap_or_default();
                    rj.push('\n');
                    writer.write_all(rj.as_bytes()).await?;
                    continue;
                }
            }
        }

        metrics.record_request(request_type);

        // Execute with request timeout.
        let handler_ref = Arc::clone(&handler);
        let response = match tokio::time::timeout(request_timeout, async move {
            handler_ref.handle(request)
        })
        .await
        {
            Ok(resp) => resp,
            Err(_) => {
                metrics.record_error(request_type);
                Response::error("request timeout")
            }
        };

        let duration = start.elapsed();
        metrics.record_duration(request_type, duration);

        if matches!(response, Response::Error { .. }) {
            metrics.record_error(request_type);
        }

        let mut response_json = serde_json::to_string(&response)
            .unwrap_or_else(|_| r#"{"status":"error","message":"serialization failed"}"#.into());
        response_json.push('\n');
        writer.write_all(response_json.as_bytes()).await?;
    }

    Ok(())
}

/// Extract the request type name for metrics/auth.
fn request_type_name(request: &Request) -> &'static str {
    match request {
        Request::CreateNode { .. } => "CreateNode",
        Request::CreateEdge { .. } => "CreateEdge",
        Request::GetNode { .. } => "GetNode",
        Request::GetEdge { .. } => "GetEdge",
        Request::UpdateNode { .. } => "UpdateNode",
        Request::UpdateEdge { .. } => "UpdateEdge",
        Request::DeleteNode { .. } => "DeleteNode",
        Request::DeleteEdge { .. } => "DeleteEdge",
        Request::Neighbors { .. } => "Neighbors",
        Request::NeighborsAt { .. } => "NeighborsAt",
        Request::Bfs { .. } => "Bfs",
        Request::BfsAt { .. } => "BfsAt",
        Request::ShortestPath { .. } => "ShortestPath",
        Request::ShortestPathAt { .. } => "ShortestPathAt",
        Request::VectorSearch { .. } => "VectorSearch",
        Request::HybridSearch { .. } => "HybridSearch",
        Request::SemanticNeighbors { .. } => "SemanticNeighbors",
        Request::SemanticWalk { .. } => "SemanticWalk",
        Request::Query { .. } => "Query",
        Request::ExtractSubgraph { .. } => "ExtractSubgraph",
        Request::GraphRag { .. } => "GraphRag",
        Request::Ping => "Ping",
    }
}

/// Extract auth_token from a raw JSON request string.
/// Looks for "auth_token":"<value>" in the JSON.
fn extract_auth_token(json: &str) -> Option<&str> {
    let marker = "\"auth_token\":\"";
    if let Some(start) = json.find(marker) {
        let rest = &json[start + marker.len()..];
        if let Some(end) = rest.find('"') {
            return Some(&rest[..end]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.bind_address, "127.0.0.1");
        assert_eq!(config.port, 7687);
        assert_eq!(config.connection.max_connections, 1024);
    }

    #[test]
    fn request_type_name_matches() {
        let req = Request::Ping;
        assert_eq!(request_type_name(&req), "Ping");

        let req = Request::CreateNode {
            labels: vec![],
            properties: serde_json::json!({}),
            embedding: None,
        };
        assert_eq!(request_type_name(&req), "CreateNode");
    }

    #[test]
    fn extract_auth_token_works() {
        let json = r#"{"type":"Ping","auth_token":"my-secret-key"}"#;
        assert_eq!(extract_auth_token(json), Some("my-secret-key"));
    }

    #[test]
    fn extract_auth_token_missing() {
        let json = r#"{"type":"Ping"}"#;
        assert_eq!(extract_auth_token(json), None);
    }

    #[test]
    fn temporal_request_types() {
        let req = Request::NeighborsAt {
            id: 1,
            direction: "outgoing".into(),
            timestamp: 100,
            edge_type: None,
        };
        assert_eq!(request_type_name(&req), "NeighborsAt");

        let req = Request::BfsAt {
            start: 1,
            max_depth: 3,
            timestamp: 100,
        };
        assert_eq!(request_type_name(&req), "BfsAt");

        let req = Request::ShortestPathAt {
            from: 1,
            to: 2,
            timestamp: 100,
            weighted: false,
        };
        assert_eq!(request_type_name(&req), "ShortestPathAt");
    }
}
