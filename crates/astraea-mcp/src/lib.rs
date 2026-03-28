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
