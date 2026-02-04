# AstraeaDB Implementation Plan

## Overview

AstraeaDB is a Cloud-Native, AI-First Graph Database written in Rust. This plan breaks the vision from `claude.md` into concrete, ordered implementation steps with suggested improvements.

---

## Project Structure

```
astraeadb/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── astraea-core/       # Data types, errors, traits
│   ├── astraea-storage/    # Storage engine, buffer pool, WAL
│   ├── astraea-graph/      # Graph operations, traversals, algorithms
│   ├── astraea-query/      # GQL parser, planner, executor
│   ├── astraea-vector/     # HNSW index, similarity search
│   ├── astraea-server/     # Network protocol, client handling
│   └── astraea-cli/        # CLI tooling
├── benches/                # Benchmarks (Criterion)
├── tests/                  # Integration tests
└── proto/                  # gRPC/Protocol definitions
```

**Improvement:** Use a Cargo workspace with separate crates instead of a monolithic binary. This enforces separation of concerns at compile time, enables parallel compilation, and allows independent versioning.

---

## Phase 1: The Rust Foundation

### Step 1.1 — Project Scaffolding & Core Types
**Crate:** `astraea-core`

- [ ] Convert to a Cargo workspace with the crate structure above
- [ ] Define core error types using `thiserror`
- [ ] Define core data types:
  - `NodeId` — 64-bit unique identifier
  - `EdgeId` — 64-bit unique identifier
  - `Node` — `{ id: NodeId, labels: Vec<String>, properties: serde_json::Value, embedding: Option<Vec<f32>> }`
  - `Edge` — `{ id: EdgeId, source: NodeId, target: NodeId, edge_type: String, properties: serde_json::Value, weight: f64 }`
  - `GraphPath` — ordered sequence of `(NodeId, EdgeId)` pairs for traversal results
- [ ] Define core traits:
  - `StorageEngine` — CRUD trait for persisting nodes/edges
  - `GraphOps` — traversal and query trait
  - `IndexEngine` — trait for indexing strategies
- [ ] Add `serde`, `serde_json`, `thiserror`, `uuid`, `bytes` as dependencies

**Sub-agent: `core-types`** — Can work independently to define and unit-test all core types and traits.

---

### Step 1.2 — Storage Engine: Page-Based Store
**Crate:** `astraea-storage`

This is the most critical subsystem. It implements a custom page-based store optimized for graph topology.

- [ ] **Page Format Design**
  - Fixed-size pages (default 8 KiB, configurable)
  - Page types: `NodePage`, `EdgePage`, `OverflowPage`, `FreelistPage`
  - Each `NodePage` stores a node record with a fixed-size adjacency list header containing direct page offsets to neighbor edge pages (**Index-Free Adjacency**)
  - Overflow pages for nodes with very high degree (>threshold neighbors)

- [ ] **File Manager**
  - Memory-mapped file I/O using `memmap2` crate for the page store
  - Page allocation and deallocation with a freelist
  - File growth strategy (exponential pre-allocation)

- [ ] **Buffer Pool Manager**
  - LRU-based page cache (configurable size)
  - Pin/unpin semantics for pages in active use
  - Dirty page tracking and flush logic
  - This is **Tier 2 (Warm)** from the architecture — the in-memory cache of on-disk pages

- [ ] **Pointer Swizzling Engine (Tier 3 — Hot)**
  - When a page is pinned in the buffer pool, convert 64-bit `PageId` references into raw memory pointers
  - Un-swizzle on eviction
  - This enables O(k) traversal in hot subgraphs without any hash-table lookups

**Improvement:** Start with `memmap2` and standard file I/O instead of `io_uring`. The `io_uring` API is Linux-only; since development is on macOS, use a platform abstraction via `tokio` (which uses `kqueue` on macOS, `io_uring` on Linux when available). Add `io_uring` optimization later as a backend behind a feature flag.

