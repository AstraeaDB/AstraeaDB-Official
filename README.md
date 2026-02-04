# AstraeaDB

A cloud-native, AI-first graph database written in Rust. AstraeaDB combines a **Vector-Property Graph** model with an **HNSW vector index**, enabling both structural graph traversals and semantic similarity search in a single system.

## Architecture

```
                    ┌─────────────────────────────────┐
                    │         astraea-cli              │
                    │   serve | shell | import | export│
                    └──────────────┬──────────────────┘
                                   │
                    ┌──────────────▼──────────────────┐
                    │        astraea-server            │
                    │   JSON-over-TCP (port 7687)      │
                    │   async tokio, per-connection     │
                    └──────┬───────────┬──────────────┘
                           │           │
              ┌────────────▼──┐   ┌────▼───────────┐
              │ astraea-query │   │ astraea-vector  │
              │  GQL Parser   │   │  HNSW Index     │
              │  Lexer → AST  │   │  ANN Search     │
              └───────────────┘   └────────────────┘
                           │           │
                    ┌──────▼───────────▼──────────────┐
                    │        astraea-graph             │
                    │   CRUD, BFS, DFS, Dijkstra       │
                    │   GraphOps trait implementation   │
                    └──────────────┬──────────────────┘
                                   │
                    ┌──────────────▼──────────────────┐
                    │       astraea-storage            │
                    │  Pages (8 KiB) → Buffer Pool     │
                    │  File Manager → WAL (CRC32)      │
                    └──────────────┬──────────────────┘
                                   │
                    ┌──────────────▼──────────────────┐
                    │        astraea-core              │
                    │  Types, Traits, Errors            │
                    │  Node, Edge, StorageEngine, ...   │
                    └─────────────────────────────────┘
```

## Crate Overview

| Crate | Purpose | Tests |
|---|---|---:|
| `astraea-core` | Foundational types (`Node`, `Edge`, `NodeId`), traits (`StorageEngine`, `GraphOps`, `VectorIndex`), and error types | 4 |
| `astraea-storage` | Disk-backed storage engine: 8 KiB pages, LRU buffer pool, write-ahead log with CRC32 checksums | 20 |
| `astraea-graph` | Graph CRUD operations and traversal algorithms (BFS, DFS, Dijkstra shortest path) | 24 |
| `astraea-query` | Hand-written GQL/Cypher parser: lexer, recursive-descent parser, AST for MATCH/CREATE/DELETE | 25 |
| `astraea-vector` | HNSW approximate nearest-neighbor index with cosine, Euclidean, and dot-product distance metrics | 26 |
| `astraea-server` | Async TCP server (tokio) accepting newline-delimited JSON requests | 6 |
| `astraea-cli` | Command-line interface: `serve`, `shell`, `status`, `import`, `export` | - |
| **Total** | | **105** |

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

- **Tier 1 (Cold):** Data on disk in fixed 8 KiB pages. Each page carries a CRC32 checksum for corruption detection.
- **Tier 2 (Warm):** An LRU buffer pool caches frequently accessed pages in memory with pin/unpin semantics to prevent eviction of in-use pages.
- **Tier 3 (Hot):** In-memory indices (`HashMap<NodeId, PageId>`) and the HNSW vector index provide nanosecond-level lookups for active data.

**Write-Ahead Log (WAL):** Every mutation is logged before being applied. Records use a `[length][type][JSON payload][CRC32]` frame format. The WAL supports checkpoint and truncation for recovery.

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

### GQL Parser (`astraea-query`)

A hand-written recursive-descent parser for a subset of ISO GQL / Cypher:

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

### HNSW Vector Index (`astraea-vector`)

An implementation of the Hierarchical Navigable Small World algorithm (Malkov & Yashunin, 2016):

- **Multi-layer graph** with exponentially decreasing node membership at higher layers
- **Configurable parameters:** `M` (max connections, default 16), `ef_construction` (build beam width, default 200), `ef_search` (query beam width, default 50)
- **Three distance metrics:** Cosine similarity, Euclidean (L2), dot product
- **Incremental updates:** Insert and remove vectors without rebuilding
- **Thread-safe:** `RwLock` wrapper allows concurrent reads with exclusive writes

```rust
let index = HnswVectorIndex::new(128, DistanceMetric::Cosine);
index.insert(node_id, &embedding)?;

let results = index.search(&query_vector, 10)?;
// results: Vec<SimilarityResult { node_id, distance }>
```

