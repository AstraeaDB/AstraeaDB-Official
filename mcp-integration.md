# MCP Integration Plan for AstraeaDB

## Overview

This document describes the implementation plan for adding Model Context Protocol (MCP) support to AstraeaDB. MCP is a JSON-RPC 2.0 based protocol that allows LLM clients (Claude Desktop, Claude Code, Cursor, VS Code Copilot, etc.) to discover and invoke database capabilities as **tools**, read graph data as **resources**, and use predefined **prompt templates** — all through a standardized interface.

An `astraea-mcp` crate will expose AstraeaDB's 32 protocol operations, RAG pipeline, and graph algorithms to any MCP-compatible client with zero custom glue code.

---

## Architecture

### Operating Modes

The MCP server supports two modes:

**Proxy mode (default)** — connects to a running AstraeaDB instance over TCP:
```
┌──────────────┐    JSON-RPC/stdio     ┌──────────────┐    NDJSON/TCP    ┌──────────────┐
│  LLM Client  │ ◄──────────────────► │  astraea-mcp │ ◄──────────────► │  AstraeaDB   │
│ (Claude, etc)│                       │  (MCP server)│                  │  (TCP server) │
└──────────────┘                       └──────────────┘                  └──────────────┘
```
- Recommended for production and multi-user setups
- Reuses existing auth, TLS, connection management
- MCP server is a lightweight stateless bridge

**Embedded mode** — opens the storage layer directly (no separate server process):
```
┌──────────────┐    JSON-RPC/stdio     ┌──────────────────────────────┐
│  LLM Client  │ ◄──────────────────► │  astraea-mcp (embedded)      │
│ (Claude, etc)│                       │  ┌─────────┐  ┌───────────┐ │
└──────────────┘                       │  │  Graph   │  │  Storage  │ │
                                       │  └─────────┘  └───────────┘ │
                                       └──────────────────────────────┘
```
- Best for single-user local development
- No network hop; direct `GraphOps` access
- Opens the data directory exclusively (no concurrent TCP server on the same dir)

### Transport Layers

| Transport | Use Case | Protocol |
|-----------|----------|----------|
| **stdio** | Local subprocess (Claude Desktop, Claude Code, Cursor) | JSON-RPC 2.0 over stdin/stdout |
| **SSE** | Remote / networked clients | HTTP POST for requests, Server-Sent Events for responses |

stdio is the primary transport. SSE is optional and can be deferred to a later phase.

---

## Crate Structure

```
crates/astraea-mcp/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API: McpServer, McpConfig
│   ├── server.rs           # JSON-RPC 2.0 dispatch loop
│   ├── transport/
│   │   ├── mod.rs
│   │   ├── stdio.rs        # stdin/stdout transport
│   │   └── sse.rs          # HTTP/SSE transport (Phase 2)
│   ├── tools/
│   │   ├── mod.rs           # Tool registry and dispatch
│   │   ├── crud.rs          # create_node, get_node, update_node, delete_node, etc.
│   │   ├── traversal.rs     # neighbors, bfs, dfs, shortest_path
│   │   ├── search.rs        # vector_search, hybrid_search, semantic_neighbors, find_by_label
│   │   ├── algorithms.rs    # pagerank, louvain, connected_components, centrality
│   │   ├── temporal.rs      # neighbors_at, bfs_at, dfs_at, shortest_path_at
│   │   ├── rag.rs           # graph_rag, extract_subgraph
│   │   └── admin.rs         # graph_stats, ping, query (GQL)
│   ├── resources.rs         # MCP resource definitions and handlers
│   ├── prompts.rs           # MCP prompt templates
│   ├── client.rs            # TCP client for proxy mode (reuse from astraea-cli)
│   └── errors.rs            # MCP-specific error codes
```

### Dependencies

```toml
[package]
name = "astraea-mcp"
version = "0.1.0"
edition = "2024"

[dependencies]
astraea-core = { path = "../astraea-core" }
astraea-graph = { path = "../astraea-graph" }
astraea-storage = { path = "../astraea-storage" }
astraea-query = { path = "../astraea-query" }
astraea-vector = { path = "../astraea-vector" }
astraea-rag = { path = "../astraea-rag" }
astraea-algorithms = { path = "../astraea-algorithms" }
astraea-server = { path = "../astraea-server" }   # For protocol types + TCP client

serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
clap = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

No external MCP SDK is required. The MCP protocol is straightforward JSON-RPC 2.0 — implementing it directly keeps dependencies minimal and avoids coupling to an immature SDK ecosystem.

---

## MCP Protocol Implementation

### JSON-RPC 2.0 Message Types

```rust
// Incoming from client
#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,           // Must be "2.0"
    id: Option<Value>,         // Request ID (null for notifications)
    method: String,            // MCP method name
    params: Option<Value>,     // Method parameters
}