**Sub-agent: `storage-engine`** — Large, self-contained workstream. Can be developed and tested with synthetic graph data independently.

---

### Step 1.3 — Write-Ahead Log (WAL) & Crash Recovery
**Crate:** `astraea-storage`

**Improvement (not in original spec):** A WAL is essential for ACID durability. Without it, a crash during a write can corrupt the database.

- [ ] Append-only WAL file with LSN (Log Sequence Numbers)
- [ ] Log record types: `InsertNode`, `InsertEdge`, `DeleteNode`, `DeleteEdge`, `UpdateProperty`, `Checkpoint`
- [ ] Checkpoint mechanism: flush all dirty pages and write a checkpoint record
- [ ] Recovery: replay WAL from last checkpoint on startup
- [ ] WAL truncation after successful checkpoint
- [ ] `fsync` policy: configurable (every commit, periodic, or OS-managed)

**Sub-agent: `wal-recovery`** — Can be developed once the page store exists. Requires `storage-engine` step 1.2.

---

### Step 1.4 — Transaction Manager (MVCC)
**Crate:** `astraea-storage`

**Improvement (not in original spec):** Multi-Version Concurrency Control is required for concurrent read/write without blocking readers. This is foundational for a production database.

- [ ] Transaction ID assignment (monotonic counter)
- [ ] Version chains on node/edge records (each record carries `created_txn` and `deleted_txn` fields)
- [ ] Snapshot isolation: readers see a consistent snapshot at their start timestamp
- [ ] Commit protocol: validate no write-write conflicts, then append to WAL
- [ ] Garbage collection of old versions beyond the oldest active snapshot

**Sub-agent: `transaction-mvcc`** — Requires `wal-recovery`. Architecturally critical; design carefully.

---

### Step 1.5 — Graph Operations Layer
**Crate:** `astraea-graph`

- [ ] **CRUD Operations**
  - `create_node(labels, properties) -> NodeId`
  - `create_edge(source, target, edge_type, properties) -> EdgeId`
  - `get_node(NodeId) -> Node`
  - `get_edge(EdgeId) -> Edge`
  - `update_node(NodeId, properties)`
  - `delete_node(NodeId)` — cascading edge deletion
  - `delete_edge(EdgeId)`

- [ ] **Traversal Primitives**
  - `neighbors(NodeId, Direction) -> Iterator<(EdgeId, NodeId)>`
  - `neighbors_filtered(NodeId, Direction, edge_type_filter) -> Iterator`
  - BFS traversal with depth limit
  - DFS traversal with depth limit
  - Shortest path (Dijkstra, weighted)
  - All-shortest-paths (BFS-based, unweighted)

- [ ] **Adjacency List Management**
  - Each node's page contains an inline adjacency list (outgoing + incoming edge page offsets)
  - For high-degree nodes (> ~200 edges), spill to overflow pages with a linked-list structure
  - Degree counting without loading full adjacency list

**Sub-agent: `graph-ops`** — Depends on `storage-engine`. Core graph traversal logic; heavily unit-testable.

---

### Step 1.6 — Serialization & Persistence Format
**Crate:** `astraea-storage`

- [ ] **On-disk format:** Custom binary page format (step 1.2) for hot/warm data
- [ ] **Cold storage export:** Apache Parquet via the `parquet` crate
  - Node table: `(node_id, labels, properties_json, embedding_blob)`
  - Edge table: `(edge_id, source_id, target_id, edge_type, properties_json, weight)`
  - Topology table: `(node_id, neighbor_ids[])` — columnar adjacency for bulk analytics
- [ ] **Apache Arrow integration** via `arrow-rs` for zero-copy in-memory representation
- [ ] Import/export between page store and Parquet (bulk load / dump)

**Sub-agent: `serialization`** — Can work in parallel with `graph-ops` once core types exist.

---

### Step 1.7 — Network Server & Protocol
**Crate:** `astraea-server`