### Network Server (`astraea-server`)

A TCP server using newline-delimited JSON as its wire protocol. Each request is a single JSON line; each response is a single JSON line. This makes the protocol debuggable with standard tools like `telnet` or `netcat`.

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
| `VectorSearch` | k-nearest-neighbor search (planned) |
| `Query` | Execute a GQL query string (planned) |
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

Then send JSON requests:

```json
{"type":"CreateNode","labels":["Person"],"properties":{"name":"Alice","age":30}}
{"type":"CreateNode","labels":["Person"],"properties":{"name":"Bob","age":25}}
{"type":"CreateEdge","source":1,"target":2,"edge_type":"KNOWS","properties":{},"weight":1.0}
{"type":"Neighbors","id":1,"direction":"outgoing"}
{"type":"ShortestPath","from":1,"to":2,"weighted":false}
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

// Add an edge
graph.create_edge(alice, bob, "KNOWS".into(), serde_json::json!({}), 1.0)?;

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

### Parsing GQL Queries

```rust
use astraea_query::parse;

let ast = parse("MATCH (a:Person)-[:KNOWS]->(b) WHERE a.age > 30 RETURN b.name")?;
// ast is a Statement::Match with pattern, where clause, and return clause
```

### Python Client

A Python client is provided in `examples/python_client.py`. It wraps the TCP/JSON protocol in a clean API.

**Prerequisites:** Python 3.10+ (no external dependencies required — uses only the standard library).

**Start the server, then run the demo:**

```bash
# Terminal 1: start the server
cargo run -p astraea-cli -- serve

# Terminal 2: run the Python demo
python3 examples/python_client.py
```

**Example output:**

```
============================================================
AstraeaDB Python Client Demo: Social Network
============================================================

1. Creating nodes (people)...
   Created: Alice(id=1), Bob(id=2), Charlie(id=3), Diana(id=4), Eve(id=5)

2. Creating edges (relationships)...
   Created 6 edges (5 KNOWS + 1 FOLLOWS)

3. Reading nodes...
   Alice: labels=['Person'], properties={'age': 30, 'city': 'NYC', 'name': 'Alice'}

4. Updating Alice's properties...
   Alice now: {'age': 30, 'city': 'San Francisco', 'name': 'Alice', 'title': 'Engineer'}

5. Querying neighbors...
   Alice's outgoing neighbors: 3 connections
     -> Charlie (edge_id=2)
     -> Bob (edge_id=1)
     -> Eve (edge_id=6)
   Alice KNOWS: 2 people
   Who knows Diana: 2 people
     <- Bob
     <- Charlie

6. BFS traversal from Alice (depth=2)...
   Depth 0: Alice
   Depth 1: Charlie
   Depth 1: Bob
   Depth 1: Eve
   Depth 2: Diana

7. Shortest path from Alice to Eve...
   Unweighted (fewest hops): Alice -> Eve (1 hops)
   Weighted (lowest cost):   Alice -> Eve (cost=0.30)

8. Deleting Eve...
   No path from Alice to Eve (Eve was deleted)

9. Server health check...
   Server version: 0.1.0, pong: True

============================================================
Demo complete.
============================================================
```

**Using the client in your own code:**

```python
from examples.python_client import AstraeaClient

with AstraeaClient(host="127.0.0.1", port=7687) as client:
    # Create nodes
    alice = client.create_node(["Person"], {"name": "Alice", "age": 30})
    bob = client.create_node(["Person"], {"name": "Bob", "age": 25})

    # Create an edge
    client.create_edge(alice, bob, "KNOWS", {"since": 2020}, weight=0.9)

    # Query neighbors
    neighbors = client.neighbors(alice, direction="outgoing")

    # BFS traversal (2 hops)
    reachable = client.bfs(alice, max_depth=2)

    # Shortest path (weighted Dijkstra)
    path = client.shortest_path(alice, bob, weighted=True)

    # Update properties (merge semantics)
    client.update_node(alice, {"city": "San Francisco"})

    # Delete a node (cascades to edges)
    client.delete_node(bob)

    # Health check
    status = client.ping()