// Outgoing to client
#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,           // "2.0"
    id: Value,                 // Matches request ID
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,     // Success payload
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>, // Error payload
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}
```

### MCP Lifecycle Methods

| Method | Direction | Purpose |
|--------|-----------|---------|
| `initialize` | client → server | Negotiate capabilities, exchange protocol version |
| `initialized` | client → server (notification) | Client confirms initialization complete |
| `ping` | either direction | Keep-alive |
| `notifications/cancelled` | client → server | Cancel in-progress request |

**Initialize handshake:**

```json
// Client sends:
{
  "jsonrpc": "2.0", "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2025-03-26",
    "capabilities": { "roots": { "listChanged": true } },
    "clientInfo": { "name": "claude-desktop", "version": "1.0" }
  }
}

// Server responds:
{
  "jsonrpc": "2.0", "id": 1,
  "result": {
    "protocolVersion": "2025-03-26",
    "capabilities": {
      "tools": { "listChanged": false },
      "resources": { "subscribe": false, "listChanged": false },
      "prompts": { "listChanged": false }
    },
    "serverInfo": { "name": "astraea-mcp", "version": "0.1.0" }
  }
}
```

---

## Tool Definitions

### Core Tools (22 tools)

Each tool maps to an existing `Request` variant. Inputs use JSON Schema, matching the existing protocol field types exactly.

#### CRUD Tools

**`create_node`**
```json
{
  "name": "create_node",
  "description": "Create a new node in the graph with labels, properties, and an optional embedding vector.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "labels": { "type": "array", "items": { "type": "string" }, "description": "Node labels (e.g., [\"Person\", \"Employee\"])" },
      "properties": { "type": "object", "description": "Arbitrary JSON properties", "default": {} },
      "embedding": { "type": "array", "items": { "type": "number" }, "description": "Optional embedding vector for semantic search" }
    },
    "required": ["labels"]
  }
}
```

**`create_edge`**
```json
{
  "name": "create_edge",
  "description": "Create a directed edge between two nodes with a type, properties, weight, and optional temporal validity window.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "source": { "type": "integer", "description": "Source node ID" },
      "target": { "type": "integer", "description": "Target node ID" },
      "edge_type": { "type": "string", "description": "Relationship type (e.g., \"KNOWS\", \"WORKS_AT\")" },
      "properties": { "type": "object", "default": {} },
      "weight": { "type": "number", "default": 1.0 },
      "valid_from": { "type": "integer", "description": "Start of validity (epoch ms), null for unbounded" },
      "valid_to": { "type": "integer", "description": "End of validity (epoch ms), null for unbounded" }
    },
    "required": ["source", "target", "edge_type"]
  }
}
```

**`get_node`** — `{ id: u64 }` → Node JSON
**`get_edge`** — `{ id: u64 }` → Edge JSON
**`update_node`** — `{ id: u64, properties: Object }` → merge properties
**`update_edge`** — `{ id: u64, properties: Object }` → merge properties
**`delete_node`** — `{ id: u64 }` → deletes node + connected edges
**`delete_edge`** — `{ id: u64 }` → deletes edge

#### Traversal Tools

**`neighbors`** — `{ id: u64, direction?: "outgoing"|"incoming"|"both", edge_type?: string }`
**`bfs`** — `{ start: u64, max_depth?: u64 (default 3) }`
**`dfs`** — `{ start: u64, max_depth?: u64 (default 3) }`
**`shortest_path`** — `{ from: u64, to: u64, weighted?: bool (default false) }`

#### Search Tools

**`vector_search`** — `{ query: [f32], k?: u64 (default 10) }`
**`hybrid_search`** — `{ anchor: u64, query: [f32], max_hops?: u64, k?: u64, alpha?: f32 }`
**`find_by_label`** — `{ label: string }`

#### Algorithm Tools

**`run_pagerank`** — `{ nodes?: [u64], damping?: f64, max_iterations?: u64, tolerance?: f64 }`
**`run_louvain`** — `{ nodes?: [u64] }`
**`run_connected_components`** — `{ nodes?: [u64], strong?: bool }`
**`run_centrality`** — `{ nodes?: [u64], metric: "degree"|"betweenness", direction?: string }`

#### RAG Tools

**`graph_rag`**
```json
{
  "name": "graph_rag",
  "description": "Answer a natural language question using graph-augmented retrieval. Extracts a subgraph around the most relevant node, linearizes it as context, and queries the configured LLM.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "question": { "type": "string", "description": "The natural language question to answer" },
      "question_embedding": { "type": "array", "items": { "type": "number" }, "description": "Embedding of the question for vector search (optional if anchor provided)" },
      "anchor": { "type": "integer", "description": "Anchor node ID (skips vector search if provided)" },
      "hops": { "type": "integer", "default": 2 },
      "max_nodes": { "type": "integer", "default": 50 },
      "format": { "type": "string", "enum": ["prose", "structured", "triples", "json"], "default": "structured" }
    },
    "required": ["question"]
  }
}
```

**`extract_subgraph`** — `{ center: u64, hops?: u64, max_nodes?: u64, format?: string }`

#### Admin Tools

**`query`** — `{ gql: string }` — Execute a raw GQL query
**`graph_stats`** — `{}` — Return node/edge counts, labels, etc.
**`ping`** — `{}` — Server health check

#### Temporal Tools

**`neighbors_at`** — `{ id: u64, direction?: string, timestamp: i64, edge_type?: string }`
**`bfs_at`** — `{ start: u64, max_depth?: u64, timestamp: i64 }`
**`dfs_at`** — `{ start: u64, max_depth?: u64, timestamp: i64 }`
**`shortest_path_at`** — `{ from: u64, to: u64, timestamp: i64, weighted?: bool }`

### Tool Dispatch

```rust
async fn handle_tool_call(&self, name: &str, args: Value) -> Result<Value> {
    match name {
        "create_node" => self.tools.create_node(args).await,
        "create_edge" => self.tools.create_edge(args).await,
        "neighbors" => self.tools.neighbors(args).await,
        "graph_rag" => self.tools.graph_rag(args).await,
        "query" => self.tools.query(args).await,
        // ... etc
        _ => Err(McpError::ToolNotFound(name.to_string())),
    }
}
```

In proxy mode, each handler serializes the args into the corresponding `Request` variant, sends it over TCP, and deserializes the `Response`.

In embedded mode, each handler calls the `GraphOps` / `VectorIndex` / `Executor` methods directly.

---

## Resource Definitions

MCP resources provide read-only data that clients can fetch and include in LLM context.

### Static Resources

| URI | Description | MIME Type |
|-----|-------------|-----------|
| `astraea://stats` | Graph statistics (node/edge counts, labels) | `application/json` |