- [ ] TCP server using `tokio` async runtime
- [ ] Wire protocol: length-prefixed binary frames
  - Option A: Custom binary protocol (compact, fast)
  - Option B: gRPC via `tonic` (ecosystem compatibility, streaming support) — **Recommended**
- [ ] Request/response types: `Query`, `Mutate`, `BulkLoad`, `Status`
- [ ] Connection pooling and session management
- [ ] Authentication placeholder (API key or mTLS)
- [ ] Health check and metrics endpoint (Prometheus-compatible)

**Sub-agent: `network-server`** — Independent of storage internals; depends only on `astraea-core` traits.

---

### Step 1.8 — CLI & Configuration
**Crate:** `astraea-cli`

- [ ] CLI using `clap`:
  - `astraeadb serve --config <path>` — start the server
  - `astraeadb import --format parquet <file>` — bulk load
  - `astraeadb export --format parquet <file>` — dump
  - `astraeadb shell` — interactive query REPL
- [ ] Configuration via TOML file (`config.toml`):
  - Storage: page size, buffer pool size, data directory
  - Server: bind address, port, max connections
  - WAL: fsync policy, checkpoint interval
  - Logging: level, format

**Sub-agent: `cli-config`** — Light workstream, can run late in Phase 1.

---

### Step 1.9 — Testing & Benchmarking Infrastructure

- [ ] Unit tests in each crate (standard `#[test]` modules)
- [ ] Integration tests in `tests/`:
  - Spin up an embedded database, run CRUD, verify results
  - Crash recovery tests (write, kill, recover, verify)
  - Concurrent read/write tests
- [ ] Property-based tests using `proptest`:
  - Random graph generation → insert → verify traversal consistency
- [ ] Benchmarks using `criterion`:
  - Single-hop traversal latency (target: <1 us hot, <100 us warm)
  - BFS to depth 3 on power-law graph (10K nodes)
  - Bulk insert throughput (nodes/sec, edges/sec)
- [ ] CI pipeline: `cargo test`, `cargo clippy`, `cargo fmt --check`

**Sub-agent: `testing-infra`** — Parallel workstream from day one. Write tests as each component lands.

---

## Phase 2: The Semantic Layer (Vector Integration)

### Step 2.1 — Embedding Storage
**Crate:** `astraea-core` + `astraea-storage`

- [ ] Extend `Node` to carry a fixed-dimension `Vec<f32>` embedding
- [ ] Dedicated `EmbeddingPage` type for storing dense vectors contiguously (cache-friendly)
- [ ] Dimension validation: all embeddings in a graph must share the same dimensionality (configurable at DB creation)
- [ ] Bulk embedding import from NumPy `.npy` or Arrow arrays

---

### Step 2.2 — HNSW Index
**Crate:** `astraea-vector`

- [ ] Implement Hierarchical Navigable Small World (HNSW) graph index
  - Or integrate the `usearch` crate (Rust bindings) initially, then replace with custom implementation
- [ ] Distance metrics: cosine similarity, L2 (Euclidean), dot product
- [ ] Configurable parameters: `M` (max connections per layer), `ef_construction`, `ef_search`
- [ ] Persistence: serialize HNSW layers to dedicated pages in the storage engine
- [ ] Incremental updates: insert/delete vectors without full rebuild

**Key Design Decision:** The HNSW navigation graph edges should be stored *as graph edges* in the main graph. This unifies the vector index with the property graph — "Semantic Traversal" as described in the spec.

**Sub-agent: `vector-index`** — Algorithmically complex, self-contained. Can be developed and benchmarked independently against standard ANN benchmark datasets (e.g., SIFT1M, GloVe).

---

### Step 2.3 — Hybrid Search API

- [ ] `vector_search(query_embedding, k, filter?) -> Vec<(NodeId, f32)>`
  - Pre-filter: apply label/property filters, then search HNSW
  - Post-filter: search HNSW, then filter results