```

**Client API reference:**

| Method | Description |
|---|---|
| `ping()` | Health check, returns server version |
| `create_node(labels, properties, embedding?)` | Create a node, returns node ID |
| `get_node(id)` | Get node by ID |
| `update_node(id, properties)` | Merge properties into a node |
| `delete_node(id)` | Delete node and all connected edges |
| `create_edge(source, target, type, properties?, weight?)` | Create an edge, returns edge ID |
| `get_edge(id)` | Get edge by ID |
| `delete_edge(id)` | Delete an edge |
| `neighbors(id, direction?, edge_type?)` | Get neighbors with optional filtering |
| `bfs(start, max_depth?)` | Breadth-first traversal |
| `shortest_path(from, to, weighted?)` | Shortest path (BFS or Dijkstra) |

### R Client

An R example client is provided at `examples/r_client.R`. It uses the `jsonlite`
package and base R socket connections to communicate with the server.

**Prerequisites:**

```r
install.packages("jsonlite")
```

**Run the demo:**

```bash
# Start the server
cargo run -p astraea-cli -- serve

# In another terminal
Rscript examples/r_client.R

# Or with a custom address
Rscript examples/r_client.R --host 127.0.0.1 --port 7687
```

**Example output:**

```
============================================================
AstraeaDB R Client Demo: Social Network
============================================================

1. Creating nodes (people)...
   Created: Alice(id=1), Bob(id=2), Charlie(id=3), Diana(id=4), Eve(id=5)

2. Creating edges (relationships)...
   Created 6 edges (5 KNOWS + 1 FOLLOWS)

3. Reading nodes...
   Alice: labels=["Person"], properties={"name":"Alice","age":30,"city":"NYC"}

4. Updating Alice's properties...
   Alice now: {"name":"Alice","age":30,"city":"San Francisco","title":"Engineer"}

5. Querying neighbors...
   Alice's outgoing neighbors: 3 connections
   Alice KNOWS: 2 people
   Who knows Diana: 2 people

6. BFS traversal from Alice (depth=2)...
   Depth 0: Alice
   Depth 1: Bob
   Depth 1: Charlie
   Depth 1: Eve
   Depth 2: Diana

7. Shortest path from Alice to Eve...
   Unweighted (fewest hops): Alice -> Eve (1 hops)
   Weighted (lowest cost):   Alice -> Eve (cost=0.30)

8. Deleting Eve...
   No path from Alice to Eve (Eve was deleted)

9. Server health check...
   Server version: 0.1.0, pong: TRUE
============================================================
Demo complete.
============================================================
```

**Programmatic usage in R:**

```r
library(jsonlite)
source("examples/r_client.R")

client <- AstraeaClient$new(host = "127.0.0.1", port = 7687L)
client$connect()

# Create nodes
id <- client$create_node(list("Person"), list(name = "Alice", age = 30))

# Create edges
eid <- client$create_edge(id1, id2, "KNOWS", list(since = 2024), weight = 0.9)

# Traverse
neighbors <- client$neighbors(id, direction = "outgoing", edge_type = "KNOWS")

# Shortest path
result <- client$shortest_path(from_node = id1, to_node = id2, weighted = TRUE)

# Health check
status <- client$ping()