### Resource Templates (Dynamic)

| URI Template | Description | MIME Type |
|--------------|-------------|-----------|
| `astraea://node/{id}` | Full node data (labels, properties, embedding metadata) | `application/json` |
| `astraea://edge/{id}` | Full edge data (type, properties, weight, validity) | `application/json` |
| `astraea://subgraph/{nodeId}?hops={hops}&max={max}&format={format}` | Linearized subgraph around a node | `text/plain` |
| `astraea://label/{label}` | All nodes with a given label | `application/json` |

### Resource Handler

```rust
async fn handle_resource_read(&self, uri: &str) -> Result<ResourceContent> {
    let parsed = parse_astraea_uri(uri)?;
    match parsed {
        AstraeaUri::Stats => {
            let stats = self.call(Request::GraphStats).await?;
            Ok(ResourceContent::text(uri, stats))
        }
        AstraeaUri::Node(id) => {
            let node = self.call(Request::GetNode { id }).await?;
            Ok(ResourceContent::text(uri, node))
        }
        AstraeaUri::Subgraph { node_id, hops, max_nodes, format } => {
            let sg = self.call(Request::ExtractSubgraph {
                center: node_id, hops, max_nodes, format
            }).await?;
            Ok(ResourceContent::text(uri, sg))
        }
        // ...
    }
}
```

---

## Prompt Templates

MCP prompts are reusable prompt templates that clients can discover and fill in.