- [ ] Hybrid query: combine vector similarity score with graph distance score
  - `hybrid_search(query_embedding, anchor_node, k, alpha)` — `alpha` blends vector vs. graph proximity
- [ ] GQL integration: `CALL db.index.vector.search(...)` as a built-in procedure

**Sub-agent: `hybrid-search`** — Requires `vector-index` + `graph-ops`.

---

### Step 2.4 — Temporal Graph Support
**Crate:** `astraea-graph`

- [ ] Extend `Edge` with `valid_from: Option<i64>` and `valid_to: Option<i64>` (epoch milliseconds)
- [ ] Time-windowed traversals: `neighbors_at(NodeId, timestamp)` filters edges by validity
- [ ] Persistent versioning using copy-on-write B-tree for temporal edge indices
- [ ] Query syntax: `MATCH (a)-[r]->(b) WHERE r.valid_at(timestamp("2024-01-01"))`
- [ ] Snapshot queries: "show me the graph as of time T" without full data duplication

**Sub-agent: `temporal-graph`** — Can be developed alongside `graph-ops` as an extension.

---

## Phase 3: Query Engine & GraphRAG

### Step 3.1 — GQL Parser
**Crate:** `astraea-query`

- [ ] Lexer/tokenizer for GQL syntax (ISO/IEC 39075)
- [ ] Parser using `nom` or `pest` or `lalrpop` crate
  - Start with a subset: `MATCH`, `WHERE`, `RETURN`, `CREATE`, `DELETE`, `SET`
  - Pattern matching: `(a:Person)-[:KNOWS]->(b:Person)`
  - Property filters: `WHERE a.age > 30`
  - Aggregations: `COUNT`, `SUM`, `AVG`, `COLLECT`
- [ ] AST (Abstract Syntax Tree) representation for parsed queries
- [ ] Pretty-print and error reporting with source spans

**Sub-agent: `gql-parser`** — Pure parsing logic with zero storage dependencies. Can be developed fully in parallel from day one.

---

### Step 3.2 — Query Planner & Optimizer

- [ ] Logical plan: convert AST to relational-style algebra (Scan, Filter, Expand, Project, Aggregate)
- [ ] Physical plan: map logical operators to storage engine calls
- [ ] Optimizations:
  - Predicate pushdown (filter before expand)
  - Join ordering based on estimated cardinality
  - Index selection (label index, property index, vector index)
- [ ] `EXPLAIN` output showing the plan tree

---

### Step 3.3 — Query Executor

- [ ] Volcano-style pull iterator model
- [ ] Operators: `NodeScan`, `EdgeExpand`, `Filter`, `Project`, `Aggregate`, `Sort`, `Limit`
- [ ] Result serialization: rows of `(Map<String, Value>)` or Arrow RecordBatches
- [ ] Parameterized queries (prevent injection, enable plan caching)

---

### Step 3.4 — GraphRAG Pipeline
**Crate:** `astraea-graph` + new `astraea-rag` crate

- [ ] Subgraph extraction: given a seed node, extract the k-hop neighborhood
- [ ] Subgraph linearization: convert subgraph to natural language text
  - Template-based: `"Node {name} is connected to {neighbor} via {edge_type}"`
  - Structured: output as JSON-LD or RDF triples
- [ ] LLM integration via HTTP API (OpenAI-compatible endpoint)
  - `rag_query(question, k_hops, top_k_vectors)` → retrieves context → calls LLM → returns answer
- [ ] Streaming response support

**Sub-agent: `graphrag`** — Depends on working graph + vector search. Late-phase work.

---

## Phase 4: Advanced Features (Post-MVP)

### Step 4.1 — Graph Algorithms Library

