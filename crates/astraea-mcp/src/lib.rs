//! Model Context Protocol server fronting a running `astraea-server`.
//!
//! `McpServer` is a JSON-RPC 2.0 server speaking MCP protocol version
//! `2025-03-26` over a `Transport` (currently only `StdioTransport`).
//! `ToolRegistry` exposes 28 compile-time tools across `crud`,
//! `traversal`, `search`, `algorithms`, `temporal`, `rag`, and `admin`,
//! all dispatched through a `ProxyClient` that opens a fresh TCP
//! connection per call to an `astraea-server` instance — this crate
//! holds no graph state of its own.
//!
//! Invariants: stdout is reserved for JSON-RPC frames; any logging must
//! go to stderr or it will corrupt the stream. Adding a tool requires
//! edits in three places (`definitions()`, the handler fn, and the
//! `ToolRegistry::call` match) — the dispatch is not compile-checked,
//! so a missing arm returns `McpError::ToolNotFound` at runtime.

pub mod client;
pub mod errors;
pub mod prompts;
pub mod resources;
pub mod server;
pub mod tools;
pub mod transport;

pub use client::ProxyClient;
pub use errors::McpError;
pub use server::{McpConfig, McpServer};