| Prompt Name | Arguments | Description |
|-------------|-----------|-------------|
| `analyze-node` | `node_id: u64` | "Analyze node {id}: describe its properties, connections, and role in the graph." |
| `explain-path` | `from: u64, to: u64` | "Find and explain the shortest path between nodes {from} and {to}." |
| `explore-community` | `node_id: u64` | "Run community detection and describe the community containing node {id}." |
| `summarize-graph` | _(none)_ | "Provide a high-level summary of the graph: size, key labels, structure." |
| `temporal-diff` | `node_id: u64, t1: i64, t2: i64` | "Compare the neighborhood of node {id} at timestamps {t1} and {t2}." |
| `rag-query` | `question: string` | "Answer this question using graph-augmented retrieval: {question}" |

These are templates only — the LLM client fills them in and may invoke the corresponding tools to gather data.

---

## CLI Integration

Add an `mcp` subcommand to `astraea-cli`:

```rust
#[derive(Subcommand)]
enum Commands {
    // ... existing: Serve, Import, Export, Shell, Status
    /// Start an MCP server for LLM tool integration
    Mcp(McpArgs),
}

#[derive(Args)]
struct McpArgs {
    /// Transport type
    #[arg(long, default_value = "stdio")]
    transport: String,       // "stdio" or "sse"

    /// Server address for proxy mode
    #[arg(long, default_value = "127.0.0.1:7687")]
    address: String,

    /// Use embedded mode (open storage directly, no TCP connection)
    #[arg(long)]
    embedded: bool,

    /// Data directory for embedded mode
    #[arg(long, default_value = "data")]
    data_dir: PathBuf,

    /// Port for SSE transport
    #[arg(long, default_value = "8080")]
    sse_port: u16,

    /// Auth token for proxy mode
    #[arg(long)]
    auth_token: Option<String>,

    /// Log level (stderr, never stdout — stdout is the MCP transport)
    #[arg(long, default_value = "warn")]
    log_level: String,
}
```

Usage:

```bash
# Proxy mode over stdio (most common — for Claude Desktop / Claude Code)
astraeadb mcp

# Proxy mode connecting to a remote server
astraeadb mcp --address 10.0.1.5:7687 --auth-token mytoken

# Embedded mode (no separate server needed)
astraeadb mcp --embedded --data-dir ./my-graph-data

# SSE transport for remote clients (Phase 2)
astraeadb mcp --transport sse --sse-port 8080
```

### Client Configuration Examples

**Claude Desktop (`claude_desktop_config.json`):**
```json
{
  "mcpServers": {
    "astraeadb": {
      "command": "astraeadb",
      "args": ["mcp"],
      "env": {
        "ASTRAEA_HOST": "127.0.0.1",
        "ASTRAEA_PORT": "7687"
      }
    }
  }
}
```

**Claude Code (`.claude/settings.json`):**
```json
{
  "mcpServers": {
    "astraeadb": {
      "command": "astraeadb",
      "args": ["mcp", "--address", "127.0.0.1:7687"]
    }
  }
}
```

**Cursor (`.cursor/mcp.json`):**
```json
{
  "mcpServers": {
    "astraeadb": {
      "command": "astraeadb",
      "args": ["mcp"]
    }
  }
}
```

---

## Implementation Phases

### Phase 1: Core MCP Server with stdio Transport

**Goal:** Working MCP server that exposes all tools over stdio in proxy mode.

**Steps:**

1. **Create `astraea-mcp` crate**
   - Add to workspace members and `[workspace.dependencies]` in root `Cargo.toml`
   - Set up module structure as described above

2. **Implement JSON-RPC 2.0 message parsing** (`server.rs`)
   - `JsonRpcRequest` / `JsonRpcResponse` / `JsonRpcError` types
   - Read newline-delimited JSON from stdin, write to stdout
   - All logging to stderr (stdout is the transport)

3. **Implement MCP lifecycle** (`server.rs`)
   - `initialize` → return server capabilities (tools, resources, prompts)
   - `initialized` notification → mark session as active
   - `ping` / `pong`
   - `tools/list` → return all tool definitions with JSON Schemas
   - `tools/call` → dispatch to tool handler, return result

4. **Implement stdio transport** (`transport/stdio.rs`)
   - Async reader on stdin (tokio `BufReader`)
   - Async writer on stdout
   - Newline-delimited JSON framing

5. **Implement TCP client for proxy mode** (`client.rs`)
   - Reuse connection logic from `astraea-cli` shell command
   - Send `Request` JSON, receive `Response` JSON
   - Connection pooling / reconnection