- [ ] PageRank (power iteration)
- [ ] Community detection (Louvain, Label Propagation)
- [ ] Connected components (Union-Find)
- [ ] Centrality measures (betweenness, closeness)
- [ ] Triangle counting
- [ ] Expose as built-in GQL procedures: `CALL algo.pageRank(config)`

### Step 4.2 — Differentiable Traversal (GNN Training Loop)

- [ ] Edge weights as differentiable parameters
- [ ] Forward pass: execute a GQL traversal, aggregate neighbor features (mean/sum/attention)
- [ ] Loss computation against ground truth labels
- [ ] Backward pass: compute gradients w.r.t. edge weights
- [ ] SGD/Adam optimizer to update weights in-place
- [ ] Integration point: PyTorch/JAX via Arrow Flight for gradient exchange

### Step 4.3 — Distributed / MPP (Future)

- [ ] Hash-based graph partitioning (vertex-cut or edge-cut)
- [ ] Raft consensus for metadata coordination
- [ ] Cross-shard traversal protocol
- [ ] Bulk Synchronous Parallel (BSP) model for distributed graph algorithms

### Step 4.4 — Homomorphic Encryption (Research)

- [ ] Integrate `tfhe-rs` (Rust-native FHE library) or SEAL bindings
- [ ] Encrypted label matching
- [ ] Encrypted property comparison (equality, range)
- [ ] Performance characterization and feasibility study

### Step 4.5 — GPU Acceleration (Research)

- [ ] CUDA kernel integration behind feature flag
- [ ] CSR (Compressed Sparse Row) adjacency matrix export
- [ ] GPU-accelerated PageRank, BFS, SSSP
- [ ] Investigate `cuGraph` FFI or `wgpu` compute shaders for cross-platform

---

## Suggested Improvements Over Original Spec

| Area | Original Spec | Improvement |
|------|--------------|-------------|
| **Durability** | Not mentioned | Add WAL + crash recovery (Step 1.3). Without this, any crash can corrupt data. Non-negotiable for a production database. |
| **Concurrency** | Not mentioned | Add MVCC transactions (Step 1.4). Required for concurrent readers/writers. |
| **I/O Layer** | `io_uring` | Start with `tokio` + `memmap2` for cross-platform support (macOS dev). Add `io_uring` backend behind a feature flag for Linux deployments (Step 1.2). |
| **Parquet usage** | Cold storage format | Also use as bulk import/export format and analytics format. Enables interop with the entire Arrow/Parquet ecosystem (DuckDB, Spark, Polars) out of the box. |
| **HNSW ownership** | Separate vector index | Unify HNSW navigation edges with graph edges. This is the key differentiator — no other database does this. |
| **Testing** | Not mentioned | Property-based testing with `proptest`, crash injection tests, ANN benchmark suite. Essential for correctness in a storage engine. |
| **GQL scope** | Full ISO GQL | Start with a practical subset (MATCH/WHERE/RETURN/CREATE/DELETE). Full ISO GQL is enormous; shipping a useful subset fast is better than shipping nothing for months. |
| **Encryption** | Microsoft SEAL | Use `tfhe-rs` instead — it's a pure Rust FHE library, avoiding C++ FFI complexity and aligning with the Rust-native philosophy. |
| **Network** | Not detailed | gRPC via `tonic` gives streaming, schema evolution, and ecosystem compatibility (every language has a gRPC client). |
| **Temporal Graphs** | Phase 4 (research) | Move to Phase 2 (Step 2.4). Temporal support is a strong differentiator and architecturally simpler to add early (just edge metadata + filtered traversals). |

---

## Dependency Map (What Blocks What)

