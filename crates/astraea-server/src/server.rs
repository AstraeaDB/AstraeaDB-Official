use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, warn};

use crate::auth::AuthManager;
use crate::connection::{ConnectionConfig, ConnectionManager};
use crate::handler::RequestHandler;
use crate::metrics::ServerMetrics;
use crate::protocol::{Request, Response};
use crate::tls::{extract_client_cn, TlsConfig};

/// Configuration for the AstraeaDB server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
    pub connection: ConnectionConfig,
    /// Optional TLS configuration. When set, enables TLS/mTLS.
    pub tls: Option<TlsConfig>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".into(),
            port: 7687,
            connection: ConnectionConfig::default(),
            tls: None,
        }
    }
}

/// TCP server that accepts newline-delimited JSON requests.
///
/// Protocol: each request is a single JSON line, each response is a single JSON line.
/// Supports connection limits, request timeouts, idle timeouts, metrics, auth, TLS/mTLS, and graceful shutdown.
pub struct AstraeaServer {
    config: ServerConfig,
    handler: Arc<RequestHandler>,
    auth: Arc<AuthManager>,
    metrics: Arc<ServerMetrics>,
    connection_manager: Arc<ConnectionManager>,
    tls_acceptor: Option<TlsAcceptor>,
}

impl AstraeaServer {
    /// Create a new server. If TLS is configured, this will load the certificates.
    ///
    /// # Errors
    /// Returns an error if TLS is configured but certificates cannot be loaded.
    pub fn new(config: ServerConfig, handler: RequestHandler) -> Result<Self, crate::tls::TlsError> {
        let connection_manager = Arc::new(ConnectionManager::new(config.connection.clone()));

        // Build TLS acceptor if TLS is configured
        let tls_acceptor = if let Some(ref tls_config) = config.tls {
            info!("TLS enabled, loading certificates...");
            let acceptor = tls_config.build_acceptor()?;
            info!(
                "TLS configured: cert={}, require_client_cert={}",
                tls_config.cert_path.display(),
                tls_config.require_client_cert
            );
            Some(acceptor)
        } else {
            None
        };

        Ok(Self {
            config,
            handler: Arc::new(handler),
            auth: Arc::new(AuthManager::disabled()),
            metrics: Arc::new(ServerMetrics::new()),
            connection_manager,
            tls_acceptor,
        })
    }

    /// Create a new server without TLS validation at construction time.
    /// Use this for testing or when you want to defer TLS setup.
    pub fn new_without_tls(config: ServerConfig, handler: RequestHandler) -> Self {
        let connection_manager = Arc::new(ConnectionManager::new(config.connection.clone()));
        Self {
            config,
            handler: Arc::new(handler),
            auth: Arc::new(AuthManager::disabled()),
            metrics: Arc::new(ServerMetrics::new()),
            connection_manager,
            tls_acceptor: None,
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

        if self.tls_acceptor.is_some() {
            info!("AstraeaDB server listening on {} (TLS enabled)", addr);
        } else {
            info!("AstraeaDB server listening on {} (plaintext)", addr);
        }

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
            let tls_acceptor = self.tls_acceptor.clone();

            tokio::spawn(async move {
                let result = if let Some(acceptor) = tls_acceptor {
                    // TLS connection
                    match acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            // Extract client certificate CN if available
                            let client_cn = tls_stream
                                .get_ref()
                                .1
                                .peer_certificates()
                                .and_then(|certs| extract_client_cn(certs));

                            if let Some(ref cn) = client_cn {
                                debug!("TLS client authenticated: CN={}", cn);
                            }

                            handle_connection(
                                tls_stream,
                                handler,
                                auth,
                                metrics.clone(),
                                idle_timeout,
                                request_timeout,
                                client_cn,
                            )
                            .await
                        }
                        Err(e) => {
                            error!("TLS handshake error from {}: {}", peer_addr, e);
                            Err(std::io::Error::new(std::io::ErrorKind::Other, e))
                        }
                    }
                } else {
                    // Plain TCP connection
                    handle_connection(
                        stream,
                        handler,
                        auth,
                        metrics.clone(),
                        idle_timeout,
                        request_timeout,
                        None,
                    )
                    .await
                };

                if let Err(e) = result {
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

    /// Check if TLS is enabled for this server.
    pub fn is_tls_enabled(&self) -> bool {
        self.tls_acceptor.is_some()
    }
}

async fn handle_connection<S>(
    stream: S,
    handler: Arc<RequestHandler>,
    auth: Arc<AuthManager>,
    metrics: Arc<ServerMetrics>,
    idle_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
    _client_cn: Option<String>,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
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
        Request::Dfs { .. } => "Dfs",
        Request::DfsAt { .. } => "DfsAt",
        Request::FindByLabel { .. } => "FindByLabel",
        Request::DeleteByLabel { .. } => "DeleteByLabel",
        Request::RunPageRank { .. } => "RunPageRank",
        Request::RunLouvain { .. } => "RunLouvain",
        Request::RunConnectedComponents { .. } => "RunConnectedComponents",
        Request::RunDegreeCentrality { .. } => "RunDegreeCentrality",
        Request::RunBetweennessCentrality { .. } => "RunBetweennessCentrality",
        Request::GraphStats => "GraphStats",
        Request::GetSubgraph { .. } => "GetSubgraph",
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