6. **Implement all tool handlers** (`tools/*.rs`)
   - Each tool: validate input args → build `Request` → send to AstraeaDB → format response
   - Return MCP `CallToolResult` with `content: [{ type: "text", text: "..." }]`
   - Map `Response::Error` to MCP error responses with appropriate codes

7. **Add `mcp` subcommand to CLI** (`astraea-cli`)
   - Parse `McpArgs`
   - Instantiate `McpServer` with proxy client
   - Run the server loop

8. **Tests**
   - Unit tests: JSON-RPC parsing, tool input validation, response formatting
   - Integration tests: spawn MCP server process, send initialize + tool calls over stdio, verify responses
   - Use `MockProvider` for RAG tool tests

**Deliverables:**
- `astraeadb mcp` works as a subprocess for Claude Desktop / Claude Code
- All 22+ tools callable from any MCP client
- Comprehensive test coverage

### Phase 2: Resources, Prompts, and Embedded Mode

**Goal:** Full MCP feature set including resources, prompt templates, and embedded mode.

**Steps:**

1. **Implement resource handlers** (`resources.rs`)
   - `resources/list` → return static resources + resource templates
   - `resources/read` → parse URI, fetch data, return content
   - URI parser for `astraea://` scheme

2. **Implement prompt templates** (`prompts.rs`)
   - `prompts/list` → return all prompt definitions
   - `prompts/get` → fill in template arguments, return prompt messages

3. **Implement embedded mode**
   - Initialize `Graph` + `BufferPool` + `HnswIndex` + `Executor` directly
   - Implement same tool dispatch but calling `GraphOps` methods instead of TCP
   - Share tool handler logic between proxy and embedded via a trait:
     ```rust
     #[async_trait]
     trait McpBackend: Send + Sync {
         async fn execute(&self, request: Request) -> Result<Response>;
     }

     struct ProxyBackend { /* TCP connection */ }
     struct EmbeddedBackend { /* Graph + VectorIndex + Executor */ }
     ```

4. **Tests**
   - Resource read tests for each URI pattern
   - Prompt template rendering tests
   - Embedded mode integration tests with tempfile storage

### Phase 3: SSE Transport and Production Hardening

**Goal:** Remote access via SSE and production-grade reliability.

**Steps:**

1. **Implement SSE transport** (`transport/sse.rs`)
   - HTTP POST endpoint for client → server messages
   - SSE stream for server → client messages
   - Use `hyper` or `axum` for HTTP server
   - Session management with unique session IDs

2. **Auth integration**
   - Pass `auth_token` from MCP server config to AstraeaDB TCP requests
   - Optional: MCP-level API key validation for SSE transport

3. **Error handling hardening**
   - Graceful handling of malformed JSON-RPC messages
   - Timeout on tool calls (configurable, default 30s)
   - Connection retry with backoff for proxy mode
   - Proper MCP error codes:
     - `-32700` Parse error
     - `-32600` Invalid request
     - `-32601` Method not found
     - `-32602` Invalid params
     - `-32603` Internal error

4. **Logging and observability**
   - Structured logging to stderr (tracing)
   - Optional request/response logging at debug level
   - Metrics: tool call counts, latencies, errors

5. **Documentation**
   - Update project README with MCP section
   - Example workflows: "Ask Claude to analyze your graph"
   - Configuration reference for all supported clients

---

## Design Decisions

### Why not use an MCP SDK?

The Rust MCP SDK ecosystem is young. The protocol itself is simple (JSON-RPC 2.0 with a handful of methods), and implementing it directly:
- Avoids a dependency on a rapidly-changing crate
- Gives full control over transport and error handling
- Keeps the implementation transparent and debuggable

If the official `mcp-rust-sdk` stabilizes and provides clear value (e.g., built-in SSE transport, automatic schema generation), it can be adopted later as an internal refactor without changing the public API.

### Why proxy mode as default?

- AstraeaDB is designed as a server — auth, TLS, connection management, WAL, and buffer pool are all managed there
- Proxy mode avoids duplicating storage initialization and lock management
- Multiple MCP server instances can connect to the same AstraeaDB server
- Embedded mode is offered for simplicity in single-user local scenarios

### Tool granularity

Tools are mapped 1:1 to existing protocol operations rather than aggregated into coarser actions. This gives the LLM maximum flexibility to compose operations. For example, an LLM can:
1. Call `find_by_label` to discover nodes
2. Call `neighbors` to explore connections
3. Call `extract_subgraph` to get context
4. Call `graph_rag` for a synthesized answer