```
Step 1.1 (Core Types)
  ├── Step 1.2 (Storage Engine)
  │     ├── Step 1.3 (WAL)
  │     │     └── Step 1.4 (MVCC)
  │     ├── Step 1.5 (Graph Ops)
  │     │     ├── Step 2.4 (Temporal)
  │     │     ├── Step 3.2 (Query Planner)
  │     │     └── Step 3.4 (GraphRAG)
  │     └── Step 1.6 (Serialization)
  ├── Step 2.1 (Embedding Storage)
  │     └── Step 2.2 (HNSW Index)
  │           └── Step 2.3 (Hybrid Search)
  ├── Step 1.7 (Network Server) [independent]
  └── Step 3.1 (GQL Parser) [independent]
        └── Step 3.2 (Query Planner)
              └── Step 3.3 (Query Executor)

Step 1.8 (CLI) — depends on 1.7
Step 1.9 (Testing) — continuous, parallel to everything
```

---

## Sub-Agent Summary

These are the recommended parallel workstreams, each suitable for an independent sub-agent:

| Sub-Agent | Scope | Dependencies | Can Start |
|-----------|-------|-------------|-----------|
| **`core-types`** | Step 1.1: types, traits, errors | None | Immediately |
| **`gql-parser`** | Step 3.1: lexer, parser, AST | None | Immediately |
| **`storage-engine`** | Steps 1.2-1.3: pages, buffer pool, WAL | `core-types` | After 1.1 |
| **`graph-ops`** | Step 1.5: CRUD, traversals | `storage-engine` | After 1.2 |
| **`transaction-mvcc`** | Step 1.4: MVCC, snapshots | `storage-engine` | After 1.3 |
| **`serialization`** | Step 1.6: Parquet, Arrow | `core-types` | After 1.1 |
| **`network-server`** | Step 1.7: gRPC server | `core-types` | After 1.1 |
| **`vector-index`** | Step 2.2: HNSW implementation | `core-types` | After 1.1 |
| **`temporal-graph`** | Step 2.4: time-windowed edges | `graph-ops` | After 1.5 |
| **`hybrid-search`** | Step 2.3: combined vector+graph | `vector-index` + `graph-ops` | After 2.2 + 1.5 |
| **`query-engine`** | Steps 3.2-3.3: planner, executor | `gql-parser` + `graph-ops` | After 3.1 + 1.5 |
| **`graphrag`** | Step 3.4: RAG pipeline | `graph-ops` + `vector-index` | After 1.5 + 2.2 |
| **`testing-infra`** | Step 1.9: test harness, benchmarks | None | Immediately |
| **`cli-config`** | Step 1.8: CLI, config loading | `network-server` | After 1.7 |

---

## Recommended Initial Crate Dependencies

```toml
# astraea-core
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uuid = { version = "1", features = ["v4"] }
bytes = "1"

# astraea-storage
memmap2 = "0.9"
parking_lot = "0.12"
crossbeam = "0.8"
crc32fast = "1"          # WAL checksums

# astraea-graph
dashmap = "6"            # concurrent hash maps
smallvec = "1"           # inline adjacency lists

# astraea-query
lalrpop = "0.21"         # parser generator (or pest/nom)

# astraea-vector
rand = "0.8"             # HNSW construction randomness
ordered-float = "4"      # NaN-safe float comparisons

# astraea-server
tokio = { version = "1", features = ["full"] }
tonic = "0.12"           # gRPC
prost = "0.13"           # protobuf

# astraea-cli
clap = { version = "4", features = ["derive"] }
toml = "0.8"

# dev-dependencies (all crates)
criterion = "0.5"
proptest = "1"
tempfile = "3"
tracing = "0.1"
tracing-subscriber = "0.3"
```

---

## Getting Started: First 5 Steps

1. **Scaffold the workspace** — Convert to a Cargo workspace with the 7 crates
2. **Define core types** — `NodeId`, `EdgeId`, `Node`, `Edge`, traits in `astraea-core`
3. **Build the page store** — Fixed-size pages, file manager, buffer pool in `astraea-storage`
4. **Implement graph CRUD** — Create/read/update/delete nodes and edges in `astraea-graph`
5. **Write traversal tests** — BFS/DFS on small synthetic graphs to validate index-free adjacency
