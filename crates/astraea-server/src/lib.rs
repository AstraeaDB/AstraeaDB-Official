//! TCP and gRPC front-ends for AstraeaDB.
//!
//! `AstraeaServer` runs a newline-delimited JSON request loop over TCP
//! (one `protocol::Request` per line, one `protocol::Response` per
//! line); the `grpc::AstraeaGrpcService` provides a parallel tonic
//! adapter. Both transports are thin and must route through a single
//! `RequestHandler::handle(Request) -> Response` so feature parity is
//! preserved. Cross-cutting concerns live in `AuthManager`,
//! `ConnectionManager`, `ServerMetrics`, and `TlsConfig`.
//!
//! Invariants: `request_type_name` in `server.rs` must have an arm for
//! every `Request` variant — it drives metrics and RBAC. Vector-aware
//! requests (`VectorSearch`, `HybridSearch`, `SemanticNeighbors`,
//! `SemanticWalk`, anchor-less `GraphRag`) require constructing the
//! server with `Some(vector_index)`; otherwise they return an error
//! rather than panic. `collect_graph_stats` and `resolve_node_ids(None,
//! ...)` probe IDs `1..10_000` sequentially and silently truncate.

pub mod auth;
pub mod connection;
pub mod grpc;
pub mod handler;
pub mod metrics;
pub mod protocol;
pub mod server;
pub mod tls;

pub use auth::{ApiKeyEntry, AuthManager, Role};
pub use connection::{ConnectionConfig, ConnectionManager};
pub use handler::RequestHandler;
pub use metrics::ServerMetrics;
pub use protocol::{Request, Response};
pub use server::{AstraeaServer, ServerConfig};
pub use tls::{TlsConfig, TlsError};