client$close()
```

**Client API reference:**

| Method | Description |
|---|---|
| `$new(host, port)` | Create a new client instance |
| `$connect()` | Open the TCP connection |
| `$close()` | Close the connection |
| `$ping()` | Health check, returns server version |
| `$create_node(labels, properties, embedding?)` | Create a node, returns node ID |
| `$get_node(id)` | Get node by ID |
| `$update_node(id, properties)` | Merge properties into a node |
| `$delete_node(id)` | Delete node and all connected edges |
| `$create_edge(source, target, type, properties?, weight?)` | Create an edge, returns edge ID |
| `$get_edge(id)` | Get edge by ID |
| `$delete_edge(id)` | Delete an edge |
| `$neighbors(id, direction?, edge_type?)` | Get neighbors with optional filtering |
| `$bfs(start, max_depth?)` | Breadth-first traversal |
| `$shortest_path(from, to, weighted?)` | Shortest path (BFS or Dijkstra) |

## What Remains To Be Done

### Phase 1 Remaining (Foundation)

| Feature | Status | Description |
|---|---|---|
| **MVCC Transactions** | Not started | Snapshot isolation, write-conflict detection, transaction IDs on records, garbage collection of old versions |
| **Query Planner** | Not started | Convert parsed AST into logical plan (Scan, Filter, Expand, Project, Aggregate), then optimize with predicate pushdown and join reordering |
| **Query Executor** | Not started | Volcano-style pull iterator that evaluates physical plans against the storage engine |
| **Label Index** | Placeholder | `find_by_label()` currently returns an error; needs a label-to-NodeId index in storage |
| **Parquet/Arrow I/O** | Not started | Cold-tier export to Apache Parquet; zero-copy Arrow integration for analytics interop |
| **Import/Export CLI** | Stubbed | CLI commands exist but `import` and `export` are not yet implemented |
| **Integration Tests** | Minimal | End-to-end tests (server start, send requests, verify), crash recovery tests, concurrent access tests |
| **Benchmarks** | Stubbed | Criterion benchmark harnesses exist but have no benchmark functions yet |

### Phase 2 (Semantic Layer)

| Feature | Description |
|---|---|
| **Hybrid Search API** | Combine vector similarity scores with graph distance scores; blend with configurable alpha |
| **Temporal Traversals** | Filter edges by `ValidityInterval` during BFS/DFS/Dijkstra; "show me the graph at time T" queries |
| **Bulk Embedding Import** | Load embeddings from NumPy `.npy` or Arrow arrays |
| **HNSW Persistence** | Serialize/deserialize the HNSW layers to disk pages in the storage engine |

### Phase 3 (Query Engine & GraphRAG)

| Feature | Description |
|---|---|
| **Query Planner & Optimizer** | Predicate pushdown, cardinality estimation, index selection |
| **Query Executor** | Volcano iterator model with NodeScan, EdgeExpand, Filter, Project, Aggregate, Sort, Limit operators |
| **GQL Procedures** | Built-in `CALL db.index.vector.search(...)` for vector search within queries |
| **GraphRAG Pipeline** | Subgraph extraction, linearization to text, LLM integration via HTTP API |

### Phase 4 (Advanced)

| Feature | Description |
|---|---|
| **Graph Algorithms** | PageRank, Louvain community detection, connected components, centrality measures |
| **Differentiable Traversal** | Forward/backward pass through query execution for GNN training |
| **Distributed / MPP** | Hash-based graph partitioning, Raft consensus, cross-shard traversal |
| **Homomorphic Encryption** | Encrypted label matching and property comparison via `tfhe-rs` |
| **GPU Acceleration** | CUDA/cuGraph offload for PageRank, BFS, SSSP on large graphs |

## Project Structure

```
astraeadb/
├── Cargo.toml                 # Workspace root
├── implementation_plan.md     # Detailed step-by-step plan
├── claude.md                  # Project vision and requirements
├── crates/
│   ├── astraea-core/          # Types, traits, errors
│   │   └── src/
│   │       ├── types.rs       # NodeId, EdgeId, Node, Edge, etc.
│   │       ├── traits.rs      # StorageEngine, GraphOps, VectorIndex
│   │       └── error.rs       # AstraeaError enum
│   ├── astraea-storage/       # Disk-backed storage
│   │   └── src/
│   │       ├── page.rs        # 8 KiB page format, checksums
│   │       ├── file_manager.rs# Disk I/O
│   │       ├── buffer_pool.rs # LRU page cache
│   │       ├── wal.rs         # Write-ahead log
│   │       └── engine.rs      # DiskStorageEngine
│   ├── astraea-graph/         # Graph operations
│   │   └── src/
│   │       ├── graph.rs       # Graph struct, CRUD, GraphOps impl
│   │       ├── traversal.rs   # BFS, DFS, Dijkstra
│   │       └── test_utils.rs  # InMemoryStorage
│   ├── astraea-query/         # GQL parser
│   │   └── src/
│   │       ├── token.rs       # Token enum, Span
│   │       ├── lexer.rs       # Tokenizer
│   │       ├── ast.rs         # Statement, Expr, Pattern types
│   │       └── parser.rs      # Recursive-descent parser
│   ├── astraea-vector/        # Vector index
│   │   └── src/
│   │       ├── distance.rs    # Cosine, Euclidean, dot product
│   │       ├── hnsw.rs        # HNSW algorithm
│   │       └── index.rs       # Thread-safe VectorIndex wrapper
│   ├── astraea-server/        # Network server
│   │   └── src/
│   │       ├── protocol.rs    # Request/Response JSON types
│   │       ├── handler.rs     # Request dispatcher
│   │       └── server.rs      # Async TCP server
│   └── astraea-cli/           # CLI binary
│       └── src/
│           └── main.rs        # serve, shell, status, import, export
└── target/                    # Build artifacts
```

## License

MIT
