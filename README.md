<p align="center">
  <img src="docs/assets/logo.png" alt="AstraeaDB Logo" width="200">
</p>

# AstraeaDB

A cloud-native, AI-first graph database written in Rust. AstraeaDB combines a **Vector-Property Graph** model with an **HNSW vector index**, enabling both structural graph traversals and semantic similarity search in a single system.

> **New to AstraeaDB?** Start with the [**Gentle Introduction**](https://astraeadb.github.io/AstraeaDB-Official/gentle-intro.html) — a comprehensive, beginner-friendly guide that takes you from graph database fundamentals through advanced features like vector search, GraphRAG, and GNN training, with examples in Python, R, Go, and Java.

## Architecture

```
                    ┌─────────────────────────────────┐
                    │         astraea-cli              │
                    │   serve | shell | import | export│
                    └──────────────┬──────────────────┘
                                   │
          ┌────────────────────────┼────────────────────────┐
          │                        │                        │
┌─────────▼──────────┐  ┌─────────▼──────────┐  ┌─────────▼──────────┐
│  astraea-server    │  │  astraea-flight    │  │  Client Libraries  │
│  JSON-TCP (7687)   │  │  Arrow Flight      │  │  Python, R, Go,    │
│  gRPC (7688)       │  │  do_get / do_put   │  │  Java — JSON +     │
│  Auth, Metrics     │  │                    │  │  gRPC + Flight     │
│  Connection Mgmt   │  │                    │  │                    │
└──────┬─────────────┘  └──────┬─────────────┘  └────────────────────┘
       │                       │
       └───────────┬───────────┘
                   │
    ┌──────────────┼──────────────────────────────┐
    │              │              │                │
┌───▼──────┐ ┌────▼────────┐ ┌───▼───────┐ ┌─────▼──────────┐
│astraea-  │ │ astraea-    │ │astraea-   │ │ astraea-       │
│  rag     │ │   query     │ │  gnn      │ │  algorithms    │
│Subgraph  │ │ GQL Parser  │ │Model,     │ │  PageRank,     │
│Linearize │ │ + Executor  │ │Backprop,  │ │  Louvain,      │
│LLM, RAG  │ │             │ │SpMM,      │ │  Components    │
│          │ │             │ │Temporal   │ │                │
└───┬──────┘ └────┬────────┘ └───┬───────┘ └─────┬──────────┘
    │              │              │                │
    └──────────────┼──────────────┴────────────────┘
                   │
       ┌───────────┴───────────┐
       │                       │
┌──────▼───────────┐  ┌───────▼──────────┐
│  astraea-graph   │  │  astraea-vector  │
│  CRUD, BFS, DFS  │  │  HNSW Index      │
│  Hybrid Search   │  │  ANN Search      │
│  Semantic Walk   │  │  Persistence     │
│  Temporal Queries│  │                  │
└──────────┬───────┘  └──────────────────┘
           │
┌──────────▼──────────────────────────────┐
│          astraea-storage                │
│  Pages → Buffer Pool → Pointer Swizzle │
│  MVCC, WAL, PageIO, Cold Storage       │
└──────────┬──────────────────────────────┘
           │
┌──────────▼──────────────────────────────┐
│          astraea-core                   │
│  Types, Traits, Errors                  │
│  Node, Edge, StorageEngine, ...         │
└──────────┬──────────────────────────────┘
           │
    ┌──────┴──────┬───────────────┐
    │             │               │
┌───▼────────┐ ┌─▼───────────┐ ┌─▼──────────────┐
│astraea-    │ │astraea-     │ │astraea-        │
│  crypto    │ │  gpu        │ │  cluster       │
│Encrypted   │ │CSR Matrix,  │ │Partitioning,   │
│Labels,     │ │CPU Backend, │ │Sharding,       │
│FHE Engine  │ │PageRank/BFS │ │Coordination    │
└────────────┘ └─────────────┘ └────────────────┘
```

## Crate Overview

| Crate | Purpose | Tests |
|---|---|---:|
| `astraea-core` | Foundational types (`Node`, `Edge`, `NodeId`), traits (`StorageEngine`, `GraphOps`, `VectorIndex`, `TransactionalEngine`), and error types | 4 |
| `astraea-storage` | Disk-backed storage engine: 8 KiB pages, LRU buffer pool with pointer swizzling, MVCC transactions, WAL with CRC32 checksums, PageIO trait (+ io_uring on Linux), cold storage (JSON, Parquet, S3/GCS/Azure), label index | 75 |
| `astraea-graph` | Graph CRUD, traversals (BFS, DFS, Dijkstra), temporal queries, hybrid search, semantic traversal, auto-indexing vector embeddings | 55 |
| `astraea-query` | Hand-written GQL/Cypher parser and executor: lexer, recursive-descent parser, AST, full query execution pipeline | 56 |
| `astraea-vector` | HNSW approximate nearest-neighbor index with cosine, Euclidean, and dot-product distance metrics; binary persistence | 33 |
| `astraea-rag` | GraphRAG engine: subgraph extraction (BFS + semantic), linearization (4 formats), token budgets, LLM provider trait, GraphRAG pipeline | 27 |
| `astraea-gnn` | GNN engine: learnable weight matrices (GNNModel), analytical backpropagation, SpMM-accelerated message passing, GraphSAGE neighbor sampling, Adam optimizer, EvolveGCN temporal learning, node classification training | 53 |
| `astraea-server` | Async TCP server (tokio) with JSON/gRPC transport; auth (RBAC + mTLS), metrics (Prometheus), connection management, GQL execution, vector/hybrid/semantic/RAG operations | 68 |
| `astraea-flight` | Apache Arrow Flight server for zero-copy data exchange: `do_get` (query → Arrow), `do_put` (Arrow → bulk import) | 11 |
| `astraea-algorithms` | Graph algorithms: PageRank (power iteration), connected/strongly-connected components (Tarjan's), degree/betweenness centrality (Brandes'), Louvain community detection | 20 |
| `astraea-crypto` | Homomorphic encryption foundation: key generation, encrypted labels/values/nodes, server-side encrypted label matching | 31 |
| `astraea-gpu` | GPU acceleration framework: CSR matrix representation, GpuBackend trait, CPU fallback (PageRank, BFS, SSSP) | 16 |
| `astraea-cluster` | Distributed processing foundation: hash/range partitioning, shard management, cluster coordinator trait | 19 |
| `astraea-cli` | Command-line interface: `serve`, `shell` (REPL), `status`, `import`, `export` | - |
| `python/astraeadb` | Python client: JSON/TCP (no deps) + Arrow Flight (optional pyarrow) | 23 |
| `go/astraeadb` | Go client: JSON/TCP, gRPC (protobuf), unified client with auto-transport selection | 30 |
| `java/astraeadb` | Java client: JSON/TCP, gRPC (protobuf), Arrow Flight, unified client with auto-transport selection | 113 |
| **Rust Total** | | **441** |
| **Python Total** | | **23** |
| **Go Total** | | **30** |
| **Java Total** | | **113** |

## Data Model: Vector-Property Graph

AstraeaDB unifies property graphs and vector embeddings into a single data model:

- **Nodes** carry labels, arbitrary JSON properties, and an optional float32 embedding vector.
- **Edges** carry a type, JSON properties, a learnable weight (for GNN integration), and a temporal validity interval.
- **Vector Index** stores node embeddings in an HNSW graph where the navigation links can map to graph edges, enabling "semantic traversal."

```rust
// Create a node with an embedding
let node_id = graph.create_node(
    vec!["Person".into()],
    json!({"name": "Alice", "age": 30}),
    Some(vec![0.1, 0.2, 0.3, ...]),  // 128-dim embedding
)?;

// Create a weighted, temporal edge
let edge = Edge {
    source: alice_id,
    target: bob_id,
    edge_type: "KNOWS".into(),
    weight: 0.95,
    validity: ValidityInterval {
        valid_from: Some(1704067200000),  // 2024-01-01
        valid_to: None,                   // still valid
    },
    ..
};
```

## Components

### Storage Engine (`astraea-storage`)

A three-tier storage architecture:

- **Tier 1 (Cold):** `ColdStorage` trait with three pluggable backends:
  - `JsonFileColdStorage` — human-readable JSON files on local disk
  - `ParquetColdStorage` — columnar Apache Parquet format with Arrow schema mapping
  - `ObjectStoreColdStorage` — cloud object stores (S3, GCS, Azure) or local filesystem via `object_store` crate
- **Tier 2 (Warm):** An LRU buffer pool caches frequently accessed pages in memory with pin/unpin semantics. The `PageIO` trait abstracts disk I/O with two backends:
  - `FileManager` — cross-platform memmap2-based I/O (default)
  - `UringPageIO` — Linux io_uring async I/O (feature-gated: `--features io-uring`)
- **Tier 3 (Hot):** **Pointer swizzling** promotes frequently-accessed pages to permanently-pinned status, preventing eviction and enabling zero-copy access. In-memory indices and the HNSW vector index provide nanosecond-level lookups.

**MVCC Transactions:** Snapshot isolation with first-writer-wins conflict detection. `TransactionalEngine` trait provides `begin_transaction()`, `commit_transaction()`, `abort_transaction()`, and transactional read/write methods. Write sets are buffered per-transaction and applied atomically on commit.

**Label Index:** `HashMap<String, HashSet<NodeId>>` for O(1) label-based node lookups, integrated with `put_node()` and `delete_node()`.

**Write-Ahead Log (WAL):** Every mutation is logged before being applied. Records use a `[length][type][JSON payload][CRC32]` frame format. Supports `BeginTransaction`, `CommitTransaction`, and `AbortTransaction` records. The WAL supports checkpoint and truncation for recovery.

**Page Format:**
```
┌─────────────────────────────────┐
│ PageHeader (17 bytes)           │
│   page_id, type, record_count, │
│   free_space_offset, checksum  │
├─────────────────────────────────┤
│ Record 0: NodeRecordHeader      │
│   node_id, data_len, adj_offset│
│   + serialized properties      │
├─────────────────────────────────┤
│ Record 1: ...                   │
├─────────────────────────────────┤
│         (free space)            │
└─────────────────────────────────┘
          8192 bytes total
```

### Graph Operations (`astraea-graph`)

Implements the `GraphOps` trait on top of any `StorageEngine`:

- **CRUD:** Create, read, update, and delete nodes and edges. Deleting a node cascades to all connected edges.
- **Property updates** use JSON merge semantics (keys are inserted or overwritten, not replaced wholesale).
- **Traversals:**
  - `bfs(start, max_depth)` — breadth-first search returning `(NodeId, depth)` pairs
  - `dfs(start, max_depth)` — depth-first search
  - `shortest_path(from, to)` — unweighted shortest path via BFS
  - `shortest_path_weighted(from, to)` — Dijkstra's algorithm using edge weights
- **Neighbor queries** support direction filtering (`Outgoing`, `Incoming`, `Both`) and edge-type filtering.
- **Hybrid Search:**
  - `hybrid_search(anchor, query_embedding, max_hops, k, alpha)` — BFS from anchor to collect candidates, score by vector distance, blend: `final_score = alpha * vector_score + (1 - alpha) * graph_score`, return top-k.
- **Semantic Traversal:**
  - `semantic_neighbors(node_id, concept_embedding, direction, k)` — rank neighbors by embedding similarity to a concept vector.
  - `semantic_walk(start, concept_embedding, max_hops)` — greedy multi-hop walk, at each hop moving to the unvisited neighbor most similar to the concept embedding.
- **Temporal Queries:**
  - `neighbors_at(node_id, direction, timestamp)` — neighbors filtered to edges valid at the given timestamp
  - `bfs_at(start, max_depth, timestamp)` — BFS traversal only following edges valid at the timestamp
  - `shortest_path_at(from, to, timestamp)` — shortest path using only temporally-valid edges
  - `shortest_path_weighted_at(from, to, timestamp)` — Dijkstra with temporal filtering
- **Auto-indexing:** When a `VectorIndex` is attached, `create_node()` automatically indexes embeddings and `delete_node()` removes them.

### GQL Parser & Executor (`astraea-query`)

A hand-written recursive-descent parser and full query executor for a subset of ISO GQL / Cypher:

```
MATCH (a:Person)-[:KNOWS]->(b:Person)
WHERE a.age > 30 AND b.name = "Bob"
RETURN a.name AS person, b.name AS friend
ORDER BY a.age DESC
LIMIT 10
```

**Supported statements:**
- `MATCH` with node/edge patterns, `WHERE`, `RETURN` (with `DISTINCT`), `ORDER BY`, `SKIP`, `LIMIT`
- `CREATE` with node and edge patterns (inline properties)
- `DELETE` with variable references

**Expression support:**
- Property access (`a.name`), literals, arithmetic (`+`, `-`, `*`, `/`, `%`)
- Comparisons (`=`, `<>`, `<`, `<=`, `>`, `>=`)
- Boolean logic (`AND`, `OR`, `NOT`) with correct precedence
- `IS NULL` / `IS NOT NULL`
- Function calls (`count(a)`, `sum(a.x)`)
- Parenthesized grouping

**Edge directions:** `-[:TYPE]->` (outgoing), `<-[:TYPE]-` (incoming), `-[:TYPE]-` (undirected)

**Query Executor:** Full execution pipeline: pattern resolution (label-based lookups via the label index) -> WHERE filtering (recursive expression evaluation) -> ORDER BY -> RETURN projection (with aliasing) -> DISTINCT -> SKIP/LIMIT. Built-in functions include `id()`, `labels()`, `type()`, `count()`, `toString()`, `toInteger()`.

### HNSW Vector Index (`astraea-vector`)

An implementation of the Hierarchical Navigable Small World algorithm (Malkov & Yashunin, 2016):

- **Multi-layer graph** with exponentially decreasing node membership at higher layers
- **Configurable parameters:** `M` (max connections, default 16), `ef_construction` (build beam width, default 200), `ef_search` (query beam width, default 50)
- **Three distance metrics:** Cosine similarity, Euclidean (L2), dot product
- **Incremental updates:** Insert and remove vectors without rebuilding
- **Binary persistence:** Versioned file format with magic bytes, bincode serialization. Save/load entire index to disk without rebuilding
- **Thread-safe:** `RwLock` wrapper allows concurrent reads with exclusive writes

```rust
let index = HnswVectorIndex::new(128, DistanceMetric::Cosine);
index.insert(node_id, &embedding)?;

let results = index.search(&query_vector, 10)?;
// results: Vec<SimilarityResult { node_id, distance }>
```

### Network Server (`astraea-server` + `astraea-flight`)

Three transport layers for different use cases:

1. **JSON-over-TCP** (port 7687): Newline-delimited JSON wire protocol. Each request/response is a single JSON line, debuggable with `telnet` or `netcat`.
2. **gRPC/Protobuf** (port 7688): Schema-enforced API via `tonic`/`prost` with 14 RPCs. Better performance and type safety for production clients.
3. **Arrow Flight** (port 7689): Zero-copy data exchange via Apache Arrow Flight. `do_get` streams GQL query results as Arrow RecordBatches; `do_put` accepts Arrow tables for bulk node/edge import. Ideal for Python/Polars/Pandas integration.

JSON and gRPC transports delegate to the same `RequestHandler` and `Executor`. The Flight server wraps the same `Graph` + `Executor` with Arrow serialization.

**Authentication & Access Control:**
- API key authentication with `auth_token` field in JSON requests
- **mTLS (mutual TLS):** Optional TLS encryption with client certificate verification
  - `TlsConfig` with `cert_path`, `key_path`, `ca_cert_path`, `require_client_cert`
  - Client certificate CN automatically maps to role (`admin`, `writer`, `reader`)
  - Uses `rustls` for modern, safe TLS implementation
- Three roles: `Reader` (read-only), `Writer` (read + write), `Admin` (full access)
- Audit logging with bounded circular buffer
- Key management: add, revoke, list

**Connection Management:**
- Configurable connection limits (default: 1024) with semaphore-based enforcement
- Request-level backpressure (default: 256 concurrent requests)
- Idle timeout (default: 5 minutes) and request timeout (default: 30 seconds)
- Graceful shutdown: stops accepting, drains in-flight requests, flushes state

**Observability:**
- Prometheus text exposition format at the metrics endpoint
- Request counters, error counters, duration percentiles (p50/p90/p99)
- Connection gauges (active, total, rejected)
- Health check returning uptime, connection stats, status

**Supported requests:**

| Request | Description |
|---|---|
| `CreateNode` | Create a node with labels, properties, optional embedding |
| `CreateEdge` | Create an edge between two nodes |
| `GetNode` / `GetEdge` | Retrieve by ID |
| `UpdateNode` / `UpdateEdge` | Merge properties |
| `DeleteNode` / `DeleteEdge` | Delete (node deletion cascades edges) |
| `Neighbors` | Get neighbors with direction and edge-type filtering |
| `Bfs` | Breadth-first traversal with depth limit |
| `ShortestPath` | Unweighted or weighted (Dijkstra) shortest path |
| `VectorSearch` | k-nearest-neighbor search via attached HNSW index |
| `HybridSearch` | Blended graph proximity + vector similarity (configurable alpha) |
| `SemanticNeighbors` | Rank neighbors by embedding similarity to a concept |
| `SemanticWalk` | Greedy multi-hop walk toward a concept embedding |
| `ExtractSubgraph` | Extract and linearize a local subgraph (Prose/Structured/Triples/JSON) |
| `GraphRag` | GraphRAG pipeline: vector search → subgraph → linearize → context for LLM |
| `NeighborsAt` | Get neighbors at a specific timestamp (temporal edge filtering) |
| `BfsAt` | BFS traversal at a specific timestamp |
| `ShortestPathAt` | Shortest path at a specific timestamp (weighted or unweighted) |
| `Query` | Execute a GQL query string (fully functional) |
| `Ping` | Health check |

### CLI (`astraea-cli`)

```
astraeadb serve [--config config.toml] [--bind 0.0.0.0] [--port 7687]
astraeadb shell [--address 127.0.0.1:7687]
astraeadb status [--address 127.0.0.1:7687]
astraeadb import --file data.json --format json --data-dir ./data
astraeadb export --file dump.json --format json --data-dir ./data
```

**Configuration** is loaded from a TOML file (default `config.toml`):

```toml
[server]
bind_address = "127.0.0.1"
port = 7687

[storage]
data_dir = "data"
buffer_pool_size = 1024
wal_dir = "data/wal"
```

### GraphRAG Engine (`astraea-rag`)

Retrieval-Augmented Generation for graph-backed LLM context:

- **Subgraph extraction:** `extract_subgraph(graph, center, hops, max_nodes)` — BFS-based local neighborhood extraction, capped at `max_nodes`. `extract_subgraph_semantic()` uses vector search to find the anchor automatically.
- **Linearization:** Convert subgraphs to text in 4 formats:
  - `Structured` — indented tree with arrows (`-[KNOWS]->`)
  - `Prose` — natural language paragraphs
  - `Triples` — `(subject, predicate, object)` triples
  - `Json` — compact JSON for structured prompts
- **Token budget:** `extract_with_budget()` incrementally builds the subgraph, stopping when estimated tokens approach the budget.
- **LLM providers:** `LlmProvider` trait with Mock, OpenAI, Anthropic, and Ollama implementations. Providers use injectable HTTP callbacks (no HTTP dependencies in the crate). Users supply their own HTTP client.
- **GraphRAG pipeline:** `graph_rag_query()` performs: vector search → subgraph extraction → linearization → LLM completion. `graph_rag_query_anchored()` skips vector search when the anchor node is known.

### GNN Engine (`astraea-gnn`)

Full Graph Neural Network engine in pure Rust (no external ML framework). Designed around findings from testing on the Elliptic Bitcoin dataset (203k nodes, 234k edges, 49 temporal snapshots).

- **Model architecture (`model.rs`):** `GNNModel` with learnable weight matrices (`W_neigh`, `W_self`, bias) per layer and a classification head (`W_out`, `b_out`). Decouples input feature dimension from hidden dimension and output classes. Per-layer computation: `h' = activation(W_self * h + AGG(w_e * W_neigh * h_j) + bias)`. Xavier weight initialization for stable training.
- **Analytical backpropagation (`backward.rs`):** Full gradient computation through softmax cross-entropy loss, classification head, and per-layer message passing. Replaces O(E * L * N) numerical gradients with O(N + E) analytical gradients — ~1000x speedup on large graphs.
- **Tensor & Matrix (`tensor.rs`):** `Tensor` struct with element-wise ops, activations (ReLU, Sigmoid, LeakyReLU, Tanh, ELU), gradient tracking. `Matrix` struct (row-major) with matvec, transpose_matvec, outer product, Xavier initialization.
- **Message passing (`message_passing.rs`):** Configurable aggregation (Sum, Mean, Max), activation (ReLU, Sigmoid, LeakyReLU, Tanh, ELU, None), optional L2 normalization, dropout.
- **SpMM acceleration (`sparse.rs`):** `FeatureMatrix` (contiguous row-major) and `CSRAdjacency` (Compressed Sparse Row) for cache-friendly SpMM-based message passing on large graphs. Verified equivalent to HashMap-based forward pass.
- **Neighbor sampling (`sampling.rs`):** GraphSAGE-style fixed-fanout sampling (`SamplingConfig { fanout, batch_size }`) for mini-batch training on graphs that exceed memory limits.
- **Training loop (`training.rs`):** `train_node_classification()` supports both legacy (edge-weight-only, numerical gradients) and new (weight matrices, analytical backpropagation) modes. Adam optimizer, early stopping with validation split, configurable hidden dimension.
- **Temporal GNN (`temporal.rs`):** EvolveGCN-H architecture with GRU cells that evolve layer weights across timesteps. `train_temporal()` trains over sequences of temporal snapshots using `neighbors_at()` for time-aware message passing.

## Quick Start

### Build

```bash
cargo build --workspace
```

### Run Tests

```bash
cargo test --workspace
```

### Start the Server

```bash
cargo run -p astraea-cli -- serve
```

### Connect with the Shell

```bash
cargo run -p astraea-cli -- shell
```

Then use GQL queries or JSON requests:

```
astraea> CREATE (a:Person {name: "Alice", age: 30})
Nodes created: 1

astraea> CREATE (b:Person {name: "Bob", age: 25})
Nodes created: 1

astraea> MATCH (a:Person) WHERE a.age > 25 RETURN a.name, a.age ORDER BY a.age DESC
+-------+-----+
| a.name| a.age|
+-------+-----+
| Alice | 30  |
+-------+-----+

astraea> .status
Connected to 127.0.0.1:7687 — version 0.1.0

astraea> .quit
```

JSON requests are also supported:

```json
{"type":"CreateNode","labels":["Person"],"properties":{"name":"Alice","age":30}}
{"type":"Neighbors","id":1,"direction":"outgoing"}
{"type":"Ping"}
```

### Check Server Status

```bash
cargo run -p astraea-cli -- status
```

## Programmatic Usage

### Embedded (no server)

```rust
use astraea_graph::Graph;
use astraea_graph::test_utils::InMemoryStorage;
use astraea_core::traits::GraphOps;
use astraea_core::types::Direction;

// Create a graph with in-memory storage
let storage = InMemoryStorage::new();
let graph = Graph::new(Box::new(storage));

// Add nodes
let alice = graph.create_node(
    vec!["Person".into()],
    serde_json::json!({"name": "Alice"}),
    None,
)?;
let bob = graph.create_node(
    vec!["Person".into()],
    serde_json::json!({"name": "Bob"}),
    None,
)?;

// Add an edge (None, None = always valid; use Some(epoch_ms) for temporal edges)
graph.create_edge(alice, bob, "KNOWS".into(), serde_json::json!({}), 1.0, None, None)?;

// Traverse
let neighbors = graph.neighbors(alice, Direction::Outgoing)?;
let path = graph.shortest_path(alice, bob)?;
let bfs_results = graph.bfs(alice, 3)?;
```

### With Disk Persistence

```rust
use astraea_storage::DiskStorageEngine;
use astraea_graph::Graph;

let storage = DiskStorageEngine::new("./my_database")?;
let graph = Graph::new(Box::new(storage));
// ... use graph as above ...
graph.storage().flush()?;  // Persist to disk
```

### Vector Search

```rust
use astraea_vector::HnswVectorIndex;
use astraea_core::traits::VectorIndex;
use astraea_core::types::DistanceMetric;

let index = HnswVectorIndex::new(128, DistanceMetric::Cosine);

// Insert embeddings
index.insert(node_id, &embedding_vec)?;

// Search for similar nodes
let results = index.search(&query_vec, 10)?;
for result in results {
    println!("Node {:?}, distance: {}", result.node_id, result.distance);
}
```

### GraphRAG (Subgraph Extraction + LLM)

```rust
use astraea_rag::{extract_subgraph, linearize_subgraph, TextFormat, estimate_tokens};
use astraea_rag::{GraphRagConfig, graph_rag_query_anchored, MockProvider};
use astraea_core::traits::GraphOps;

// Extract a 2-hop subgraph around a node (max 50 nodes)
let subgraph = extract_subgraph(&*graph, node_id, 2, 50)?;

// Linearize to text for LLM context
let text = linearize_subgraph(&subgraph, TextFormat::Structured);
let tokens = estimate_tokens(&text);

// Full GraphRAG pipeline with an LLM provider
let llm = MockProvider {
    response_prefix: "Based on the graph:".into(),
    context_window: 8000,
};
let config = GraphRagConfig {
    hops: 2,
    max_context_nodes: 50,
    text_format: TextFormat::Structured,
    token_budget: 4000,
    ..Default::default()
};
let result = graph_rag_query_anchored(&*graph, &llm, "Who knows Alice?", node_id, &config)?;
println!("Answer: {}", result.answer);
println!("Context used {} tokens across {} nodes", result.estimated_tokens, result.nodes_in_context);
```

### GNN Training

```rust
use astraea_gnn::{TrainingConfig, TrainingData, MessagePassingConfig, Aggregation, Activation};
use astraea_gnn::training::train_node_classification;
use std::collections::HashMap;

// Define training labels (node classification)
let mut labels = HashMap::new();
labels.insert(NodeId(1), 0);  // class 0
labels.insert(NodeId(2), 1);  // class 1
let training_data = TrainingData { labels, num_classes: 2 };

// Configure with learnable weight matrices and analytical backpropagation
let config = TrainingConfig {
    layers: 2,
    learning_rate: 0.01,
    epochs: 50,
    message_passing: MessagePassingConfig {
        aggregation: Aggregation::Mean,
        activation: Activation::ReLU,
        normalize: true,
        dropout: 0.0,
    },
    hidden_dim: Some(64),           // Enable weight matrices (None = legacy mode)
    use_adam: true,                  // Adam optimizer (vs vanilla SGD)
    early_stopping_patience: Some(10), // Stop if val loss plateaus
    validation_split: Some(0.2),    // 20% held out for validation
};
let result = train_node_classification(&*graph, &training_data, &config)?;
println!("Final accuracy: {:.1}%", result.accuracy * 100.0);
println!("Loss decreased: {:.4} -> {:.4}", result.epoch_losses[0], result.epoch_losses.last().unwrap());
```

### Hybrid Search & Semantic Traversal

```rust
use astraea_graph::Graph;
use astraea_vector::HnswVectorIndex;
use astraea_core::traits::{GraphOps, VectorIndex};
use astraea_core::types::{Direction, DistanceMetric};
use std::sync::Arc;

// Create a graph with an attached vector index
let storage = InMemoryStorage::new();
let vector_index = Arc::new(HnswVectorIndex::new(128, DistanceMetric::Cosine));
let graph = Graph::with_vector_index(Box::new(storage), vector_index);

// Nodes with embeddings are auto-indexed
let alice = graph.create_node(
    vec!["Person".into()],
    serde_json::json!({"name": "Alice"}),
    Some(vec![0.1; 128]),  // embedding auto-indexed
)?;

// Hybrid search: combine graph proximity + vector similarity
// alpha=0.5 blends equally; alpha=1.0 = pure vector; alpha=0.0 = pure graph
let results = graph.hybrid_search(alice, &query_embedding, 3, 10, 0.5)?;

// Semantic neighbors: rank neighbors by similarity to a concept
let similar = graph.semantic_neighbors(alice, &concept_vec, Direction::Outgoing, 5)?;

// Semantic walk: greedy multi-hop walk toward a concept
let path = graph.semantic_walk(alice, &concept_vec, 4)?;
```

### Parsing & Executing GQL Queries

```rust
use astraea_query::parse;
use astraea_query::executor::Executor;

// Parse a GQL query into an AST
let ast = parse("MATCH (a:Person)-[:KNOWS]->(b) WHERE a.age > 30 RETURN b.name")?;

// Execute against a graph (requires Arc<dyn GraphOps>)
let executor = Executor::new(graph.clone());
let result = executor.execute(ast)?;
// result.columns: ["b.name"]
// result.rows: [["Bob"], ["Charlie"], ...]
// result.stats: { nodes_created: 0, edges_created: 0, ... }
```

### Python Client

AstraeaDB provides two Python clients in the `python/astraeadb` package:

- **`JsonClient`** — TCP/JSON protocol with zero external dependencies. Works out of the box with Python 3.10+.
- **`ArrowClient`** — Arrow Flight protocol for zero-copy data exchange. Requires `pip install astraeadb[arrow]` (installs `pyarrow`).
- **`AstraeaClient`** — Unified client that auto-selects Arrow for bulk queries and falls back to JSON.

A legacy example client is also available at `examples/python_client.py`.

**Installation:**

```bash
# Basic (JSON only, no dependencies)
pip install ./python

# With Arrow Flight support
pip install ./python[arrow]
```

**Using the client:**

```python
from astraeadb import AstraeaClient

# Connect with optional authentication
with AstraeaClient(host="127.0.0.1", port=7687, auth_token="my-api-key") as client:
    # Create nodes (embeddings are auto-indexed server-side)
    alice = client.create_node(["Person"], {"name": "Alice", "age": 30},
                               embedding=[0.1] * 128)
    bob = client.create_node(["Person"], {"name": "Bob", "age": 25},
                             embedding=[0.2] * 128)

    # Create a temporal edge (valid_from in milliseconds)
    client.create_edge(alice, bob, "KNOWS", {"since": 2020}, weight=0.9,
                       valid_from=1609459200000)  # Jan 1, 2021

    # Query neighbors
    neighbors = client.neighbors(alice, direction="outgoing")

    # Temporal query - neighbors at a point in time
    old_neighbors = client.neighbors_at(alice, "outgoing", 1577836800000)  # Jan 1, 2020

    # BFS traversal (2 hops)
    reachable = client.bfs(alice, max_depth=2)

    # Shortest path (weighted Dijkstra)
    path = client.shortest_path(alice, bob, weighted=True)

    # Vector search (k-nearest-neighbors)
    results = client.vector_search([0.15] * 128, k=5)

    # Hybrid search (graph proximity + vector similarity)
    results = client.hybrid_search(anchor=alice, query_vector=[0.15] * 128,
                                   max_hops=3, k=10, alpha=0.5)

    # GraphRAG - extract subgraph context
    context = client.extract_subgraph(alice, hops=2, max_nodes=50, format="prose")

    # GraphRAG - full pipeline with LLM
    answer = client.graph_rag("Who does Alice know?", anchor=alice)

    # Batch operations
    node_ids = client.create_nodes([
        {"labels": ["Person"], "properties": {"name": "Charlie"}},
        {"labels": ["Person"], "properties": {"name": "Diana"}}
    ])

    # Execute GQL queries
    result = client.query("MATCH (a:Person) WHERE a.age > 25 RETURN a.name")

    # Health check
    status = client.ping()
```

**DataFrame support (requires pandas):**

```python
from astraeadb import AstraeaClient
from astraeadb.dataframe import import_nodes_df, export_nodes_df, export_bfs_df
import pandas as pd

df = pd.DataFrame([
    {"label": "Person", "name": "Alice", "age": 30},
    {"label": "Person", "name": "Bob", "age": 25}
])

with AstraeaClient() as client:
    node_ids = import_nodes_df(client, df, label_col="label")
    result_df = export_nodes_df(client, node_ids)
    bfs_df = export_bfs_df(client, start=node_ids[0], max_depth=2)
```

**Arrow Flight client for bulk operations:**

```python
from astraeadb import ArrowClient

arrow = ArrowClient(host="127.0.0.1", flight_port=7689)

# Execute query, get results as an Apache Arrow Table
table = arrow.query("MATCH (a:Person) RETURN a.name, a.age")
df = table.to_pandas()  # zero-copy to Pandas

# Bulk import nodes from an Arrow Table
import pyarrow as pa
nodes_table = pa.table({
    "id": [1, 2, 3],
    "labels": ["Person", "Person", "Person"],
    "properties": ['{"name":"Alice"}', '{"name":"Bob"}', '{"name":"Charlie"}'],
})
arrow.bulk_insert_nodes(nodes_table)
```

**Client API reference:**

| Category | Method | Description |
|---|---|---|
| Health | `ping()` | Health check, returns server version |
| Node CRUD | `create_node(labels, properties?, embedding?)` | Create a node, returns node ID |
| | `get_node(id)` / `update_node(id, props)` / `delete_node(id)` | Get, update, or delete a node |
| Edge CRUD | `create_edge(src, tgt, type, props?, weight?, valid_from?, valid_to?)` | Create edge with optional temporal validity |
| | `get_edge(id)` / `update_edge(id, props)` / `delete_edge(id)` | Get, update, or delete an edge |
| Traversal | `neighbors(id, direction?, edge_type?)` | Get neighbors with optional filtering |
| | `bfs(start, max_depth?)` | Breadth-first traversal |
| | `shortest_path(from, to, weighted?)` | Shortest path (BFS or Dijkstra) |
| Temporal | `neighbors_at(id, direction, timestamp, edge_type?)` | Neighbors at point in time |
| | `bfs_at(start, max_depth, timestamp)` | BFS at point in time |
| | `shortest_path_at(from, to, timestamp, weighted?)` | Path at point in time |
| Vector | `vector_search(embedding, k?)` | k-nearest-neighbor search |
| | `hybrid_search(anchor, embedding, max_hops?, k?, alpha?)` | Blended graph + vector search |
| | `semantic_neighbors(node, embedding, direction?, k?)` | Rank neighbors by concept similarity |
| | `semantic_walk(start, embedding, max_hops?)` | Greedy semantic walk |
| GraphRAG | `extract_subgraph(center, hops?, max_nodes?, format?)` | Extract + linearize subgraph |
| | `graph_rag(question, anchor?, embedding?, hops?, max_nodes?, format?)` | Full RAG pipeline with LLM |
| GQL | `query(gql_string)` | Execute a GQL query |
| Batch | `create_nodes(nodes_list)` / `create_edges(edges_list)` | Batch create |
| | `delete_nodes(node_ids)` / `delete_edges(edge_ids)` | Batch delete |

**DataFrame module** (`from astraeadb.dataframe import ...`): `import_nodes_df`, `import_edges_df`, `export_nodes_df`, `export_edges_df`, `export_bfs_df`, `export_bfs_at_df`

### R Client

A full-featured R client is provided at `examples/r_client.R` with feature parity to the Python client. Includes three client classes:

- **AstraeaClient** - JSON/TCP client (always available)
- **ArrowClient** - Arrow Flight client for high-performance queries
- **UnifiedClient** - Auto-selects best available transport

**Prerequisites:**

```r
install.packages("jsonlite")  # Required
install.packages("arrow")     # Optional, for Arrow Flight support
```

**Run the demo:**

```bash
# Start the server
cargo run -p astraea-cli -- serve

# In another terminal
Rscript examples/r_client.R
```

**Programmatic usage in R:**

```r
library(jsonlite)
source("examples/r_client.R")

# Connect with optional auth token
client <- AstraeaClient$new(host = "127.0.0.1", port = 7687L, auth_token = "my-key")
client$connect()

# Create nodes with embeddings
alice <- client$create_node(
  list("Person"),
  list(name = "Alice", age = 30),
  embedding = c(0.9, 0.1, 0.3)
)

# Create temporal edges (valid_from/valid_to in milliseconds)
eid <- client$create_edge(
  alice, bob, "KNOWS",
  properties = list(since = 2024),
  weight = 0.9,
  valid_from = 1704067200000  # Jan 1, 2024
)

# Vector search
results <- client$vector_search(c(1.0, 0.0, 0.0), k = 5L)

# Hybrid search (graph + vector)
results <- client$hybrid_search(alice, c(0.5, 0.5, 0.0), max_hops = 2L, k = 10L, alpha = 0.5)

# Temporal queries (time-travel)
neighbors_2020 <- client$neighbors_at(alice, "outgoing", timestamp = 1577836800000)

# GQL queries
result <- client$query("MATCH (p:Person) RETURN p.name")

# GraphRAG
subgraph <- client$extract_subgraph(alice, hops = 2L, format = "structured")
answer <- client$graph_rag("Who does Alice know?", anchor = alice)

client$close()
```

**Client API reference:**

| Category | Method | Description |
|---|---|---|
| Connection | `$new(host, port, auth_token?)` | Create client instance |
| | `$connect()` / `$close()` | Open/close TCP connection |
| | `$ping()` | Health check |
| Node CRUD | `$create_node(labels, properties, embedding?)` | Create node, returns ID |
| | `$get_node(id)` / `$update_node(id, props)` / `$delete_node(id)` | Read/update/delete |
| Edge CRUD | `$create_edge(src, tgt, type, props?, weight?, valid_from?, valid_to?)` | Create temporal edge |
| | `$get_edge(id)` / `$update_edge(id, props)` / `$delete_edge(id)` | Read/update/delete |
| Traversal | `$neighbors(id, direction?, edge_type?)` | Get neighbors |
| | `$bfs(start, max_depth?)` | Breadth-first search |
| | `$shortest_path(from, to, weighted?)` | Shortest path |
| Temporal | `$neighbors_at(id, direction, timestamp, edge_type?)` | Neighbors at time T |
| | `$bfs_at(start, max_depth, timestamp)` | BFS at time T |
| | `$shortest_path_at(from, to, timestamp, weighted?)` | Path at time T |
| Vector | `$vector_search(query_vector, k?)` | k-NN search |
| | `$hybrid_search(anchor, vector, max_hops?, k?, alpha?)` | Graph + vector |
| | `$semantic_neighbors(id, concept, direction?, k?)` | Neighbors by similarity |
| | `$semantic_walk(start, concept, max_hops?)` | Greedy semantic walk |
| GQL | `$query(gql)` | Execute GQL query |
| GraphRAG | `$extract_subgraph(center, hops?, max_nodes?, format?)` | Extract + linearize |
| | `$graph_rag(question, anchor?, embedding?, hops?, max_nodes?, format?)` | Full RAG pipeline |
| Batch | `$create_nodes(nodes_list)` / `$create_edges(edges_list)` | Bulk create |
| | `$delete_nodes(ids)` / `$delete_edges(ids)` | Bulk delete |
| Data Frame | `$import_nodes_df(df, ...)` / `$import_edges_df(df, ...)` | Import from data.frame |
| | `$export_nodes_df(ids)` / `$export_bfs_df(start, depth)` | Export to data.frame |
| Arrow | `ArrowClient$query(gql)` / `$query_df(gql)` | High-performance queries |
| | `UnifiedClient` (auto-selects transport) | Best of both worlds |

### Go Client

A full-featured Go client is provided in the `go/astraeadb` package with three transport layers and idiomatic Go patterns:

- **`JSONClient`** — JSON/TCP transport with zero external dependencies beyond the Go standard library
- **`GRPCClient`** — gRPC transport with Protocol Buffers for type-safe, high-performance access (14 RPCs)
- **`Client`** (unified) — Auto-selects gRPC when available, falls back to JSON/TCP. Arrow Flight support is stubbed for future implementation.

**Installation:**

```bash
go get github.com/AstraeaDB/AstraeaDB-Official
```

**Using the unified client:**

```go
package main

import (
    "context"
    "fmt"
    "log"

    "github.com/AstraeaDB/AstraeaDB-Official"
)

func main() {
    ctx := context.Background()

    // Connect with auto-transport selection (gRPC preferred, JSON/TCP fallback)
    client := astraeadb.NewClient(
        astraeadb.WithAddress("127.0.0.1", 7687),
        astraeadb.WithAuthToken("my-api-key"),
    )
    if err := client.Connect(ctx); err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    // Create nodes with embeddings
    alice, _ := client.CreateNode(ctx, []string{"Person"},
        map[string]any{"name": "Alice", "age": 30},
        []float32{0.1, 0.2, 0.3})

    bob, _ := client.CreateNode(ctx, []string{"Person"},
        map[string]any{"name": "Bob", "age": 25}, nil)

    // Create a temporal edge
    client.CreateEdge(ctx, alice, bob, "KNOWS",
        astraeadb.WithWeight(0.9),
        astraeadb.WithProperties(map[string]any{"since": 2020}),
        astraeadb.WithValidFrom(1609459200000))

    // Traverse
    neighbors, _ := client.Neighbors(ctx, alice, astraeadb.WithDirection("outgoing"))
    bfs, _ := client.BFS(ctx, alice, 3)
    path, _ := client.ShortestPath(ctx, alice, bob, true)

    // Vector search
    results, _ := client.VectorSearch(ctx, []float32{0.15, 0.25, 0.35}, 5)

    // Hybrid search (graph proximity + vector similarity)
    hybrid, _ := client.HybridSearch(ctx, alice, []float32{0.15, 0.25, 0.35},
        astraeadb.WithK(10), astraeadb.WithMaxHops(3))

    // Temporal query - neighbors at a point in time
    temporal, _ := client.NeighborsAt(ctx, alice, "outgoing", 1577836800000)

    // GQL query
    result, _ := client.Query(ctx, "MATCH (n:Person) WHERE n.age > 25 RETURN n.name")

    // GraphRAG
    rag, _ := client.GraphRAG(ctx, "Who does Alice know?",
        astraeadb.WithAnchor(alice), astraeadb.WithRAGHops(2))

    // Batch operations
    ids, _ := client.CreateNodes(ctx, []astraeadb.NodeInput{
        {Labels: []string{"Person"}, Properties: map[string]any{"name": "Charlie"}},
        {Labels: []string{"Person"}, Properties: map[string]any{"name": "Diana"}},
    })

    fmt.Printf("neighbors=%d bfs=%d path=%v results=%d hybrid=%d temporal=%d rows=%d rag=%s ids=%v\n",
        len(neighbors), len(bfs), path.Found, len(results), len(hybrid), len(temporal), len(result.Rows), rag.Context, ids)
}
```

**Client API reference:**

| Category | Method | Description |
|---|---|---|
| Health | `Ping(ctx)` | Health check, returns server version |
| Node CRUD | `CreateNode(ctx, labels, properties, embedding)` | Create a node, returns node ID |
| | `GetNode(ctx, id)` / `UpdateNode(ctx, id, props)` / `DeleteNode(ctx, id)` | Get, update, or delete a node |
| Edge CRUD | `CreateEdge(ctx, src, tgt, type, opts...)` | Create edge with `WithWeight`, `WithProperties`, `WithValidFrom`, `WithValidTo` |
| | `GetEdge(ctx, id)` / `UpdateEdge(ctx, id, props)` / `DeleteEdge(ctx, id)` | Get, update, or delete an edge |
| Traversal | `Neighbors(ctx, id, opts...)` | Get neighbors with `WithDirection`, `WithEdgeType` |
| | `BFS(ctx, start, maxDepth)` | Breadth-first traversal |
| | `ShortestPath(ctx, from, to, weighted)` | Shortest path (BFS or Dijkstra) |
| Temporal | `NeighborsAt(ctx, id, direction, timestamp, opts...)` | Neighbors at point in time |
| | `BFSAt(ctx, start, maxDepth, timestamp)` | BFS at point in time |
| | `ShortestPathAt(ctx, from, to, timestamp, weighted)` | Path at point in time |
| Vector | `VectorSearch(ctx, embedding, k)` | k-nearest-neighbor search |
| | `HybridSearch(ctx, anchor, embedding, opts...)` | Blended graph + vector search |
| | `SemanticNeighbors(ctx, id, concept, opts...)` | Rank neighbors by concept similarity |
| | `SemanticWalk(ctx, start, concept, maxHops)` | Greedy semantic walk |
| GraphRAG | `ExtractSubgraph(ctx, center, opts...)` | Extract + linearize subgraph |
| | `GraphRAG(ctx, question, opts...)` | Full RAG pipeline with LLM |
| GQL | `Query(ctx, gql)` | Execute a GQL query |
| Batch | `CreateNodes(ctx, nodes)` / `CreateEdges(ctx, edges)` | Batch create |
| | `DeleteNodes(ctx, ids)` / `DeleteEdges(ctx, ids)` | Batch delete |

### Java Client

A full-featured Java client is provided in the `java/astraeadb` Gradle project with three transport layers, a unified client, and idiomatic Java patterns (records, builders, try-with-resources):

- **`JsonClient`** — JSON/TCP transport with all 22 operations. Requires Jackson for JSON serialization.
- **`GrpcClient`** — gRPC transport with Protocol Buffers for type-safe, high-performance access (14 RPCs).
- **`FlightAstraeaClient`** — Arrow Flight transport for zero-copy queries and bulk import.
- **`UnifiedClient`** — Auto-selects the best transport per operation with graceful degradation.

**Requirements:** Java 17+ (uses records and text blocks)

**Installation (Gradle):**

```groovy
dependencies {
    implementation 'com.astraeadb:astraeadb-unified:0.1.0'  // All transports
    // Or pick individual transports:
    // implementation 'com.astraeadb:astraeadb-json:0.1.0'
    // implementation 'com.astraeadb:astraeadb-grpc:0.1.0'
}
```

**Using the unified client:**

```java
import com.astraeadb.unified.UnifiedClient;
import com.astraeadb.model.*;
import com.astraeadb.options.*;

try (var client = UnifiedClient.builder()
        .host("127.0.0.1")
        .jsonPort(7687)
        .grpcPort(7688)
        .authToken("my-api-key")
        .build()) {

    client.connect();

    // Create nodes with embeddings
    long alice = client.createNode(
        List.of("Person"),
        Map.of("name", "Alice", "age", 30),
        new float[]{0.1f, 0.2f, 0.3f});

    long bob = client.createNode(
        List.of("Person"),
        Map.of("name", "Bob", "age", 25),
        null);

    // Create a temporal edge
    long edge = client.createEdge(alice, bob, "KNOWS",
        EdgeOptions.builder()
            .weight(0.9)
            .properties(Map.of("since", 2020))
            .validFrom(1609459200000L)
            .build());

    // Traverse
    List<NeighborEntry> neighbors = client.neighbors(alice,
        NeighborOptions.builder().direction("outgoing").build());

    List<BfsEntry> bfs = client.bfs(alice, 3);

    PathResult path = client.shortestPath(alice, bob, true);

    // Vector search
    List<SearchResult> results = client.vectorSearch(
        new float[]{0.15f, 0.25f, 0.35f}, 5);

    // Hybrid search (graph + vector)
    List<SearchResult> hybrid = client.hybridSearch(alice,
        new float[]{0.15f, 0.25f, 0.35f},
        HybridSearchOptions.builder().k(10).maxHops(3).build());

    // Temporal query
    List<NeighborEntry> temporal = client.neighborsAt(
        alice, "outgoing", 1577836800000L);

    // GQL query
    QueryResult result = client.query(
        "MATCH (n:Person) WHERE n.age > 25 RETURN n.name");

    // GraphRAG
    RagResult rag = client.graphRag("Who does Alice know?",
        RagOptions.builder().anchor(alice).hops(2).build());

    // Batch operations
    List<Long> ids = client.createNodes(List.of(
        new NodeInput(List.of("Person"), Map.of("name", "Charlie")),
        new NodeInput(List.of("Person"), Map.of("name", "Diana"))
    ));
}
```

**Client API reference:**

| Category | Method | Description |
|---|---|---|
| Health | `ping()` | Health check, returns server version |
| Node CRUD | `createNode(labels, properties, embedding)` | Create a node, returns node ID |
| | `getNode(id)` / `updateNode(id, props)` / `deleteNode(id)` | Get, update, or delete a node |
| Edge CRUD | `createEdge(src, tgt, type, options)` | Create edge with `EdgeOptions` (weight, temporal validity) |
| | `getEdge(id)` / `updateEdge(id, props)` / `deleteEdge(id)` | Get, update, or delete an edge |
| Traversal | `neighbors(id, options)` | Get neighbors with `NeighborOptions` (direction, edge type) |
| | `bfs(start, maxDepth)` | Breadth-first traversal |
| | `shortestPath(from, to, weighted)` | Shortest path (BFS or Dijkstra) |
| Temporal | `neighborsAt(id, direction, timestamp)` | Neighbors at point in time |
| | `bfsAt(start, maxDepth, timestamp)` | BFS at point in time |
| | `shortestPathAt(from, to, timestamp, weighted)` | Path at point in time |
| Vector | `vectorSearch(embedding, k)` | k-nearest-neighbor search |
| | `hybridSearch(anchor, embedding, options)` | Blended graph + vector search |
| | `semanticNeighbors(id, concept, options)` | Rank neighbors by concept similarity |
| | `semanticWalk(start, concept, maxHops)` | Greedy semantic walk |
| GraphRAG | `extractSubgraph(center, options)` | Extract + linearize subgraph |
| | `graphRag(question, options)` | Full RAG pipeline with LLM |
| GQL | `query(gql)` | Execute a GQL query |
| Batch | `createNodes(nodes)` / `createEdges(edges)` | Batch create |
| | `deleteNodes(ids)` / `deleteEdges(ids)` | Batch delete |

## Example: Cybersecurity Threat Investigation

This example demonstrates how AstraeaDB enables security analysts to investigate network alerts by tracing connections through a graph. A full runnable demo is provided at `examples/cybersecurity_demo.py` with matching Rust tests in the `astraea-graph` crate.

### The Problem

Cybersecurity tools typically deal in IP addresses, but IPs are ephemeral. When a firewall alerts on suspicious traffic from `10.0.1.50`, the analyst must manually:

1. Search DHCP logs to find which hostname held that IP at the time of the alert
2. Search asset management records to find which user is assigned to that hostname
3. Search other log sources to understand the full scope of the incident

With AstraeaDB, these datasets are loaded as a graph, and the investigation becomes a series of traversals.

### Graph Model

```
User <--[ASSIGNED_TO]-- Laptop <--[DHCP_LEASE]-- IPAddress
                                                    |
                                              [TRAFFIC]  [TRIGGERED]
                                                    |         |
                                              IPAddress  FirewallAlert --[TARGETS]--> ExternalHost
```

| Node Label | Properties | Description |
|---|---|---|
| `User` | `name`, `department`, `role` | Corporate employee |
| `Laptop` | `brand`, `model`, `serial_number`, `hostname` | Assigned device |
| `IPAddress` | `address`, `network` | Internal (10.0.x.y) or external IP |
| `ExternalHost` | `hostname`, `category`, `risk_level` | External server/website |
| `FirewallAlert` | `alert_id`, `rule`, `severity`, `timestamp`, `action` | Security alert |

| Edge Type | Direction | Key Feature |
|---|---|---|
| `ASSIGNED_TO` | Laptop -> User | Asset inventory |
| `DHCP_LEASE` | IPAddress -> Laptop | **Temporal edge** with `valid_from`/`valid_to` (lease window) |
| `TRAFFIC` | IPAddress -> IPAddress/ExternalHost | Network flow with port, protocol, bytes |
| `TRIGGERED` | IPAddress -> FirewallAlert | Links source IP to alert |
| `TARGETS` | FirewallAlert -> destination | Links alert to target |

### The Scenario: Eve's Attack

Three employees -- Alice (Engineering), Bob (Finance), and Eve (Marketing) -- each have laptops with DHCP-assigned IPs on the `10.0.1.0/24` network:

| User | Laptop | Hostname | IP Address |
|---|---|---|---|
| Alice | MacBook Pro 16 | ALICE-MBP01 | 10.0.1.10 |
| Bob | ThinkPad X1 | BOB-TP01 | 10.0.1.20 |
| Eve | Latitude 5540 | EVE-LAT01 | 10.0.1.50 |

Eve's attack chain:

1. Downloads a password cracker from `darktools.example.com` (port 443)
2. Firewall logs the connection (alert `FW-2025-0042`, severity: critical)
3. Attempts RDP to Bob's machine at `10.0.1.20:3389` -- **blocked**
4. Attempts SSH to Alice's machine at `10.0.1.10:22` -- **blocked**

### Investigation with AstraeaDB

**Step 1: Load datasets as graphs.**

```python
from examples.python_client import AstraeaClient

with AstraeaClient() as client:
    # Asset management: users and laptops
    eve = client.create_node(["User"], {"name": "Eve", "department": "Marketing"})
    laptop = client.create_node(["Laptop"], {"hostname": "EVE-LAT01", ...})
    client.create_edge(laptop, eve, "ASSIGNED_TO", {"assigned_date": "2024-09-10"})

    # DHCP leases with temporal validity (epoch milliseconds)
    ip_eve = client.create_node(["IPAddress"], {"address": "10.0.1.50"})
    client.create_edge(ip_eve, laptop, "DHCP_LEASE",
        {"dhcp_server": "10.0.0.1"},
        valid_from=1736928000000,  # 2025-01-15 08:00 UTC
        valid_to=1736935200000,    # 2025-01-15 10:00 UTC
    )

    # Network traffic and firewall alerts
    alert = client.create_node(["FirewallAlert"], {
        "alert_id": "FW-2025-0042", "rule": "MALWARE_DOWNLOAD",
        "severity": "critical",
    })
    client.create_edge(ip_eve, alert, "TRIGGERED", {"timestamp": 1736929800000})
```

**Step 2: Analyst investigates alert FW-2025-0042.**

```python
    # Who triggered this alert?
    sources = client.neighbors(alert_id, "incoming", edge_type="TRIGGERED")
    # -> [{"node_id": <ip_eve>, "edge_id": ...}]

    source_ip = client.get_node(sources[0]["node_id"])
    # -> {"address": "10.0.1.50", "network": "internal"}

    # Trace IP -> Laptop via DHCP lease
    leases = client.neighbors(source_ip_id, "outgoing", edge_type="DHCP_LEASE")
    laptop = client.get_node(leases[0]["node_id"])
    # -> {"hostname": "EVE-LAT01", "brand": "Dell", ...}

    # Trace Laptop -> User
    users = client.neighbors(laptop_id, "outgoing", edge_type="ASSIGNED_TO")
    user = client.get_node(users[0]["node_id"])
    # -> {"name": "Eve", "department": "Marketing", "role": "Analyst"}
```

**Step 3: Pivot -- what else has Eve's IP been doing?**

```python
    # All outbound traffic from 10.0.1.50
    traffic = client.neighbors(source_ip_id, "outgoing", edge_type="TRAFFIC")
    # -> darktools.example.com:443, 10.0.1.20:3389 (RDP), 10.0.1.10:22 (SSH)

    # BFS to see full blast radius (2 hops from Eve's IP)
    blast_radius = client.bfs(source_ip_id, max_depth=2)
```

### Expected Output

```
======================================================================
  Phase 2: Analyst Investigation
======================================================================

[Step 1] Analyst sees alert FW-2025-0042 (MALWARE_DOWNLOAD)
   Alert: Connection to known malware distribution site
   Severity: critical

[Step 2] Who triggered this alert? (follow TRIGGERED edges)
   Source IP: 10.0.1.50

[Step 3] Trace 10.0.1.50 -> Laptop via DHCP_LEASE
   Laptop: EVE-LAT01
   DHCP lease valid: 08:00 - 10:00 UTC

[Step 4] Trace EVE-LAT01 -> User via ASSIGNED_TO
   >>> IDENTIFIED USER: Eve
       Department: Marketing
       Role: Analyst

[Step 5] Pivot: What else has 10.0.1.50 been doing?
   Found 3 outbound traffic connections:
   -> darktools.example.com:443 - Downloaded password_cracker.zip
   -> 10.0.1.20:3389 - RDP connection attempt
   -> 10.0.1.10:22 - SSH connection attempt

[Step 6] Identify targets of lateral movement attempts
   Alert MALWARE_DOWNLOAD (critical): target = darktools.example.com
   Alert LATERAL_MOVEMENT_RDP (high): target = 10.0.1.20 -> BOB-TP01 -> Bob
   Alert UNAUTHORIZED_SSH (high): target = 10.0.1.10 -> ALICE-MBP01 -> Alice

======================================================================
  Investigation Summary
======================================================================

  Alert:   FW-2025-0042 (MALWARE_DOWNLOAD, critical)
  Source:  10.0.1.50
  Laptop:  EVE-LAT01 (Dell Latitude 5540, SN-DEL-3001)
  User:    Eve (Marketing, Analyst)

  Activity from 10.0.1.50:
    1. Downloaded password cracker from darktools.example.com
    2. Attempted RDP to Bob's machine (10.0.1.20, BOB-TP01) - BLOCKED
    3. Attempted SSH to Alice's machine (10.0.1.10, ALICE-MBP01) - BLOCKED

  Recommendation: Isolate EVE-LAT01, revoke Eve's credentials,
  initiate incident response procedure.
```

### Running the Demo

```bash
# Terminal 1: Start the server
cargo run -p astraea-cli -- serve

# Terminal 2: Run the cybersecurity demo
python3 examples/cybersecurity_demo.py
```

### Rust Tests

The same scenario is implemented as 13 Rust tests in the `astraea-graph` crate covering:

- Full investigation chain (alert -> IP -> laptop -> user)
- Temporal validity of DHCP leases
- BFS blast-radius discovery
- Shortest path between attacker and target IPs
- Edge-type filtering for traffic analysis
- Verification that innocent users have clean traffic profiles

```bash
cargo test --package astraea-graph cybersecurity
```

## Implementation Status

### Phase 1 (Foundation) — COMPLETED

All Phase 1 items have been implemented.

| Feature | Status | Description |
|---|---|---|
| **Query Executor** | Done | Full GQL execution: MATCH, CREATE, DELETE, WHERE, ORDER BY, LIMIT, SKIP, DISTINCT. 30 tests. |
| **Pointer Swizzling** | Done | Frequency-based hot page promotion, zero-copy access, eviction prevention. 6 tests. |
| **Label Index** | Done | `HashMap<String, HashSet<NodeId>>` for O(1) label lookups. 5 tests. |
| **MVCC Transactions** | Done | Snapshot isolation, first-writer-wins conflict detection, `TransactionalEngine` trait. 15 tests. |
| **HNSW Persistence** | Done | Versioned binary format with bincode. Save/load without rebuilding. 7 tests. |
| **Cold Tier Storage** | Done | `ColdStorage` trait with 3 backends: `JsonFileColdStorage`, `ParquetColdStorage` (Arrow schema), `ObjectStoreColdStorage` (S3/GCS/Azure). 24 tests. |
| **PageIO Trait** | Done | `PageIO` abstraction with `FileManager` (memmap2) + `UringPageIO` (Linux io_uring, feature-gated). 6 tests. |
| **CLI Commands** | Done | `import`, `export`, `shell` (REPL with rustyline), `status`. |
| **gRPC Transport** | Done | tonic/prost gRPC service with 14 RPCs. 7 tests. |
| **Benchmarks** | Done | 16 criterion benchmarks across storage, vector, and graph crates. |

### Phase 2 (Semantic Layer) — COMPLETED

All Phase 2 items have been implemented.

| Feature | Status | Description |
|---|---|---|
| **Hybrid Search API** | Done | BFS graph proximity + vector distance blended with configurable alpha. 3 tests. |
| **Semantic Traversal** | Done | `semantic_neighbors()` ranks neighbors by embedding distance; `semantic_walk()` greedy multi-hop walk toward a concept. 8 tests. |
| **Vector Server Integration** | Done | `VectorIndex` wired into `Graph` and `RequestHandler`; auto-indexes embeddings on `create_node()`, auto-removes on `delete_node()`. 7 tests. |
| **Apache Arrow Flight** | Done | `astraea-flight` crate: `do_get` (GQL → Arrow RecordBatch streaming), `do_put` (Arrow → bulk node/edge import). 11 tests. |
| **Python Client** | Done | `python/astraeadb` package: `JsonClient` (zero deps), `ArrowClient` (pyarrow.flight), `AstraeaClient` (unified). 23 tests. |

### Phase 3 (GraphRAG Engine) — COMPLETED

All Phase 3 items have been implemented.

| Feature | Status | Description |
|---|---|---|
| **Subgraph Extraction** | Done | BFS-based and semantic (vector-guided) extraction; 4 linearization formats (Prose, Structured, Triples, JSON); token budget estimation. 12 tests. |
| **LLM Integration** | Done | `LlmProvider` trait with Mock/OpenAI/Anthropic/Ollama providers (callback-based, no HTTP deps); `GraphRagConfig` + pipeline; `ExtractSubgraph` and `GraphRag` server requests. 19 tests. |
| **Differentiable Traversal** | Done | `Tensor` type with autograd; message passing layer (Sum/Mean/Max aggregation, ReLU/Sigmoid activation); `train_node_classification()` with numerical gradient descent. 26 tests. |

### Phase 4 (Advanced / Research) — COMPLETED

All Phase 4 items have been implemented. 408 Rust tests pass across the workspace.

| Feature | Status | Description |
|---|---|---|
| **Temporal Queries** | Done | `neighbors_at()`, `bfs_at()`, `shortest_path_at()` filter edges by `ValidityInterval` at a given timestamp. `NeighborsAt`, `BfsAt`, `ShortestPathAt` server requests. 11 tests. |
| **Graph Algorithms** | Done | `astraea-algorithms` crate: PageRank (power iteration), connected/strongly-connected components (Tarjan's), degree/betweenness centrality (Brandes'), Louvain community detection. 20 tests. |
| **Homomorphic Encryption** | Done | `astraea-crypto` crate: key generation, encrypted labels (deterministic tags), encrypted values (randomized), `EncryptedQueryEngine` for server-side label matching. 31 tests. |
| **GPU Acceleration** | Done | `astraea-gpu` crate: CSR matrix with SpMV/transpose, `GpuBackend` trait, `CpuBackend` (PageRank, BFS, SSSP with Bellman-Ford). 16 tests. |
| **Sharding / MPP** | Done | `astraea-cluster` crate: hash/range partitioning, shard map, `ClusterCoordinator` trait with `LocalCoordinator`. 19 tests. |

### Production Readiness — COMPLETED

| Feature | Status | Description |
|---|---|---|
| **Authentication & RBAC** | Done | API key auth with Reader/Writer/Admin roles. `AuthManager` with authenticate/authorize/audit/revoke. Integrated into server request handling. 11 tests. |
| **mTLS** | Done | Full TLS/mTLS support via `TlsConfig`. Server/client cert loading, client CN extraction, CN-to-role mapping. `TlsAcceptor` integration. `rustls` + `tokio-rustls`. 16 tests. |
| **Observability** | Done | `ServerMetrics` with Prometheus text exposition format (request counters, error counters, p50/p90/p99 durations, connection gauges, uptime). Health endpoint. 7 tests. |
| **Connection Management** | Done | `ConnectionManager` with semaphore-based connection limits, request backpressure, idle/request timeouts, graceful shutdown with drain. RAII `ConnectionGuard`. 6 tests. |

## Project Structure

```
astraeadb/
├── Cargo.toml                 # Workspace root
├── proto/
│   └── astraea.proto          # gRPC service definition (14 RPCs)
├── crates/
│   ├── astraea-core/          # Types, traits, errors
│   │   └── src/
│   │       ├── types.rs       # NodeId, EdgeId, Node, Edge, etc.
│   │       ├── traits.rs      # StorageEngine, TransactionalEngine, GraphOps, VectorIndex
│   │       └── error.rs       # AstraeaError enum (incl. WriteConflict, TransactionNotActive)
│   ├── astraea-storage/       # Disk-backed storage
│   │   ├── benches/
│   │   │   └── storage_bench.rs  # 6 criterion benchmarks
│   │   └── src/
│   │       ├── page.rs        # 8 KiB page format, checksums
│   │       ├── page_io.rs     # PageIO trait for pluggable I/O backends
│   │       ├── file_manager.rs# Disk I/O (implements PageIO)
│   │       ├── uring_page_io.rs # Linux io_uring backend (feature-gated)
│   │       ├── buffer_pool.rs # LRU page cache with pointer swizzling
│   │       ├── wal.rs         # Write-ahead log (incl. transaction records)
│   │       ├── label_index.rs # HashMap-based label-to-NodeId index
│   │       ├── mvcc.rs        # MVCC transaction manager (snapshot isolation)
│   │       ├── cold_storage.rs# ColdStorage trait + JsonFileColdStorage
│   │       ├── parquet_cold.rs# ParquetColdStorage (Arrow schema mapping)
│   │       ├── object_store_cold.rs # ObjectStoreColdStorage (S3/GCS/Azure)
│   │       └── engine.rs      # DiskStorageEngine (+ TransactionalEngine impl)
│   ├── astraea-graph/         # Graph operations
│   │   ├── benches/
│   │   │   └── graph_bench.rs # 5 criterion benchmarks
│   │   └── src/
│   │       ├── graph.rs       # Graph struct, CRUD, GraphOps impl
│   │       ├── traversal.rs   # BFS, DFS, Dijkstra
│   │       ├── test_utils.rs  # InMemoryStorage
│   │       └── cybersecurity_test.rs  # Cybersecurity scenario tests
│   ├── astraea-query/         # GQL parser + executor
│   │   └── src/
│   │       ├── token.rs       # Token enum, Span
│   │       ├── lexer.rs       # Tokenizer
│   │       ├── ast.rs         # Statement, Expr, Pattern types
│   │       ├── parser.rs      # Recursive-descent parser
│   │       └── executor.rs    # Full GQL query executor (~1866 lines)
│   ├── astraea-vector/        # Vector index
│   │   ├── benches/
│   │   │   └── vector_bench.rs# 5 criterion benchmarks
│   │   └── src/
│   │       ├── distance.rs    # Cosine, Euclidean, dot product
│   │       ├── hnsw.rs        # HNSW algorithm (Serialize/Deserialize)
│   │       ├── index.rs       # Thread-safe VectorIndex wrapper
│   │       └── persistence.rs # Binary save/load with versioned file format
│   ├── astraea-server/        # Network server
│   │   ├── build.rs           # tonic-build proto compilation
│   │   └── src/
│   │       ├── protocol.rs    # Request/Response JSON types (incl. temporal, hybrid, semantic)
│   │       ├── handler.rs     # Request dispatcher (with GQL executor + VectorIndex)
│   │       ├── grpc.rs        # gRPC service (14 RPCs via tonic)
│   │       ├── auth.rs        # RBAC authentication (Reader/Writer/Admin roles)
│   │       ├── tls.rs         # TLS/mTLS support (rustls, cert loading, CN mapping)
│   │       ├── metrics.rs     # Prometheus metrics + health endpoint
│   │       ├── connection.rs  # Connection limits, backpressure, graceful shutdown
│   │       └── server.rs      # Async TCP/TLS server with auth, metrics, connection mgmt
│   ├── astraea-flight/        # Arrow Flight server
│   │   └── src/
│   │       ├── lib.rs         # Crate root (schemas + service modules)
│   │       ├── schemas.rs     # Arrow schemas for nodes, edges, query results
│   │       └── service.rs     # FlightService impl: do_get (GQL→Arrow), do_put (Arrow→import)
│   ├── astraea-rag/           # GraphRAG engine
│   │   └── src/
│   │       ├── lib.rs         # Crate root
│   │       ├── subgraph.rs    # Subgraph extraction (BFS + semantic)
│   │       ├── linearize.rs   # 4 text formats (Prose, Structured, Triples, Json)
│   │       ├── token.rs       # Token estimation + budget-aware extraction
│   │       ├── llm.rs         # LlmProvider trait + Mock/OpenAI/Anthropic/Ollama
│   │       └── pipeline.rs    # GraphRAG pipeline (vector search → subgraph → LLM)
│   ├── astraea-gnn/           # GNN engine
│   │   └── src/
│   │       ├── lib.rs         # Crate root and public re-exports
│   │       ├── tensor.rs      # Tensor + Matrix with gradient tracking, Xavier init
│   │       ├── model.rs       # GNNModel, GNNLayer, ClassificationHead, forward pass
│   │       ├── backward.rs    # Analytical backpropagation (softmax CE → layers)
│   │       ├── message_passing.rs  # Message passing (Sum/Mean/Max, 6 activations, dropout)
│   │       ├── sparse.rs      # FeatureMatrix, CSRAdjacency, SpMM acceleration
│   │       ├── sampling.rs    # GraphSAGE neighbor sampling for mini-batch training
│   │       ├── temporal.rs    # EvolveGCN temporal GNN with GRU weight evolution
│   │       └── training.rs    # Node classification training (SGD/Adam, early stopping)
│   ├── astraea-algorithms/    # Graph algorithms
│   │   └── src/
│   │       ├── pagerank.rs    # PageRank (power iteration with dangling node handling)
│   │       ├── components.rs  # Connected + strongly-connected components (Tarjan's)
│   │       ├── centrality.rs  # Degree + betweenness centrality (Brandes')
│   │       └── community.rs   # Louvain community detection
│   ├── astraea-crypto/        # Homomorphic encryption
│   │   └── src/
│   │       ├── keys.rs        # SecretKey, PublicKey, KeyPair
│   │       ├── encrypted.rs   # EncryptedValue, EncryptedLabel, EncryptedNode
│   │       └── engine.rs      # EncryptedQueryEngine (server-side label matching)
│   ├── astraea-gpu/           # GPU acceleration
│   │   └── src/
│   │       ├── csr.rs         # CSR sparse matrix (SpMV, transpose)
│   │       ├── backend.rs     # GpuBackend trait, ComputeResult
│   │       └── cpu.rs         # CpuBackend (PageRank, BFS, SSSP fallback)
│   ├── astraea-cluster/       # Distributed processing
│   │   └── src/
│   │       ├── partition.rs   # Hash + Range partitioning strategies
│   │       ├── shard.rs       # ShardId, ShardMap, ShardInfo
│   │       └── coordinator.rs # ClusterCoordinator trait, LocalCoordinator
│   └── astraea-cli/           # CLI binary
│       └── src/
│           └── main.rs        # serve, shell (REPL), status, import, export
├── python/
│   ├── pyproject.toml         # Package config (optional [arrow] extra)
│   ├── astraeadb/
│   │   ├── __init__.py        # Exports AstraeaClient, JsonClient, ArrowClient
│   │   ├── json_client.py     # TCP/JSON client (zero deps)
│   │   ├── arrow_client.py    # Arrow Flight client (pyarrow)
│   │   └── client.py          # Unified client (auto-selects transport)
│   └── tests/
│       └── test_json_client.py # 23 unit tests
├── go/
│   └── astraeadb/             # Go client library
│       ├── go.mod             # Module: github.com/AstraeaDB/AstraeaDB-Official
│       ├── doc.go             # Package documentation
│       ├── types.go           # Node, Edge, SearchResult, etc.
│       ├── errors.go          # Sentinel errors (ErrNodeNotFound, etc.)
│       ├── options.go         # Functional options (WithAddress, WithTLS, etc.)
│       ├── json_client.go     # JSON/TCP client (all 22 operations)
│       ├── grpc_client.go     # gRPC client (14 RPCs with protobuf)
│       ├── arrow_client.go    # Arrow Flight client (stub)
│       ├── client.go          # Unified client (auto-selects transport)
│       ├── Makefile           # proto, test, build, lint targets
│       ├── proto/
│       │   └── astraea.proto  # gRPC service definition
│       ├── pb/astraea/        # Generated protobuf Go code
│       ├── internal/
│       │   ├── protocol/      # NDJSON wire protocol
│       │   └── backoff/       # Exponential backoff with jitter
│       └── examples/
│           ├── basic/         # CRUD, traversal, and GQL demo
│           └── cybersecurity/ # Threat investigation demo
├── java/
│   └── astraeadb/              # Java client library (Gradle multi-module)
│       ├── build.gradle.kts    # Root build (Java 17 toolchain)
│       ├── settings.gradle.kts # Module includes
│       ├── gradle.properties   # Version catalog
│       ├── astraeadb-api/      # Interface, models, exceptions, options
│       │   └── src/main/java/com/astraeadb/
│       │       ├── AstraeaClient.java    # Core interface (22 operations)
│       │       ├── model/                # Records: Node, Edge, PathResult, etc.
│       │       ├── exception/            # AstraeaException hierarchy (7 subclasses)
│       │       └── options/              # EdgeOptions, NeighborOptions, etc.
│       ├── astraeadb-json/     # JSON/TCP client (all 22 operations)
│       ├── astraeadb-grpc/     # gRPC client (14 RPCs, protobuf)
│       ├── astraeadb-flight/   # Arrow Flight client (query + bulk import)
│       ├── astraeadb-unified/  # Auto-transport selection + fallback
│       └── examples/           # BasicExample, VectorSearch, GraphRAG, Cybersecurity
├── examples/
│   ├── python_client.py       # Legacy Python TCP/JSON client
│   ├── cybersecurity_demo.py  # Cybersecurity investigation demo
│   └── r_client.R             # R TCP/JSON client
└── target/                    # Build artifacts
```

## License

MIT
