use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::handler::RequestHandler;
use crate::protocol::{Request, Response};

/// Configuration for the AstraeaDB server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".into(),
            port: 7687,
        }
    }
}

/// TCP server that accepts newline-delimited JSON requests.
///
/// Protocol: each request is a single JSON line, each response is a single JSON line.
/// This is simple, debuggable with telnet/netcat, and sufficient for the foundation phase.
pub struct AstraeaServer {
    config: ServerConfig,
    handler: Arc<RequestHandler>,
}

impl AstraeaServer {
    pub fn new(config: ServerConfig, handler: RequestHandler) -> Self {
        Self {
            config,
            handler: Arc::new(handler),
        }
    }

    /// Run the server, accepting connections until the process is terminated.
    pub async fn run(&self) -> std::io::Result<()> {
        let addr = format!("{}:{}", self.config.bind_address, self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        info!("AstraeaDB server listening on {}", addr);

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            info!("New connection from {}", peer_addr);

            let handler = Arc::clone(&self.handler);
            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, handler).await {
                    error!("Connection error from {}: {}", peer_addr, e);
                }
                info!("Connection closed: {}", peer_addr);
            });
        }
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    handler: Arc<RequestHandler>,
) -> std::io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // client disconnected
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Request>(trimmed) {
            Ok(request) => handler.handle(request),
            Err(e) => Response::error(format!("invalid request: {e}")),
        };

        let mut response_json = serde_json::to_string(&response)
            .unwrap_or_else(|_| r#"{"status":"error","message":"serialization failed"}"#.into());
        response_json.push('\n');
        writer.write_all(response_json.as_bytes()).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.bind_address, "127.0.0.1");
        assert_eq!(config.port, 7687);
    }
}
