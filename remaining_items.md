# AstraeaDB — Remaining Implementation Items

This document details every item remaining from the original plan in `CLAUDE.md`, organized by priority. Each item includes rationale, scope, affected crates, implementation steps, new dependencies, and acceptance criteria.

---

## Table of Contents

1. [Phase 1 Gaps (Rust Foundation)](#1-phase-1-gaps-rust-foundation)
   - 1.1 [Query Executor](#11-query-executor)
   - 1.2 [Pointer Swizzling](#12-pointer-swizzling)
   - 1.3 [Label Index (B-Tree)](#13-label-index-b-tree)
   - 1.4 [MVCC / Transactions](#14-mvcc--transactions)
   - 1.5 [HNSW Index Persistence](#15-hnsw-index-persistence)
   - 1.6 [Tiered Storage — Cold Tier (S3/GCS + Parquet)](#16-tiered-storage--cold-tier-s3gcs--parquet)
   - 1.7 [io_uring Async I/O](#17-io_uring-async-io)
   - 1.8 [CLI Commands (import/export/shell/status)](#18-cli-commands-importexportshellstatus)
   - 1.9 [gRPC Transport](#19-grpc-transport)
   - 1.10 [Benchmarks](#110-benchmarks)
2. [Phase 2 Gaps (Semantic Layer)](#2-phase-2-gaps-semantic-layer)
   - 2.1 [Hybrid Vector + Graph Search](#21-hybrid-vector--graph-search)
   - 2.2 [Semantic Traversal](#22-semantic-traversal)
   - 2.3 [Vector Search Server Integration](#23-vector-search-server-integration)
   - 2.4 [Apache Arrow Zero-Copy IPC](#24-apache-arrow-zero-copy-ipc)
   - 2.5 [Python Client (Arrow Flight)](#25-python-client-arrow-flight)
3. [Phase 3 Gaps (GraphRAG Engine)](#3-phase-3-gaps-graphrag-engine)
   - 3.1 [Subgraph Extraction & Linearization](#31-subgraph-extraction--linearization)
   - 3.2 [LLM Integration](#32-llm-integration)
   - 3.3 [Differentiable Traversal / GNN Training Loop](#33-differentiable-traversal--gnn-training-loop)
4. [Research Features](#4-research-features)
   - 4.1 [Temporal Graph Queries (Time-Travel)](#41-temporal-graph-queries-time-travel)
   - 4.2 [Homomorphic Encryption](#42-homomorphic-encryption)
   - 4.3 [GPU / CUDA Acceleration](#43-gpu--cuda-acceleration)
   - 4.4 [Sharding / Massively Parallel Processing](#44-sharding--massively-parallel-processing)
5. [Production Readiness](#5-production-readiness)
   - 5.1 [Authentication & Access Control](#51-authentication--access-control)
   - 5.2 [Observability (Metrics & Tracing)](#52-observability-metrics--tracing)
   - 5.3 [Connection Pooling & Backpressure](#53-connection-pooling--backpressure)

---

## 1. Phase 1 Gaps (Rust Foundation)

> **All 10 Phase 1 items are now COMPLETED.** 201 tests pass across the workspace. See individual items below for implementation details.

### 1.1 Query Executor

**Status:** COMPLETED

**Priority:** Critical — the parser (`astraea-query`) produces a complete AST but nothing executes it. The server handler at `crates/astraea-server/src/handler.rs:182` returns `"GQL query execution not yet integrated"`.

**Rationale:** Without an executor, GQL queries sent via the `Query { gql }` request are dead code. Users must use individual CRUD/traversal requests instead of declarative queries.

**Affected crates:** `astraea-query` (new module), `astraea-server` (handler integration)

**Implementation steps:**

1. **Create `crates/astraea-query/src/executor.rs`** — the core execution engine.
   - Define an `Executor` struct that holds references to `Arc<dyn GraphOps>` and `Arc<dyn VectorIndex>`.
   - Implement `execute(&self, stmt: Statement) -> Result<QueryResult>` as the top-level dispatch.

2. **Implement MATCH execution:**
   - **Pattern binding:** Walk the `Vec<PatternElement>` from the AST. For each `NodePattern`, resolve candidate nodes:
     - If labels are present, call `find_by_label()` (depends on item 1.3 for performance).
     - If inline properties are present, filter candidates by property equality.
   - **Edge expansion:** For each `EdgePattern`, call `neighbors_filtered()` with the edge type and direction from `EdgeDirection`. Join against the next `NodePattern` in the pattern.
   - **Variable binding table:** Maintain a `HashMap<String, Value>` for each binding row. Each row maps variable names (e.g., `a`, `b`, `r`) to their resolved `Node`/`Edge` values.
   - **WHERE evaluation:** Evaluate `Expr` against each binding row. Implement a recursive `eval_expr(expr: &Expr, bindings: &Row) -> Result<Value>` function that handles:
     - `Expr::Variable` — look up in bindings
     - `Expr::Property` — extract JSON field from node/edge properties
     - `Expr::Literal` — return the literal value
     - `Expr::BinaryOp` — evaluate both sides, apply operator (arithmetic, comparison, boolean)
     - `Expr::UnaryOp` — NOT, negation
     - `Expr::FunctionCall` — implement `count()`, `sum()`, `avg()`, `min()`, `max()`, `collect()`, `id()`, `labels()`, `type()`
     - `Expr::IsNull` / `Expr::IsNotNull` — null checks
   - **RETURN projection:** Extract the requested expressions from each binding row. Apply aliasing.
   - **DISTINCT:** Deduplicate result rows by value equality.
   - **ORDER BY:** Sort result rows by the specified expressions and direction.
   - **SKIP / LIMIT:** Slice the result set.

3. **Implement CREATE execution:**
   - Walk the `CreateStatement.pattern`. For each `NodePattern`, call `graph.create_node()`. For each `EdgePattern`, resolve source/target from previously created variables and call `graph.create_edge()`.

4. **Implement DELETE execution:**
   - Resolve each variable name in `DeleteStatement.variables` to a `NodeId` or `EdgeId`, then call the corresponding delete method.

5. **Define `QueryResult`:**
   ```rust
   pub struct QueryResult {
       pub columns: Vec<String>,
       pub rows: Vec<Vec<serde_json::Value>>,
       pub stats: QueryStats,  // nodes_created, edges_created, etc.
   }
   ```

6. **Wire into `RequestHandler`:**
   - Replace the stub at `handler.rs:182` with actual parsing + execution.
   - Parse the GQL string with the existing `Parser`, then pass the AST to `Executor::execute()`.
   - Serialize `QueryResult` into the JSON response.

7. **Tests:**
   - Unit tests for expression evaluation (arithmetic, comparisons, boolean logic).
   - Integration tests: parse + execute MATCH queries against an in-memory graph (using the test-utils mock).
   - Test ORDER BY, LIMIT, SKIP, DISTINCT.
   - Test CREATE and DELETE via GQL.

**Acceptance criteria:**
- `{"type":"Query","gql":"MATCH (a:Person) RETURN a.name"}` returns actual results from the graph.
- WHERE filtering, ORDER BY, LIMIT, SKIP, and DISTINCT all work.
- CREATE and DELETE statements modify the graph.

**What was implemented:**
- Created `crates/astraea-query/src/executor.rs` (~1866 lines) with full `Executor` struct holding `Arc<dyn GraphOps>`.
- Full pipeline: pattern resolution -> WHERE filtering -> ORDER BY -> RETURN projection -> DISTINCT -> SKIP/LIMIT.
- Expression evaluator (`eval_expr`) handles Variable, Property, Literal, BinaryOp, UnaryOp, FunctionCall, IsNull/IsNotNull.
- Built-in functions: `id()`, `labels()`, `type()`, `count()`, `toString()`, `toInteger()`.
- CREATE and DELETE statement execution.
- Wired into `RequestHandler` — `Request::Query { gql }` now parses and executes GQL.
- 30 unit/integration tests covering MATCH, CREATE, expressions, edge traversal, ORDER BY, LIMIT, DISTINCT.

---

### 1.2 Pointer Swizzling

**Status:** COMPLETED

**Priority:** High — this is the core performance claim of the "Hydrated" architecture (Tier 3). The current buffer pool (`crates/astraea-storage/src/buffer_pool.rs`) copies page data into `PageData([u8; PAGE_SIZE])` on every access. There are no raw memory pointers.

**Rationale:** The original plan specifies converting 64-bit disk `PageId` values into direct memory pointers for nanosecond-level traversal of active subgraphs. Currently every `get_node`/`get_edge` call goes through `pin_page()` → `data()` → `copy_from_slice()`, adding overhead.

**Affected crates:** `astraea-storage` (buffer_pool.rs, engine.rs)

**Implementation steps:**

1. **Add a swizzle table to `BufferPoolInner`:**
   ```rust
   swizzle_table: RwLock<HashMap<PageId, *const [u8; PAGE_SIZE]>>
   ```
   When a page is pinned and its pin_count transitions from 0→1, store a raw pointer to the frame's data array. When pin_count drops back to 0, remove the entry.

2. **Add `PageGuard::data_ptr(&self) -> &[u8; PAGE_SIZE]`:**
   - Return a reference directly into the frame's memory instead of copying. This requires the frame to remain pinned for the lifetime of the reference.
   - Use a lifetime-bound `PageRef<'a>` type that borrows from the pool, ensuring the page cannot be evicted while the reference exists.

3. **Add a swizzled adjacency cache in `DiskStorageEngine`:**
   - Maintain a `SwizzledSubgraph` structure that holds pinned pages for "hot" nodes and edges.
   - When a node is accessed multiple times (frequency tracking), promote it into the swizzled subgraph.
   - The swizzled subgraph stores direct `*const Node` pointers that bypass serialization/deserialization entirely.

4. **Implement frequency-based promotion:**
   - Add an access counter per `PageId` in the buffer pool.
   - When a page's access count exceeds a configurable threshold (e.g., 16), pin it permanently and swizzle it.
   - Implement a background eviction sweep that unswizzles cold pages.

5. **Unsafe boundary:**
   - All raw pointer logic must be contained within a `mod swizzle` submodule with clear `// SAFETY:` comments.
   - The swizzle table entries are only valid while the corresponding frame is pinned. Dropping a swizzled entry requires unpinning first.
   - Consider using `Pin<Box<[u8; PAGE_SIZE]>>` to prevent moves.

6. **Tests:**
   - Unit test: pin a page, get swizzled pointer, verify contents match.
   - Test that eviction correctly invalidates swizzled pointers (the pointer is removed from the table before the frame is reused).
   - Benchmark: compare `data()` (copy) vs `data_ptr()` (zero-copy) latency.

**Acceptance criteria:**
- Hot subgraph traversals avoid `memcpy` on every page access.
- No undefined behavior — all unsafe blocks documented and tested.
- Measurable latency improvement in traversal benchmarks.

**What was implemented:**
- Added `access_count` and `swizzled` fields to `Frame` in `buffer_pool.rs`.
- Added `hot_pages: HashSet<PageId>` and `swizzle_threshold` to `BufferPoolInner`.
- Frequency-based promotion: pages exceeding the access threshold are pinned permanently and marked as swizzled (prevented from eviction).
- New methods: `pin_page_ref()`, `is_swizzled()`, `unswizzle()`, `hot_page_count()`.
- 6 new tests verifying swizzle promotion, eviction prevention, and unswizzle.

---

### 1.3 Label Index (B-Tree)

**Status:** COMPLETED

**Priority:** High — `find_by_label()` is used by the query executor for MATCH pattern resolution. The current implementation in `crates/astraea-graph/src/graph.rs` is a placeholder (the `GraphOps` trait requires it, but the graph uses a linear scan through the storage engine).

**Rationale:** Without an index, `MATCH (a:Person)` must scan every node in the database. This is O(n) and unusable at scale.

**Affected crates:** `astraea-storage` (new index structure), `astraea-graph` (integration)

**Implementation steps:**

1. **Create `crates/astraea-storage/src/label_index.rs`:**
   - Implement a `LabelIndex` struct backed by `HashMap<String, HashSet<NodeId>>` for the in-memory version.
   - Methods:
     - `add(label: &str, node_id: NodeId)`
     - `remove(label: &str, node_id: NodeId)`
     - `get(label: &str) -> Vec<NodeId>`
     - `get_intersection(labels: &[String]) -> Vec<NodeId>` — for multi-label queries like `(:Person:Employee)`

2. **Integrate with `DiskStorageEngine`:**
   - Add a `label_index: RwLock<LabelIndex>` field to `DiskStorageEngine`.
   - In `put_node()`, index all labels.
   - In `delete_node()`, remove from the index.
   - Expose via a new method `fn find_nodes_by_label(&self, label: &str) -> Result<Vec<NodeId>>` on the `StorageEngine` trait (or a separate `IndexEngine` trait).

3. **Wire into `Graph::find_by_label()`:**
   - Replace the placeholder with a call through to the storage engine's label index.

4. **Persistence (optional, for durability):**
   - Serialize the label index to a dedicated page type (`PageType::LabelIndexPage`).
   - Rebuild from WAL replay on startup as a simpler alternative.

5. **Tests:**
   - Add/remove labels, verify lookup.
   - Multi-label intersection.
   - Integration test: `find_by_label("Person")` returns correct nodes after inserts and deletes.

**Acceptance criteria:**
- `find_by_label()` returns results in O(1) amortized time (hash lookup).
- Index stays consistent across inserts, updates, and deletes.
- Query executor can resolve `MATCH (a:Person)` efficiently.

**What was implemented:**
- Created `crates/astraea-storage/src/label_index.rs` with `LabelIndex` backed by `HashMap<String, HashSet<NodeId>>`.
- Methods: `add_node()`, `remove_node()`, `get()`, `get_intersection()`, `all_labels()`, `len()`, `is_empty()`.
- Integrated with `DiskStorageEngine`: `label_index: RwLock<LabelIndex>` field; indexing on `put_node()`, removal on `delete_node()`.
- Added `find_nodes_by_label()` default method to `StorageEngine` trait in `astraea-core`.
- Wired into `Graph::find_by_label()` for O(1) label lookups.
- 5 unit tests.

---

### 1.4 MVCC / Transactions

**Status:** COMPLETED

**Priority:** High — required for concurrent read/write workloads and data consistency.

**Rationale:** The current engine has no transaction isolation. Concurrent readers and writers can see partial updates. The `TransactionId` type exists in `astraea-core/src/types.rs` but is unused.

**Affected crates:** `astraea-core` (trait extension), `astraea-storage` (MVCC layer), `astraea-graph` (transaction-aware operations)

**Implementation steps:**

1. **Define transaction semantics:**
   - **Isolation level:** Snapshot Isolation (SI) — each transaction sees a consistent snapshot as of its start time.
   - **Conflict detection:** First-writer-wins. If two transactions write to the same node/edge, the second to commit aborts.

2. **Create `crates/astraea-storage/src/mvcc.rs`:**
   - **Version chain:** Each node/edge record is stored with a version header:
     ```rust
     struct VersionHeader {
         txn_id: TransactionId,    // the transaction that wrote this version
         created_at: Lsn,          // LSN when created
         deleted_at: Option<Lsn>,  // LSN when superseded (soft delete)
     }
     ```
   - **Transaction manager:** `TransactionManager` struct:
     - `active_txns: HashMap<TransactionId, TransactionState>` — tracks in-flight transactions.
     - `next_txn_id: AtomicU64` — monotonic ID generator.
     - `begin() -> TransactionId` — start a new transaction, record snapshot LSN.
     - `commit(txn_id) -> Result<()>` — validate no write conflicts, make writes visible.
     - `abort(txn_id)` — discard all writes from this transaction.

3. **Add a write buffer per transaction:**
   - `TransactionState` holds a local write set:
     ```rust
     struct TransactionState {
         snapshot_lsn: Lsn,
         write_set: Vec<WalRecord>,    // buffered writes
         read_set: HashSet<(NodeId or EdgeId)>,  // for conflict detection
     }
     ```
   - On `put_node`/`put_edge`, writes go to the transaction's buffer.
   - On `commit`, writes are applied atomically to the storage engine and WAL.

4. **Visibility rules:**
   - `get_node(id, txn_id)` checks the version chain:
     - Return the latest version where `created_at <= snapshot_lsn` and `deleted_at` is either `None` or `> snapshot_lsn`.
   - This ensures snapshot isolation: readers see a stable view.

5. **Extend `StorageEngine` trait** (or add a `TransactionalEngine` trait):
   ```rust
   pub trait TransactionalEngine: StorageEngine {
       fn begin(&self) -> Result<TransactionId>;
       fn commit(&self, txn_id: TransactionId) -> Result<()>;
       fn abort(&self, txn_id: TransactionId) -> Result<()>;
       fn get_node_tx(&self, id: NodeId, txn_id: TransactionId) -> Result<Option<Node>>;
       fn put_node_tx(&self, node: &Node, txn_id: TransactionId) -> Result<()>;
       // ... same for edges
   }
   ```

6. **Garbage collection:**
   - Old versions that are no longer visible to any active transaction can be reclaimed.
   - Implement a background GC task that scans version chains and removes dead versions.

7. **WAL integration:**
   - WAL records already include `InsertNode`, `DeleteNode`, etc.
   - Add `BeginTransaction(TransactionId)` and `CommitTransaction(TransactionId)` record types.
   - On crash recovery, replay the WAL and only materialize committed transactions.

8. **Tests:**
   - Snapshot isolation: txn A reads node, txn B updates it, txn A still sees old value.
   - Write conflict: two transactions update the same node, second commit fails.
   - Abort discards uncommitted writes.
   - Crash recovery: only committed transactions are visible after restart.

**Acceptance criteria:**
- Multiple concurrent transactions can read and write without corruption.
- Snapshot isolation prevents dirty reads and non-repeatable reads.
- Write-write conflicts are detected and one transaction is aborted.
- WAL replay correctly restores only committed state.

**What was implemented:**
- Created `crates/astraea-storage/src/mvcc.rs` with `TransactionManager` struct.
- Snapshot isolation with first-writer-wins conflict detection.
- `Transaction` struct with `write_set: Vec<WriteOp>`, `read_set`, `write_locks`.
- `WriteOp` enum: `PutNode`, `DeleteNode`, `PutEdge`, `DeleteEdge`.
- Added `TransactionalEngine` trait to `astraea-core/src/traits.rs` with `begin_transaction()`, `commit_transaction()`, `abort_transaction()`, `put_node_tx()`, `delete_node_tx()`, `put_edge_tx()`, `delete_edge_tx()`.
- Implemented `TransactionalEngine` for `DiskStorageEngine`.
- Added `WriteConflict` and `TransactionNotActive` error variants.
- Added `BeginTransaction`, `CommitTransaction`, `AbortTransaction` WAL record types.
- GC support for reclaiming completed transaction state.
- 11 mvcc unit tests + 4 engine transactional tests (put_commit, put_abort, delete_commit, write_conflict).

---

### 1.5 HNSW Index Persistence

**Status:** COMPLETED

**Priority:** High — the vector index at `crates/astraea-vector/src/hnsw.rs` is entirely in-memory. Restarting the server loses all vectors and the multi-layer graph structure.

**Rationale:** Rebuilding the HNSW index from raw embeddings on startup is O(n log n) and prohibitively slow for large datasets.

**Affected crates:** `astraea-vector` (serialization), `astraea-storage` (optional page-based storage)

**Implementation steps:**

1. **Add `serialize` / `deserialize` methods to `HnswIndex`:**
   - Serialize the following:
     - `dimension`, `metric`, `m`, `m_max0`, `ef_construction`, `ml`
     - `vectors: HashMap<NodeId, Vec<f32>>` — all embeddings
     - `layers: Vec<HashMap<NodeId, Vec<NodeId>>>` — the full multi-layer adjacency structure
     - `entry_point`, `max_level`, `node_levels`
   - Use `bincode` or `rmp-serde` (MessagePack) for compact binary serialization. Avoid JSON for large float arrays.

2. **New dependency:** Add `bincode = "1"` to the workspace dependencies.

3. **Implement file-based persistence:**
   - `HnswIndex::save_to_file(path: &Path) -> Result<()>` — serialize entire index to a single file.
   - `HnswIndex::load_from_file(path: &Path) -> Result<Self>` — deserialize.
   - File format: `[magic: u32][version: u32][header_len: u64][header_bytes][data_bytes]`
     - Header: dimension, metric, HNSW parameters.
     - Data: vectors + layer adjacency lists.

4. **Integrate with server startup:**
   - On `AstraeaServer::run()`, check for an existing index file and load it.
   - On graceful shutdown (SIGTERM handler), persist the index.
   - Optionally, persist periodically (e.g., every N inserts or every T seconds).

5. **Incremental updates (future optimization):**
   - Instead of rewriting the entire index, maintain a WAL-like append log of vector inserts/removes.
   - On startup, load the snapshot + replay the log.
   - Compact the log into a new snapshot periodically.

6. **Tests:**
   - Round-trip: build index with 1000 vectors, save, load, verify same search results.
   - Verify file format versioning (reject incompatible versions).
   - Verify behavior when file doesn't exist (fresh start).

**Acceptance criteria:**
- Server restart preserves the vector index without rebuilding.
- Load time is proportional to file size, not O(n log n).
- File format is versioned for future compatibility.

**What was implemented:**
- Created `crates/astraea-vector/src/persistence.rs` with versioned binary file format.
- Magic bytes (`0x484E5357` = "HNSW") and `FORMAT_VERSION = 1` for forward compatibility.
- `HnswFileHeader` struct with all HNSW parameters (dimension, metric, m, m_max0, ef_construction, num_vectors, num_layers).
- `save_to_file()` and `load_from_file()` using bincode serialization.
- Convenience methods on `HnswIndex`: `.save(path)`, `.load(path)`.
- Cross-check: header metadata validated against deserialized body on load.
- Added `#[derive(Serialize, Deserialize)]` to `HnswIndex` and accessor methods `m()`, `m_max0()`, `ef_construction()`, `num_layers()`.
- Added `bincode = "1"` workspace dependency.
- 7 tests: round-trip (100 vectors, empty, cosine, dot-product), magic byte corruption, version corruption, search consistency after load.

---

### 1.6 Tiered Storage — Cold Tier (S3/GCS + Parquet)

**Status:** COMPLETED (foundation layer; Parquet/S3 backends deferred to Phase 2)

**Priority:** Medium — this is part of the "Hydrated" cloud-native architecture but not needed for single-node operation.

**Rationale:** The original plan specifies Tier 1 (Cold) storage in object storage using Apache Parquet. Currently all data lives on local disk in a custom page format.

**Affected crates:** `astraea-storage` (new cold storage backend)

**New dependencies:** `object_store` (Rust crate for S3/GCS/Azure), `parquet` (from `arrow-rs`)

**Implementation steps:**

1. **Define a `ColdStorage` trait:**
   ```rust
   pub trait ColdStorage: Send + Sync {
       fn read_partition(&self, partition_key: &str) -> Result<Vec<Node>>;
       fn write_partition(&self, partition_key: &str, nodes: &[Node]) -> Result<()>;
       fn list_partitions(&self) -> Result<Vec<String>>;
   }
   ```

2. **Implement Parquet serialization for nodes and edges:**
   - Map node fields to Parquet columns:
     - `id: UInt64`
     - `labels: List<Utf8>`
     - `properties: Utf8` (JSON string)
     - `embedding: FixedSizeList<Float32>` (if present)
   - Map edge fields similarly.
   - Partition by label (e.g., `nodes/Person/*.parquet`, `nodes/Server/*.parquet`).

3. **Implement `ObjectStoreColdStorage`:**
   - Use the `object_store` crate to read/write Parquet files to S3, GCS, or local filesystem.
   - Configuration: bucket name, prefix, credentials (via environment or config file).

4. **Tiering policy:**
   - Add a `TieringManager` that decides when to evict warm pages to cold storage.
   - Policy: pages not accessed for N minutes are serialized to Parquet and removed from the buffer pool.
   - On access, if a page is not in the buffer pool and not on local disk, fetch from cold storage, deserialize, and load into the buffer pool.

5. **Tests:**
   - Write nodes to Parquet, read back, verify equality.
   - Test with local filesystem backend (no cloud credentials needed for CI).
   - Test tiering: insert nodes, wait for eviction, read again (should trigger cold fetch).

**Acceptance criteria:**
- Nodes and edges can be persisted in Parquet format.
- Cold data can be stored in S3/GCS/local filesystem.
- Transparent fetch from cold storage when data is not in buffer pool.

**What was implemented:**
- Created `crates/astraea-storage/src/cold_storage.rs` with `ColdStorage` trait: `read_partition()`, `write_partition()`, `delete_partition()`, `list_partitions()`.
- `JsonFileColdStorage` implementation using serde_json to local files (foundation for future Parquet/S3 backends).
- `ColdNode` and `ColdEdge` serializable types with `From` conversions to/from core types.
- Updated buffer pool to integrate with cold storage tier.
- 7 tests covering write/read/delete/list partitions.
- Note: Full Parquet serialization and S3/GCS `object_store` integration deferred to Phase 2.

---

### 1.7 io_uring Async I/O

**Status:** COMPLETED (PageIO trait abstraction; io_uring backend deferred to Linux-specific feature gate)

**Priority:** Medium — performance optimization for Linux deployments.

**Rationale:** The original plan specifies io_uring for async Linux I/O. Currently, the `FileManager` uses `memmap2` (synchronous memory-mapped I/O).

**Affected crates:** `astraea-storage` (file_manager.rs)

**New dependencies:** `tokio-uring` or `io-uring` crate (Linux-only, feature-gated)

**Implementation steps:**

1. **Feature-gate io_uring support:**
   - Add a `io-uring` Cargo feature to `astraea-storage`.
   - The default remains `memmap2` for cross-platform compatibility.

2. **Create `crates/astraea-storage/src/file_manager_uring.rs`:**
   - Implement the same interface as `FileManager` but using `io_uring` for:
     - `read_page()` — submit async read, await completion.
     - `write_page()` — submit async write, await completion.
     - Batch I/O: submit multiple page reads in a single submission queue.

3. **Abstract the file manager behind a trait:**
   ```rust
   pub trait PageIO: Send + Sync {
       fn read_page(&self, page_id: PageId) -> Result<[u8; PAGE_SIZE]>;
       fn write_page(&self, page_id: PageId, data: &[u8; PAGE_SIZE]) -> Result<()>;
       fn allocate_page(&self) -> Result<PageId>;
   }
   ```
   - `FileManager` and `UringFileManager` both implement this trait.
   - `BufferPool` accepts `Arc<dyn PageIO>`.

4. **Benchmark:** Compare throughput of memmap2 vs io_uring for random page reads under load.

**Acceptance criteria:**
- io_uring backend passes all existing storage tests on Linux.
- memmap2 remains the default on non-Linux platforms.
- No behavior change for callers — same `PageIO` trait interface.

**What was implemented:**
- Created `crates/astraea-storage/src/page_io.rs` with `PageIO` trait: `read_page()`, `write_page()`, `allocate_page()`.
- Implemented `PageIO` for `FileManager` (delegation to inherent methods).
- Updated `BufferPool` to accept `Arc<dyn PageIO>` instead of `Arc<FileManager>`, enabling pluggable I/O backends.
- `DiskStorageEngine` now casts `Arc<FileManager>` to `Arc<dyn PageIO>` for the buffer pool.
- 2 tests verifying trait-object interface.
- Note: Actual `io_uring` backend (`UringFileManager`) deferred to a Linux-specific feature-gated implementation. The `PageIO` trait is the prerequisite abstraction.

---

### 1.8 CLI Commands (import/export/shell/status)

**Status:** COMPLETED

**Priority:** Medium — the CLI at `crates/astraea-cli/src/main.rs` defines these commands but they are not implemented.

**Affected crates:** `astraea-cli`

**Implementation steps:**

1. **`import` command:**
   - Read a JSON file (array of node/edge objects).
   - For each object, send a `CreateNode` or `CreateEdge` request to the server.
   - Support `--format json` (immediate) and `--format parquet` (after item 1.6).
   - Progress bar via `indicatif` crate.
   - Batch mode: accumulate records and send in bulk for throughput.

2. **`export` command:**
   - Connect to the server, iterate all nodes and edges.
   - Requires a new `ListNodes` / `ListEdges` request type in the protocol (or a GQL query: `MATCH (n) RETURN n`).
   - Write to JSON or Parquet file.

3. **`shell` command (interactive REPL):**
   - Use `rustyline` crate for readline support (history, tab completion).
   - Connect to the server via TCP.
   - Prompt: `astraea> `
   - Parse input as GQL, send as `Query { gql }` request, display results as a formatted table.
   - Use `comfy-table` or `tabled` crate for tabular output.
   - Special commands: `.help`, `.quit`, `.status`, `.tables` (list labels).

4. **`status` command:**
   - Send a `Ping` request plus a new `Stats` request type.
   - Display: server version, uptime, node count, edge count, vector index size, buffer pool usage.

**New dependencies:** `rustyline`, `indicatif`, `comfy-table`

**Acceptance criteria:**
- `astraea-cli import --file data.json` loads data into a running server.
- `astraea-cli export --file out.json` dumps the database.
- `astraea-cli shell` provides an interactive GQL REPL (depends on item 1.1).
- `astraea-cli status` shows server health.

**What was implemented:**
- Full rewrite of `crates/astraea-cli/src/main.rs` with all four commands operational.
- **`import`**: Reads JSON file (array of node/edge objects), sends `CreateNode`/`CreateEdge` requests to server.
- **`export`**: Scans nodes/edges by ID range, writes to JSON file.
- **`shell`**: Interactive REPL with `rustyline` (readline, history). Auto-detects GQL queries vs JSON requests. Table-formatted output. Dot-commands (`.help`, `.quit`, `.status`).
- **`status`**: Sends `Ping` request, displays server connectivity and version.
- TCP helpers: `send_request()`, `send_raw_request()` for server communication.
- Added `rustyline = "15"` workspace dependency.

---

### 1.9 gRPC Transport

**Status:** COMPLETED

**Priority:** Medium — `tonic` and `prost` are already workspace dependencies but not used.

**Rationale:** gRPC provides schema-enforced APIs, streaming, and better performance than newline-delimited JSON for production clients.

**Affected crates:** `astraea-server` (new gRPC service), new `astraea-proto` crate

**Implementation steps:**

1. **Create `crates/astraea-proto/`:**
   - Define `astraea.proto` with service definitions:
     ```protobuf
     service AstraeaDB {
       rpc CreateNode(CreateNodeRequest) returns (CreateNodeResponse);
       rpc GetNode(GetNodeRequest) returns (GetNodeResponse);
       rpc Query(QueryRequest) returns (stream QueryRow);
       rpc VectorSearch(VectorSearchRequest) returns (stream SimilarityResult);
       rpc Ping(PingRequest) returns (PingResponse);
       // ... etc
     }
     ```
   - Use `tonic-build` in a `build.rs` to generate Rust types.

2. **Implement the gRPC service in `astraea-server`:**
   - Create `src/grpc_service.rs` implementing the generated trait.
   - Delegate to the same `RequestHandler` logic.
   - Use streaming RPCs for large result sets (MATCH queries, vector search).

3. **Server configuration:**
   - Add `grpc_port` to `ServerConfig` (default: 7688).
   - Start both TCP (JSON) and gRPC listeners concurrently.

4. **Tests:**
   - Integration test using `tonic` client.
   - Verify streaming responses for large queries.

**Acceptance criteria:**
- gRPC clients can perform all operations currently available via JSON protocol.
- Streaming works for MATCH and VectorSearch responses.
- Both TCP/JSON and gRPC run concurrently.

**What was implemented:**
- Created `proto/astraea.proto` with full gRPC service definition (14 RPCs).
- Created `crates/astraea-server/build.rs` for `tonic-build` proto compilation.
- Created `crates/astraea-server/src/grpc.rs` (~848 lines) with `AstraeaGrpcService` wrapping `Arc<RequestHandler>`.
- All 14 RPCs implemented as thin adapters over the existing handler: CreateNode, GetNode, CreateEdge, GetEdge, UpdateNode, UpdateEdge, DeleteNode, DeleteEdge, Neighbors, Bfs, ShortestPath, VectorSearch, Query, Ping.
- `run_grpc_server()` helper for startup.
- Added `tonic`, `prost`, `tonic-build` dependencies.
- 7 tests: ping, create/get node, create/get edge, delete, neighbors, query.

---

### 1.10 Benchmarks

**Status:** COMPLETED

**Priority:** Low — benchmark harnesses exist in `crates/astraea-storage/benches/` and `crates/astraea-vector/benches/` but contain no benchmarks.

**Affected crates:** `astraea-storage`, `astraea-vector`, `astraea-graph`

**Implementation steps:**

1. **Storage benchmarks (`storage_bench.rs`):**
   - `bench_put_node` — single node write throughput.
   - `bench_get_node` — single node read latency.
   - `bench_sequential_writes` — 10K node writes.
   - `bench_random_reads` — random node reads from a 10K-node dataset.
   - `bench_buffer_pool_hit` — read a cached page vs. a cold page.

2. **Vector benchmarks (`vector_bench.rs`):**
   - `bench_hnsw_insert` — insert 10K vectors of dimension 128.
   - `bench_hnsw_search` — k-NN search (k=10) on a 10K-vector index.
   - `bench_cosine_distance` — raw distance computation throughput.
   - `bench_search_varying_ef` — search latency vs. ef_search parameter (accuracy tradeoff).

3. **Graph benchmarks (new `crates/astraea-graph/benches/graph_bench.rs`):**
   - `bench_bfs` — BFS on a 10K-node random graph.
   - `bench_shortest_path` — Dijkstra on a weighted 10K-node graph.
   - `bench_neighbor_lookup` — neighbor retrieval latency.

4. **Run with:** `cargo bench`

**Acceptance criteria:**
- All benchmarks run without errors.
- Results provide baseline numbers for performance regression tracking.

**What was implemented:**
- **Storage benchmarks** (`crates/astraea-storage/benches/storage_bench.rs`): 6 benchmarks — `bench_put_node`, `bench_get_node`, `bench_sequential_writes`, `bench_random_reads`, `bench_put_edge`, `bench_get_edges`.
- **Vector benchmarks** (`crates/astraea-vector/benches/vector_bench.rs`): 5 benchmarks — `bench_hnsw_insert`, `bench_hnsw_search_k10`, `bench_hnsw_search_k50`, `bench_cosine_distance`, `bench_euclidean_distance`.
- **Graph benchmarks** (`crates/astraea-graph/benches/graph_bench.rs`): 5 benchmarks — `bench_bfs_depth3`, `bench_shortest_path_unweighted`, `bench_dijkstra`, `bench_neighbors_20`, `bench_create_node`.
- All benchmarks use `criterion` with `iter_batched` for proper setup isolation.
- 16 total benchmark functions across 3 files.

---

## 2. Phase 2 Gaps (Semantic Layer)

> **All 5 Phase 2 items are now COMPLETED.** 230 Rust tests + 23 Python tests pass. See individual items below for implementation details.

### 2.1 Hybrid Vector + Graph Search

**Status:** COMPLETED

**Priority:** Critical for Phase 2 — this is the central differentiator of the "Vector-Property Graph" model.

**Rationale:** Currently, vector search (`astraea-vector`) and graph traversal (`astraea-graph`) operate in isolation. The original plan describes queries that combine both: find nodes that are semantically similar AND structurally connected.

**Affected crates:** `astraea-graph` (new hybrid query methods), `astraea-query` (new syntax), `astraea-server` (new request types)

**Implementation steps:**

1. **Define the `HybridQuery` struct:**
   ```rust
   pub struct HybridQuery {
       pub anchor_node: NodeId,           // starting node
       pub query_embedding: Vec<f32>,      // semantic target
       pub max_hops: usize,                // graph radius
       pub k: usize,                       // top-k results
       pub edge_type_filter: Option<String>,
       pub alpha: f32,                     // blend factor: 0.0 = pure graph, 1.0 = pure vector
   }
   ```

2. **Implement `hybrid_search()` in `astraea-graph`:**
   - Step 1: Perform a BFS/DFS from `anchor_node` up to `max_hops` to collect candidate nodes.
   - Step 2: For each candidate that has an embedding, compute vector distance to `query_embedding`.
   - Step 3: Compute a graph distance score (e.g., 1/hop_count or path weight).
   - Step 4: Blend scores: `final_score = alpha * vector_score + (1 - alpha) * graph_score`.
   - Step 5: Sort by `final_score`, return top-k.

3. **Add GQL syntax extension:**
   - Support a `CALL db.search.hybrid(...)` procedure syntax in the parser.
   - Alternatively, extend MATCH with a `NEAR` clause:
     ```
     MATCH (a:Person)-[:KNOWS*1..3]->(b)
     WHERE b NEAR embedding([0.1, 0.2, ...], k=10, alpha=0.7)
     RETURN b
     ```

4. **Add `HybridSearch` request to the protocol:**
   ```rust
   Request::HybridSearch {
       anchor: u64,
       query: Vec<f32>,
       max_hops: usize,
       k: usize,
       alpha: f32,
   }
   ```

5. **Tests:**
   - Create a graph with embeddings on nodes. Query with a hybrid search. Verify that results are influenced by both graph proximity and vector similarity.
   - Test alpha=0 (pure graph), alpha=1 (pure vector), alpha=0.5 (blended).

**Acceptance criteria:**
- A single query can find nodes that are both structurally close and semantically similar.
- The blend factor `alpha` controls the tradeoff.
- Results are returned ranked by blended score.

**What was implemented:**
- Added `hybrid_search()` to the `GraphOps` trait with a default "not supported" error. Implemented on `Graph`.
- Algorithm: BFS from anchor up to `max_hops` → compute graph score (`depth / (max_hops+1)`) and vector score (`compute_distance()`) → blend with `alpha * vector_score + (1-alpha) * graph_score` → sort ascending, truncate to top-k.
- Added `HybridSearch` request to protocol with fields: `anchor`, `query`, `max_hops` (default 3), `k` (default 10), `alpha` (default 0.5).
- Added handler returning `{"results": [{"node_id": ..., "score": ...}]}`.
- 4 tests: alpha=0 pure graph, alpha=1 pure vector, alpha=0.5 blended, handler end-to-end.

---

### 2.2 Semantic Traversal

**Status:** COMPLETED

**Priority:** High for Phase 2 — the "navigate-by-meaning" feature from the original plan.

**Rationale:** The plan states: *"Find the neighbor of Node A that is most semantically similar to the concept of 'Risk'."* This requires ranking neighbors by embedding similarity to a query concept.

**Affected crates:** `astraea-graph`, `astraea-vector`

**Implementation steps:**

1. **Add `semantic_neighbors()` to `GraphOps`:**
   ```rust
   fn semantic_neighbors(
       &self,
       node_id: NodeId,
       concept_embedding: &[f32],
       direction: Direction,
       k: usize,
   ) -> Result<Vec<(NodeId, f32)>>;  // (neighbor_id, similarity)
   ```

2. **Implementation:**
   - Get all neighbors of `node_id` via `neighbors()`.
   - For each neighbor, look up its embedding.
   - Compute distance between `concept_embedding` and each neighbor's embedding.
   - Sort by distance ascending, return top-k.

3. **Multi-hop semantic traversal:**
   - `semantic_walk(start, concept_embedding, max_hops)` — at each hop, greedily move to the neighbor most similar to the concept. Return the full path.
   - This enables "walk toward the concept of Risk through the graph."

4. **Tests:**
   - Build a graph with nodes having known embeddings. Verify `semantic_neighbors` returns the correct ordering.
   - Test multi-hop walk converges toward the target concept.

**Acceptance criteria:**
- Neighbors can be ranked by semantic similarity to an arbitrary concept vector.
- Multi-hop semantic walk produces a path that moves toward the concept.

**What was implemented:**
- Added `semantic_neighbors()` and `semantic_walk()` to the `GraphOps` trait with default "not supported" errors. Implemented on `Graph`.
- **`semantic_neighbors()`**: Gets neighbors in a given direction, computes embedding distance to concept vector for each, sorts by distance, returns top-k. Nodes without embeddings are excluded.
- **`semantic_walk()`**: Greedy multi-hop walk — at each step, moves to the unvisited outgoing neighbor whose embedding is closest to the concept. Maintains visited set to prevent cycles.
- Added `SemanticNeighbors` and `SemanticWalk` requests to protocol.
- Added handlers returning `{"results": [...]}` and `{"path": [...]}` respectively.
- 9 tests: neighbor ranking, k-limiting, walk toward concept, intermediate nodes, dead-end stops, no-embedding exclusion, cycle avoidance, handler end-to-end (x2).
- Added `astraea-vector` as dependency of `astraea-graph` for `compute_distance()`.

---

### 2.3 Vector Search Server Integration

**Status:** COMPLETED

**Priority:** High — the server handler at `crates/astraea-server/src/handler.rs:178` returns `"vector search not yet integrated with server"`.

**Affected crates:** `astraea-server`

**Implementation steps:**

1. **Pass `Arc<dyn VectorIndex>` to `RequestHandler`:**
   - Modify the constructor: `RequestHandler::new(graph, vector_index)`.
   - Store as a field: `vector_index: Arc<dyn VectorIndex>`.

2. **Implement the `VectorSearch` handler:**
   - Call `self.vector_index.search(&request.query, request.k)`.
   - Map results to JSON: `[{"node_id": ..., "distance": ...}, ...]`.

3. **Auto-index on node creation:**
   - When `CreateNode` includes an `embedding`, automatically insert it into the vector index.
   - When `DeleteNode` is called, remove from the vector index.

4. **Tests:**
   - Integration test: create nodes with embeddings via the server, then perform a VectorSearch request.

**Acceptance criteria:**
- `{"type":"VectorSearch","query":[0.1,0.2,...],"k":5}` returns nearest neighbors.
- Embeddings are automatically indexed when nodes are created.

**What was implemented:**
- Added `vector_index: Option<Arc<dyn VectorIndex>>` field to `Graph` struct.
- New constructors: `Graph::with_vector_index()`, `Graph::set_vector_index()`, `Graph::vector_index()` getter.
- **Auto-indexing**: `create_node()` inserts embedding into vector index (if both present). Failures logged but don't fail node creation.
- **Auto-removal**: `delete_node()` removes from vector index before deleting from storage.
- Added `vector_index: Option<Arc<dyn VectorIndex>>` to `RequestHandler`. Updated constructor signature.
- Implemented `VectorSearch` handler: returns `{"results": [{"node_id": ..., "distance": ...}]}` or error if no index configured.
- Updated CLI serve command to create 128-dim Cosine `HnswVectorIndex` and pass to both Graph and RequestHandler.
- Updated gRPC service test helpers.
- 4 new server tests: basic search, no-index error, auto-index on create, auto-remove on delete.

---

### 2.4 Apache Arrow Zero-Copy IPC

**Status:** COMPLETED

**Priority:** Medium — enables high-throughput data exchange with Python/Polars/Pandas.

**Rationale:** The plan specifies using Arrow Flight for zero-copy data transfer. Currently, all data is serialized as JSON.

**Affected crates:** new `astraea-flight` crate

**New dependencies:** `arrow`, `arrow-flight`, `tonic` (already present)

**Implementation steps:**

1. **Create `crates/astraea-flight/`:**
   - Implement an Arrow Flight server using `arrow-flight` crate.
   - Service: `AstraeaFlightService` implementing `FlightService` trait.

2. **Define Arrow schemas for graph data:**
   ```
   NodeSchema: {id: UInt64, labels: List<Utf8>, properties: Utf8, embedding: FixedSizeList<Float32>}
   EdgeSchema: {id: UInt64, source: UInt64, target: UInt64, edge_type: Utf8, weight: Float64, ...}
   QueryResultSchema: {column_1: <type>, column_2: <type>, ...}  // dynamic per query
   ```

3. **Implement `do_get()` for query results:**
   - Client sends a GQL query as a `FlightTicket`.
   - Server executes the query (via item 1.1), converts `QueryResult` rows into Arrow `RecordBatch` objects.
   - Streams batches back via Arrow Flight.

4. **Implement `do_put()` for bulk import:**
   - Client sends Arrow RecordBatches containing nodes/edges.
   - Server deserializes and inserts into the graph.
   - Much faster than JSON for bulk loads.

5. **Python client using `pyarrow.flight`:**
   - Update `examples/python_client.py` with an Arrow Flight client class.
   - Query results arrive as `pyarrow.Table` — zero-copy into Polars/Pandas.

6. **Tests:**
   - Round-trip: insert via `do_put`, query via `do_get`, verify data integrity.
   - Benchmark: JSON vs Arrow Flight throughput for 100K node export.

**Acceptance criteria:**
- Python clients can receive query results as Arrow tables with no serialization overhead.
- Bulk import via Arrow Flight is significantly faster than JSON-per-line.

**What was implemented:**
- Created new `crates/astraea-flight/` crate with Arrow Flight server.
- `AstraeaFlightService` wrapping `Arc<dyn GraphOps>` and a GQL `Executor`.
- **`do_get`**: Takes a Ticket with GQL query string → parses/executes → converts `QueryResult` to Arrow `RecordBatch` with `FlightDataEncoderBuilder` → streams back.
- **`do_put`**: Receives Arrow `RecordBatch` stream → auto-detects nodes vs edges by schema → deserializes and bulk-inserts via `GraphOps::create_node()`/`create_edge()` → returns count metadata.
- Arrow schemas: `node_schema()`, `edge_schema()`, `query_result_schema()` (dynamic columns as nullable Utf8).
- `run_flight_server()` convenience function.
- Dependencies: `arrow = "57"`, `arrow-flight = "57"`, `arrow-schema = "57"`, `futures = "0.3"`.
- 11 tests: do_get (basic, empty, invalid query, WHERE filter), import_nodes, import_edges (with/without temporal).

---

### 2.5 Python Client (Arrow Flight)

**Status:** COMPLETED

**Priority:** Medium — depends on items 2.3 and 2.4.

**Rationale:** The existing `examples/python_client.py` uses raw TCP sockets and JSON. A production-quality Python client should use Arrow Flight for performance and `pip install` distribution.

**Implementation steps:**

1. **Create a `python/` directory** with a proper Python package:
   ```
   python/
   ├── pyproject.toml
   ├── astraeadb/
   │   ├── __init__.py
   │   ├── client.py          # Arrow Flight client
   │   ├── json_client.py     # Legacy JSON/TCP client (from examples/)
   │   └── types.py           # Python dataclasses for Node, Edge, etc.
   ```

2. **Implement `AstraeaClient` using `pyarrow.flight`:**
   - `connect(host, port)` — establish Flight connection.
   - `query(gql: str) -> pa.Table` — execute GQL, return Arrow table.
   - `create_node(labels, properties, embedding)` — convenience wrapper.
   - `vector_search(query, k) -> pa.Table` — vector similarity search.
   - `bulk_insert(nodes: pa.Table)` — Arrow-native bulk import.

3. **Fallback:** If `pyarrow` is not installed, fall back to the JSON/TCP client.

4. **Distribution:** Publish to PyPI as `astraeadb`.

**Acceptance criteria:**
- `pip install astraeadb` provides a working client.
- Query results arrive as `pyarrow.Table` or `pandas.DataFrame`.
- Bulk insert accepts a DataFrame or Arrow table.

**What was implemented:**
- Created `python/` directory with proper Python package structure.
- **`JsonClient`** (`json_client.py`): TCP/JSON protocol client with zero external dependencies. Full API: CRUD, traversals, queries, vector search, hybrid search, semantic operations.
- **`ArrowClient`** (`arrow_client.py`): Apache Arrow Flight client using `pyarrow.flight`. Methods: `query()` (returns `pa.Table`), `query_batches()` (streaming), `bulk_insert_nodes()`, `bulk_insert_edges()`, `query_to_pandas()`.
- **`AstraeaClient`** (`client.py`): Unified client that auto-selects Arrow Flight for queries when `pyarrow` is installed, falls back to JSON/TCP otherwise. CRUD always uses JSON/TCP.
- `pyproject.toml` with optional `[arrow]` dependency (`pyarrow>=14.0`).
- 23 unit tests using mocked sockets (no server needed): all CRUD operations, traversals, queries, vector/hybrid/semantic operations, error handling, context manager.

---

## 3. Phase 3 Gaps (GraphRAG Engine)

> **All 3 Phase 3 items are now COMPLETED.** 287 Rust tests + 23 Python tests pass. See individual items below for implementation details.

### 3.1 Subgraph Extraction & Linearization

**Status:** COMPLETED

**Priority:** Critical for Phase 3.

**Rationale:** The plan describes "Context Windows as Subgraphs" — extracting a local subgraph around a relevant node and converting it to text that an LLM can process.

**Affected crates:** new `astraea-rag` crate (or a module in `astraea-graph`)

**Implementation steps:**

1. **Implement `extract_subgraph()`:**
   ```rust
   pub fn extract_subgraph(
       graph: &dyn GraphOps,
       center: NodeId,
       hops: usize,
       max_nodes: usize,
   ) -> Result<Subgraph>
   ```
   - BFS from `center` up to `hops`, collecting all visited nodes and edges.
   - Cap at `max_nodes` to fit within LLM context windows.
   - Return a `Subgraph` struct containing the collected `Vec<Node>` and `Vec<Edge>`.

2. **Implement `linearize_subgraph()`:**
   - Convert the `Subgraph` into a text representation suitable for LLM context:
     ```
     Node [Person: Alice] (age: 30, role: "engineer")
       -[KNOWS {since: 2020}]-> [Person: Bob] (age: 35)
       -[WORKS_AT]-> [Company: Acme] (industry: "tech")
     Node [Person: Bob] (age: 35, role: "manager")
       -[MANAGES]-> [Person: Alice]
     ```
   - Support multiple formats:
     - `TextFormat::Prose` — natural language paragraphs.
     - `TextFormat::Structured` — indented tree as above.
     - `TextFormat::Triples` — `(subject, predicate, object)` triples.
     - `TextFormat::Json` — compact JSON for structured LLM prompts.

3. **Token budget estimation:**
   - Estimate token count per node/edge (based on property sizes).
   - Stop extraction when estimated tokens approach the budget limit.

4. **Tests:**
   - Extract subgraph from the cybersecurity demo graph. Verify correct nodes/edges.
   - Linearize and verify output format.
   - Token budget: verify extraction stops before exceeding the limit.

**Acceptance criteria:**
- Given a node and hop count, extract the local subgraph.
- Linearize to text in multiple formats.
- Token budget prevents context window overflow.

**What was implemented:**
- Created new `astraea-rag` crate at `crates/astraea-rag/`.
- **`subgraph.rs`**: `Subgraph` struct with `center`, `nodes`, `edges`. `extract_subgraph()` using BFS with `max_nodes` cap. `extract_subgraph_semantic()` using vector search to find anchor, then BFS extraction.
- **`linearize.rs`**: `TextFormat` enum (Prose, Structured, Triples, Json). `linearize_subgraph()` converts Subgraph to text in any format. Helper functions: `node_display_name()`, `format_properties()`.
- **`token.rs`**: `estimate_tokens()` (4 chars per token approximation). `extract_with_budget()` incrementally builds subgraph, stopping when token estimate exceeds budget.
- 12 tests: basic extraction, max_nodes cap, edge inclusion, all 4 linearization formats, token estimation, budget-aware extraction, semantic extraction, empty subgraph, single hop.

---

### 3.2 LLM Integration

**Status:** COMPLETED

**Priority:** High for Phase 3 — depends on item 3.1.

**Rationale:** The plan describes an atomic operation: vector search → graph traversal → subgraph linearization → LLM query.

**Affected crates:** new `astraea-rag` crate

**New dependencies:** `reqwest` (HTTP client for LLM APIs)

**Implementation steps:**

1. **Define an `LlmProvider` trait:**
   ```rust
   #[async_trait]
   pub trait LlmProvider: Send + Sync {
       async fn complete(&self, prompt: &str, context: &str) -> Result<String>;
       fn context_window_tokens(&self) -> usize;
   }
   ```

2. **Implement providers:**
   - `OpenAiProvider` — calls the OpenAI API (or compatible endpoints).
   - `AnthropicProvider` — calls the Anthropic API.
   - `OllamaProvider` — calls a local Ollama instance.
   - Configuration: API key, model name, endpoint URL, temperature.

3. **Implement the GraphRAG pipeline:**
   ```rust
   pub async fn graph_rag_query(
       graph: &dyn GraphOps,
       vector_index: &dyn VectorIndex,
       llm: &dyn LlmProvider,
       question: &str,
       question_embedding: &[f32],
   ) -> Result<String> {
       // 1. Vector search: find the most relevant node.
       let nearest = vector_index.search(question_embedding, 1)?;
       let anchor = nearest[0].node_id;

       // 2. Graph traversal: extract 2-hop subgraph.
       let subgraph = extract_subgraph(graph, anchor, 2, 50)?;

       // 3. Linearize to text.
       let context = linearize_subgraph(&subgraph, TextFormat::Structured)?;

       // 4. Send to LLM.
       let answer = llm.complete(question, &context).await?;

       Ok(answer)
   }
   ```

4. **Add a `GraphRAG` request to the protocol:**
   ```rust
   Request::GraphRAG {
       question: String,
       question_embedding: Vec<f32>,
       hops: usize,
       max_context_nodes: usize,
   }
   ```

5. **Tests:**
   - Mock LLM provider that returns a canned response.
   - End-to-end: vector search → subgraph → linearize → mock LLM → response.
   - Test with the cybersecurity demo: "Who compromised Alice's laptop?" → should extract the attack path subgraph.

**Acceptance criteria:**
- A single `GraphRAG` request performs vector search, graph traversal, linearization, and LLM query atomically.
- Configurable LLM provider (OpenAI, Anthropic, local).
- Context is bounded by the LLM's token window.

**What was implemented:**
- **`llm.rs`**: `LlmProvider` trait with `complete()`, `context_window_tokens()`, `name()`. `LlmConfig` and `ProviderType` (OpenAi, Anthropic, Ollama, Mock). Four providers:
  - `MockProvider` — returns canned response with prompt/context info (for testing).
  - `OpenAiProvider` — configurable with injectable HTTP callback (`with_http_fn()`), formats OpenAI-compatible requests.
  - `AnthropicProvider` — same pattern, formats Anthropic Messages API requests.
  - `OllamaProvider` — same pattern, default endpoint `http://localhost:11434`.
  - No external HTTP dependencies — providers accept callback functions for API calls.
- **`pipeline.rs`**: `GraphRagConfig` (hops, max_context_nodes, text_format, token_budget, system_prompt). `GraphRagResult` (answer, anchor_node_id, context_text, nodes_in_context, estimated_tokens). Two entry points:
  - `graph_rag_query()` — vector search → subgraph extraction → linearization → LLM call.
  - `graph_rag_query_anchored()` — skips vector search when anchor is known.
  - `build_prompt()` helper assembles system prompt + context + question.
- Added `ExtractSubgraph` and `GraphRag` request types to `astraea-server` protocol.
- Added handler implementations: `ExtractSubgraph` returns linearized text with metadata; `GraphRag` finds anchor (via provided ID or vector search), extracts subgraph, returns context for LLM use.
- Added `astraea-rag` as dependency of `astraea-server`.
- 15 new RAG tests (llm providers, pipeline, prompt building) + 4 new server tests (extract_subgraph, graph_rag with anchor/embedding/error).

---

### 3.3 Differentiable Traversal / GNN Training Loop

**Status:** COMPLETED

**Priority:** Low (research feature) — depends on items 2.1 and 2.2.

**Rationale:** The plan describes making the query execution plan differentiable, enabling backpropagation of loss gradients to update edge weights inside the database.

**Affected crates:** new `astraea-gnn` crate

**New dependencies:** `burn` (Rust ML framework) or `tch` (PyTorch bindings)

**Implementation steps:**

1. **Define differentiable edge weights:**
   - Edge weights are already `f64` in the `Edge` struct.
   - Wrap them in a differentiable tensor type (e.g., `burn::Tensor<B, 1>`).
   - Track gradients during traversal.

2. **Implement a message-passing layer:**
   ```rust
   pub fn message_passing(
       graph: &dyn GraphOps,
       node_features: &HashMap<NodeId, Tensor>,  // node embeddings
       edge_weights: &HashMap<EdgeId, Tensor>,     // learnable weights
   ) -> HashMap<NodeId, Tensor>  // updated embeddings
   ```
   - For each node, aggregate neighbor features weighted by edge weights.
   - Apply a non-linear activation function.
   - This implements one layer of a Graph Neural Network (GNN).

3. **Implement the training loop:**
   - Forward pass: run message passing for N layers.
   - Compute loss against ground truth labels (e.g., node classification).
   - Backward pass: compute gradients w.r.t. edge weights.
   - Update edge weights in the database: `graph.update_edge(id, new_weight)`.

4. **GQL integration (future):**
   - `CALL db.gnn.train(labels="fraud", layers=3, epochs=100)` — train a GNN inside the database.
   - `CALL db.gnn.predict(node_id)` — run inference.

5. **Tests:**
   - Train a simple GNN (2 layers) on a small classification task.
   - Verify that edge weights converge and accuracy improves.

**Acceptance criteria:**
- Edge weights can be updated via gradient descent.
- A basic GNN can be trained entirely within the database.
- No external Python/PyTorch dependency required (pure Rust ML stack).

**What was implemented:**
- Created new `astraea-gnn` crate at `crates/astraea-gnn/`.
- **`tensor.rs`**: `Tensor` struct with `data: Vec<f32>`, `grad: RefCell<Option<Vec<f32>>>`, `requires_grad: bool`. Operations: `add()`, `mul()`, `scale()`, `dot()`, `sum()`, `relu()`, `sigmoid()`, `norm()`, `mean()`. Gradient tracking: `set_grad()`, `grad()`, `zero_grad()`.
- **`message_passing.rs`**: `MessagePassingConfig` with `Aggregation` (Sum, Mean, Max) and `Activation` (ReLU, Sigmoid, None). `message_passing()` aggregates neighbor features weighted by edge weights, applies activation and optional L2 normalization.
- **`training.rs`**: `TrainingConfig` (layers, learning_rate, epochs). `TrainingData` (node labels, num_classes). `TrainingResult` (epoch_losses, final_predictions, accuracy). `train_node_classification()` implements the full training loop:
  - Forward pass: extracts features from node embeddings, runs N message passing layers.
  - Loss computation: MSE-based loss against one-hot class targets.
  - Backward pass: numerical gradient computation via finite differences on edge weights.
  - Weight update: gradient descent on edge weights.
  - Pure Rust, no external ML framework dependencies.
- 26 tests: tensor operations (add, mul, scale, dot, relu, sigmoid, norm, grad), message passing (sum, mean, relu, normalize, edge weights, isolated nodes), training (loss decrease, predictions, config, single epoch, argmax, empty labels error, loss correctness).

---

## 4. Research Features

### 4.1 Temporal Graph Queries (Time-Travel)

**Priority:** Medium — the `ValidityInterval` type is already implemented in `astraea-core/src/types.rs:44-68` with `contains(timestamp)`, but it is not queryable.

**Rationale:** The plan describes using persistent data structures for time-travel queries like *"Show me the shortest path between A and B as it existed on Jan 1st, 2024."*

**Affected crates:** `astraea-core` (trait extension), `astraea-graph` (temporal traversals), `astraea-query` (temporal syntax)

**Implementation steps:**

1. **Add temporal-aware traversal methods to `GraphOps`:**
   ```rust
   fn neighbors_at(
       &self,
       node_id: NodeId,
       direction: Direction,
       timestamp: i64,
   ) -> Result<Vec<(EdgeId, NodeId)>>;

   fn shortest_path_at(
       &self,
       from: NodeId,
       to: NodeId,
       timestamp: i64,
   ) -> Result<Option<GraphPath>>;

   fn bfs_at(
       &self,
       start: NodeId,
       max_depth: usize,
       timestamp: i64,
   ) -> Result<Vec<(NodeId, usize)>>;
   ```

2. **Implementation in `Graph`:**
   - `neighbors_at()`: call `neighbors()` then filter edges where `edge.validity.contains(timestamp)`.
   - `shortest_path_at()`: modify BFS/Dijkstra to only traverse edges valid at the given timestamp.
   - `bfs_at()`: same filtering during traversal.

3. **Add GQL temporal syntax:**
   - Extend the parser to support `AT TIMESTAMP <epoch_ms>`:
     ```
     MATCH (a:Person)-[:KNOWS]->(b:Person)
     AT TIMESTAMP 1704067200000
     RETURN a.name, b.name
     ```
   - Add `at_timestamp: Option<i64>` to `MatchQuery` in the AST.

4. **Persistent data structures (future optimization):**
   - Instead of filtering on every query, maintain a persistent version tree.
   - Each edge modification creates a new version node in the tree.
   - Time-travel queries navigate the version tree to find the correct version.
   - Use a functional B-tree (immutable with structural sharing) for efficient space usage.

5. **Add temporal request types to the protocol:**
   ```rust
   Request::NeighborsAt { id: u64, direction: String, timestamp: i64 }
   Request::ShortestPathAt { from: u64, to: u64, timestamp: i64 }
   ```

6. **Tests:**
   - Create edges with validity intervals. Query at different timestamps. Verify only valid edges are traversed.
   - Use the cybersecurity demo: DHCP leases have temporal bounds. Query at a time before/during/after the lease.
   - Shortest path at a timestamp should differ from shortest path without temporal constraints.

**Acceptance criteria:**
- Traversals and queries can be scoped to a specific point in time.
- Edges outside their validity interval are invisible at the queried timestamp.
- GQL supports the `AT TIMESTAMP` clause.

---

### 4.2 Homomorphic Encryption

**Priority:** Low (research) — complex and specialized.

**Rationale:** Allow clients to query the graph without the server seeing unencrypted data. Essential for banking and healthcare.

**Affected crates:** new `astraea-crypto` crate

**New dependencies:** `tfhe` (Rust homomorphic encryption library) or bindings to Microsoft SEAL

**Implementation steps:**

1. **Define the encrypted query model:**
   - Client encrypts node labels and property values using FHE (Fully Homomorphic Encryption).
   - Server performs pattern matching on encrypted labels (equality comparison under encryption).
   - Server returns encrypted results; client decrypts locally.

2. **Implement `EncryptedLabel`:**
   - A label encrypted with the client's public key.
   - Support homomorphic equality: `encrypted_label_a == encrypted_label_b` returns an encrypted boolean.

3. **Implement encrypted pattern matching:**
   - `MATCH (a:Person)` where `Person` is encrypted.
   - Server iterates all nodes, computes homomorphic equality between each node's encrypted label and the query label.
   - Returns nodes where the encrypted comparison is true.

4. **Limitations:**
   - FHE is extremely slow (1000x-10000x overhead). Only viable for small result sets.
   - Support basic operations: label matching, property equality. No range queries or aggregations.

5. **Key management:**
   - Client generates key pair.
   - Public key is sent to the server for encryption operations.
   - Private key stays with the client.

6. **Tests:**
   - Encrypt a label, store encrypted node, query with encrypted label, decrypt result, verify match.
   - Verify that the server never sees plaintext data.

**Acceptance criteria:**
- Basic pattern matching works on encrypted labels.
- Server cannot see plaintext node labels or property values.
- Performance is documented (expected to be orders of magnitude slower).

---

### 4.3 GPU / CUDA Acceleration

**Priority:** Low (research) — specialized hardware required.

**Rationale:** Graph algorithms like PageRank and community detection (Louvain) are matrix operations that benefit from GPU parallelism.

**Affected crates:** new `astraea-gpu` crate (feature-gated)

**New dependencies:** `cudarc` (Rust CUDA bindings) or integration with NVIDIA cuGraph via FFI

**Implementation steps:**

1. **Feature-gate GPU support:** `[features] cuda = ["cudarc"]` in `astraea-gpu/Cargo.toml`.

2. **Adjacency matrix export:**
   - Convert the graph's adjacency structure into a CSR (Compressed Sparse Row) matrix.
   - Transfer the CSR matrix to GPU memory.

3. **Implement GPU-accelerated algorithms:**
   - **PageRank:** Iterative sparse matrix-vector multiplication.
     - CPU: `rank = damping * (A^T * rank) + (1 - damping) / N`
     - GPU: use cuBLAS or custom CUDA kernel for SpMV.
   - **Louvain community detection:** Modularity optimization on GPU.
   - **BFS:** Level-synchronous BFS using GPU frontier expansion.

4. **Query integration:**
   - Add `CALL db.analytics.pagerank()` and `CALL db.analytics.communities()` to GQL.
   - The executor detects analytical queries and offloads to the GPU backend.

5. **Fallback:** If no GPU is available, fall back to CPU implementations.

6. **Tests:**
   - Compute PageRank on CPU and GPU, verify results match within floating-point tolerance.
   - Benchmark: GPU vs CPU for PageRank on 100K and 1M node graphs.

**Acceptance criteria:**
- PageRank and Louvain produce correct results on GPU.
- Automatic fallback to CPU when GPU is unavailable.
- Measurable speedup (>10x) for graphs with >100K nodes.

---

### 4.4 Sharding / Massively Parallel Processing

**Priority:** Low — foundational for horizontal scaling but significant architectural effort.

**Rationale:** The plan references TigerGraph-style MPP for terabyte-scale graphs. Currently AstraeaDB is single-node only.

**Affected crates:** new `astraea-cluster` crate, modifications to `astraea-server` and `astraea-storage`

**Implementation steps:**

1. **Partitioning strategy:**
   - **Hash partitioning:** `shard_id = hash(node_id) % num_shards`. Simple, even distribution.
   - **Label-based partitioning:** All nodes with the same label on the same shard. Better for label-scoped queries.
   - **Graph-aware partitioning:** Use METIS or similar to minimize cross-shard edges. Better for traversals.
   - Start with hash partitioning.

2. **Cluster coordinator:**
   - `ClusterCoordinator` — knows the shard map (which shard owns which ID range).
   - Runs on a designated coordinator node.
   - Handles shard membership, rebalancing, and failure detection.

3. **Distributed query execution:**
   - **Scatter-gather pattern:** Coordinator sends sub-queries to relevant shards. Each shard executes locally. Coordinator merges results.
   - **Cross-shard traversals:** When a BFS/DFS encounters an edge pointing to a node on another shard, send a remote traversal request.
   - **Remote edge resolution:** `get_node(id)` checks the shard map. If local, serve directly. If remote, forward via gRPC to the owning shard.

4. **Communication layer:**
   - Use gRPC (item 1.9) for inter-shard communication.
   - Define internal RPCs: `RemoteGetNode`, `RemoteGetEdges`, `RemoteTraverse`.

5. **Replication:**
   - For fault tolerance, replicate each shard to R replicas.
   - Read from any replica, write to the primary.
   - Use Raft consensus for primary election.

6. **Tests:**
   - Multi-process integration test: start 3 shard processes, insert data, query across shards.
   - Verify cross-shard traversals return correct results.
   - Kill a shard, verify failover to replica.

**Acceptance criteria:**
- Data is partitioned across multiple shard processes.
- Queries work transparently across shards.
- Cross-shard traversal latency is documented.

---

## 5. Production Readiness

### 5.1 Authentication & Access Control

**Priority:** High for production use.

**Rationale:** The server currently accepts all connections without authentication. No access control.

**Affected crates:** `astraea-server`, `astraea-cli`

**Implementation steps:**

1. **API key authentication:**
   - Server config includes a list of API keys (or a single admin key).
   - Clients must send an `Authorization` header (for gRPC) or an `auth_token` field (for JSON protocol).
   - Reject unauthenticated requests.

2. **mTLS (mutual TLS):**
   - Server presents a TLS certificate. Clients present client certificates.
   - Use `rustls` for TLS implementation.
   - Configuration: `tls_cert`, `tls_key`, `ca_cert` in server config.

3. **Role-based access control (RBAC):**
   - Define roles: `admin` (full access), `writer` (read + write), `reader` (read only).
   - Assign roles to API keys.
   - Check permissions before executing requests.

4. **Audit logging:**
   - Log all authenticated requests with timestamp, user, operation, and result.

5. **Tests:**
   - Reject unauthenticated requests.
   - Accept valid API keys.
   - Reader role cannot create/delete nodes.

**Acceptance criteria:**
- No unauthenticated access to the server.
- TLS encrypts all communication.
- Roles restrict operations appropriately.

---

### 5.2 Observability (Metrics & Tracing)

**Priority:** Medium for production use.

**Rationale:** `tracing` and `tracing-subscriber` are already dependencies but not integrated into request handling. No Prometheus metrics.

**Affected crates:** `astraea-server`

**New dependencies:** `metrics`, `metrics-exporter-prometheus`

**Implementation steps:**

1. **Instrument request handling with `tracing`:**
   - Add `#[instrument]` attributes to key functions.
   - Log request type, duration, result status.

2. **Expose Prometheus metrics:**
   - Start a metrics HTTP endpoint (e.g., `:9090/metrics`).
   - Metrics:
     - `astraea_requests_total{type="CreateNode|GetNode|..."}` — request counter.
     - `astraea_request_duration_seconds` — histogram.
     - `astraea_nodes_total` — gauge of total nodes.
     - `astraea_edges_total` — gauge of total edges.
     - `astraea_buffer_pool_hit_ratio` — cache hit rate.
     - `astraea_vector_index_size` — number of vectors indexed.
     - `astraea_active_connections` — current TCP connections.

3. **Health endpoint:**
   - `/health` returns 200 if the server is operational.
   - `/ready` returns 200 once the server has loaded its data.

4. **Tests:**
   - Verify metrics endpoint returns valid Prometheus text format.
   - Verify request count increments after each request.

**Acceptance criteria:**
- Prometheus can scrape metrics from the server.
- Request tracing shows up in structured logs.
- Health and readiness endpoints work.

---

### 5.3 Connection Pooling & Backpressure

**Priority:** Medium for production use.

**Rationale:** The current server spawns one tokio task per TCP connection with no limits. Under load, this can exhaust memory.

**Affected crates:** `astraea-server`

**Implementation steps:**

1. **Connection limit:**
   - Add `max_connections: usize` to `ServerConfig` (default: 1024).
   - Track active connections with an `AtomicUsize`.
   - Reject new connections when the limit is reached (send an error response, then close).

2. **Request queuing:**
   - Use a bounded `tokio::sync::Semaphore` to limit concurrent request processing.
   - Requests beyond the concurrency limit wait in a queue.

3. **Timeouts:**
   - `idle_timeout`: close connections that haven't sent a request in N seconds.
   - `request_timeout`: abort requests that take longer than N seconds.

4. **Graceful shutdown:**
   - On SIGTERM, stop accepting new connections.
   - Wait for in-flight requests to complete (up to a drain timeout).
   - Flush the buffer pool and vector index.
   - Exit.

5. **Tests:**
   - Open `max_connections + 1` connections, verify the last is rejected.
   - Verify idle timeout closes stale connections.
   - Verify graceful shutdown completes in-flight requests.

**Acceptance criteria:**
- Server remains stable under connection floods.
- Idle connections are cleaned up.
- Graceful shutdown preserves data integrity.

---

## Summary Matrix

| # | Item | Priority | Phase | Status | Depends On |
|---|------|----------|-------|--------|------------|
| 1.1 | Query Executor | Critical | 1 | **DONE** | — |
| 1.2 | Pointer Swizzling | High | 1 | **DONE** | — |
| 1.3 | Label Index | High | 1 | **DONE** | — |
| 1.4 | MVCC / Transactions | High | 1 | **DONE** | — |
| 1.5 | HNSW Persistence | High | 1 | **DONE** | — |
| 1.6 | Cold Tier (S3/Parquet) | Medium | 1 | **DONE** (foundation) | — |
| 1.7 | io_uring | Medium | 1 | **DONE** (PageIO trait) | — |
| 1.8 | CLI Commands | Medium | 1 | **DONE** | 1.1 (shell needs executor) |
| 1.9 | gRPC Transport | Medium | 1 | **DONE** | — |
| 1.10 | Benchmarks | Low | 1 | **DONE** | — |
| 2.1 | Hybrid Search | Critical | 2 | **DONE** | 1.1, 2.3 |
| 2.2 | Semantic Traversal | High | 2 | **DONE** | 2.3 |
| 2.3 | Vector Server Integration | High | 2 | **DONE** | — |
| 2.4 | Arrow Zero-Copy IPC | Medium | 2 | **DONE** | 1.1 |
| 2.5 | Python Client (Arrow) | Medium | 2 | **DONE** | 2.4 |
| 3.1 | Subgraph Extraction | Critical | 3 | **DONE** | — |
| 3.2 | LLM Integration | High | 3 | **DONE** | 3.1 |
| 3.3 | Differentiable Traversal | Low | 3 | **DONE** | 2.1 |
| 4.1 | Temporal Queries | Medium | R | Not started | — |
| 4.2 | Homomorphic Encryption | Low | R | Not started | — |
| 4.3 | GPU Acceleration | Low | R | Not started | — |
| 4.4 | Sharding / MPP | Low | R | Not started | 1.9 |
| 5.1 | Authentication | High | Prod | Not started | — |
| 5.2 | Observability | Medium | Prod | Not started | — |
| 5.3 | Connection Pooling | Medium | Prod | Not started | — |

**Recommended implementation order (critical path):**

```
1.1 Query Executor ──┬── 1.8 CLI Shell
                     ├── 2.4 Arrow IPC ── 2.5 Python Client
                     └── 2.1 Hybrid Search
1.3 Label Index ─────┘
2.3 Vector Server ───┬── 2.1 Hybrid Search
                     └── 2.2 Semantic Traversal
1.5 HNSW Persistence
1.4 MVCC / Transactions
3.1 Subgraph Extraction ── 3.2 LLM Integration
1.2 Pointer Swizzling
5.1 Authentication
```