Coarse tools (e.g., "search and summarize") would limit the LLM's ability to adapt its strategy.

### Sensitive operations

`delete_node`, `delete_edge`, `create_node`, `create_edge`, and `update_*` are write operations. MCP clients typically require user confirmation for tool calls, which provides a natural safety gate. For additional protection:
- The auth layer in AstraeaDB can restrict write operations per token/role
- A future `--read-only` flag on the MCP server could filter out write tools from `tools/list`

---

## File Changes Summary

| File | Action | Description |
|------|--------|-------------|
| `Cargo.toml` (root) | Modify | Add `astraea-mcp` to workspace members |
| `crates/astraea-mcp/Cargo.toml` | Create | New crate manifest |
| `crates/astraea-mcp/src/lib.rs` | Create | Public API exports |
| `crates/astraea-mcp/src/server.rs` | Create | JSON-RPC 2.0 dispatch loop, MCP lifecycle |
| `crates/astraea-mcp/src/transport/mod.rs` | Create | Transport trait |
| `crates/astraea-mcp/src/transport/stdio.rs` | Create | stdio transport |
| `crates/astraea-mcp/src/transport/sse.rs` | Create | SSE transport (Phase 3) |
| `crates/astraea-mcp/src/tools/mod.rs` | Create | Tool registry, `tools/list` and `tools/call` |
| `crates/astraea-mcp/src/tools/crud.rs` | Create | 8 CRUD tool handlers |
| `crates/astraea-mcp/src/tools/traversal.rs` | Create | 4 traversal tool handlers |
| `crates/astraea-mcp/src/tools/search.rs` | Create | 3 search tool handlers |
| `crates/astraea-mcp/src/tools/algorithms.rs` | Create | 4 algorithm tool handlers |
| `crates/astraea-mcp/src/tools/temporal.rs` | Create | 4 temporal tool handlers |
| `crates/astraea-mcp/src/tools/rag.rs` | Create | 2 RAG tool handlers |
| `crates/astraea-mcp/src/tools/admin.rs` | Create | 3 admin tool handlers |
| `crates/astraea-mcp/src/resources.rs` | Create | Resource definitions and handlers (Phase 2) |
| `crates/astraea-mcp/src/prompts.rs` | Create | Prompt templates (Phase 2) |
| `crates/astraea-mcp/src/client.rs` | Create | TCP client for proxy mode |
| `crates/astraea-mcp/src/errors.rs` | Create | MCP error types and JSON-RPC error codes |
| `crates/astraea-cli/src/main.rs` | Modify | Add `Mcp` subcommand variant and handler |
| `crates/astraea-cli/Cargo.toml` | Modify | Add `astraea-mcp` dependency |

---

## Testing Strategy

### Unit Tests (`astraea-mcp`)

- **JSON-RPC parsing**: valid requests, malformed JSON, missing fields, batch requests
- **Tool input validation**: missing required fields, wrong types, out-of-range values
- **Response formatting**: MCP `CallToolResult` structure, error code mapping
- **URI parsing**: all resource URI patterns, invalid URIs
- **Prompt rendering**: template substitution, missing arguments

### Integration Tests

- **Stdio round-trip**: spawn `astraeadb mcp --embedded` as a subprocess, send JSON-RPC over stdin, read responses from stdout
- **Full tool lifecycle**: `initialize` → `tools/list` → `tools/call` for each tool → verify results
- **Proxy mode**: start AstraeaDB TCP server, then MCP server in proxy mode, verify end-to-end
- **Error paths**: call tools with invalid arguments, call nonexistent tools, send malformed JSON

### Compatibility Tests

- Validate `tools/list` output against MCP JSON Schema spec
- Validate `initialize` response against MCP spec
- Test with `mcp-cli` inspector tool if available

---

## Success Criteria

- [ ] `astraeadb mcp` starts and completes MCP handshake with Claude Desktop
- [ ] All 22+ tools appear in Claude Desktop's tool list
- [ ] LLM can create nodes, query the graph, run algorithms, and get RAG answers through MCP
- [ ] Resources provide graph data for context augmentation
- [ ] Embedded mode works without a running AstraeaDB server
- [ ] All tests pass; no regressions in existing 408 Rust + 23 Python tests
