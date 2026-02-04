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
                    │   gRPC/Protobuf (port 7688)      │
                    │   async tokio, per-connection     │
                    └──────┬───────────┬──────────────┘
                           │           │
              ┌────────────▼──┐   ┌────▼───────────┐
              │ astraea-query │   │ astraea-vector  │
              │  GQL Parser   │   │  HNSW Index     │
              │  + Executor   │   │  ANN Search     │
              │  Lexer → AST  │   │  Persistence    │
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
                    │  Pointer Swizzling, MVCC, WAL    │
                    │  PageIO trait, Cold Storage       │
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
| `astraea-core` | Foundational types (`Node`, `Edge`, `NodeId`), traits (`StorageEngine`, `GraphOps`, `VectorIndex`, `TransactionalEngine`), and error types | 4 |
| `astraea-storage` | Disk-backed storage engine: 8 KiB pages, LRU buffer pool with pointer swizzling, MVCC transactions, WAL with CRC32 checksums, PageIO trait, cold storage, label index | 58 |
| `astraea-graph` | Graph CRUD operations and traversal algorithms (BFS, DFS, Dijkstra shortest path) | 37 |
| `astraea-query` | Hand-written GQL/Cypher parser and executor: lexer, recursive-descent parser, AST, full query execution pipeline | 56 |
| `astraea-vector` | HNSW approximate nearest-neighbor index with cosine, Euclidean, and dot-product distance metrics; binary persistence | 33 |
| `astraea-server` | Async TCP server (tokio) with JSON protocol and gRPC/Protobuf transport; GQL query execution | 13 |
| `astraea-cli` | Command-line interface: `serve`, `shell` (REPL), `status`, `import`, `export` | - |
| **Total** | | **201** |

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

- **Tier 1 (Cold):** `ColdStorage` trait with pluggable backends (currently `JsonFileColdStorage` for local files; Parquet/S3 planned). Data on disk in fixed 8 KiB pages with CRC32 checksums.
- **Tier 2 (Warm):** An LRU buffer pool caches frequently accessed pages in memory with pin/unpin semantics. The `PageIO` trait abstracts disk I/O, enabling pluggable backends (memmap2 default; io_uring planned).
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

### Network Server (`astraea-server`)

Two transport layers for different use cases:

1. **JSON-over-TCP** (port 7687): Newline-delimited JSON wire protocol. Each request/response is a single JSON line, debuggable with `telnet` or `netcat`.
2. **gRPC/Protobuf** (port 7688): Schema-enforced API via `tonic`/`prost` with 14 RPCs. Better performance and type safety for production clients.

Both transports delegate to the same `RequestHandler` and `Executor`.

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
| `VectorSearch` | k-nearest-neighbor search (server integration planned for Phase 2) |
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
| `create_edge(source, target, type, properties?, weight?, valid_from?, valid_to?)` | Create an edge, returns edge ID. `valid_from`/`valid_to` are epoch-ms bounds for temporal validity |
| `get_edge(id)` | Get edge by ID (includes `valid_from`/`valid_to`) |
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

## What Remains To Be Done

### Phase 1 (Foundation) — COMPLETED

All Phase 1 items have been implemented. 201 tests pass across the workspace.

| Feature | Status | Description |
|---|---|---|
| **Query Executor** | Done | Full GQL execution: MATCH, CREATE, DELETE, WHERE, ORDER BY, LIMIT, SKIP, DISTINCT. 30 tests. |
| **Pointer Swizzling** | Done | Frequency-based hot page promotion, zero-copy access, eviction prevention. 6 tests. |
| **Label Index** | Done | `HashMap<String, HashSet<NodeId>>` for O(1) label lookups. 5 tests. |
| **MVCC Transactions** | Done | Snapshot isolation, first-writer-wins conflict detection, `TransactionalEngine` trait. 15 tests. |
| **HNSW Persistence** | Done | Versioned binary format with bincode. Save/load without rebuilding. 7 tests. |
| **Cold Tier Storage** | Done | `ColdStorage` trait + `JsonFileColdStorage` backend. Parquet/S3 deferred. 7 tests. |
| **PageIO Trait** | Done | `PageIO` abstraction for pluggable I/O. io_uring backend deferred. 2 tests. |
| **CLI Commands** | Done | `import`, `export`, `shell` (REPL with rustyline), `status`. |
| **gRPC Transport** | Done | tonic/prost gRPC service with 14 RPCs. 7 tests. |
| **Benchmarks** | Done | 16 criterion benchmarks across storage, vector, and graph crates. |

### Phase 2 (Semantic Layer)

| Feature | Description |
|---|---|
| **Hybrid Search API** | Combine vector similarity scores with graph distance scores; blend with configurable alpha |
| **Semantic Traversal** | Rank neighbors by embedding similarity to a concept; multi-hop semantic walk |
| **Vector Server Integration** | Wire `VectorIndex` into `RequestHandler`; auto-index embeddings on node creation |
| **Apache Arrow Zero-Copy IPC** | Arrow Flight server for zero-copy data exchange with Python/Polars/Pandas |
| **Python Client (Arrow Flight)** | Production-quality Python client with `pyarrow.flight` transport |
| **Temporal Traversals** | Filter edges by `ValidityInterval` during BFS/DFS/Dijkstra; "show me the graph at time T" queries |
| **Parquet Cold Storage** | Upgrade `ColdStorage` backend from JSON to Apache Parquet with S3/GCS via `object_store` |

### Phase 3 (GraphRAG Engine)

| Feature | Description |
|---|---|
| **Subgraph Extraction** | Extract local subgraphs around nodes; linearize to text for LLM context windows |
| **LLM Integration** | Atomic GraphRAG pipeline: vector search -> graph traversal -> linearization -> LLM query |
| **Differentiable Traversal** | Forward/backward pass through query execution for GNN training |

### Phase 4 (Advanced / Research)

| Feature | Description |
|---|---|
| **Graph Algorithms** | PageRank, Louvain community detection, connected components, centrality measures |
| **Distributed / MPP** | Hash-based graph partitioning, Raft consensus, cross-shard traversal |
| **Homomorphic Encryption** | Encrypted label matching and property comparison via `tfhe-rs` |
| **GPU Acceleration** | CUDA/cuGraph offload for PageRank, BFS, SSSP on large graphs |

### Production Readiness

| Feature | Description |
|---|---|
| **Authentication & Access Control** | API key auth, mTLS, RBAC (admin/writer/reader roles), audit logging |
| **Observability** | Prometheus metrics, `tracing` instrumentation, health/readiness endpoints |
| **Connection Pooling & Backpressure** | Connection limits, request queuing, idle timeouts, graceful shutdown |

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
│   │       ├── buffer_pool.rs # LRU page cache with pointer swizzling
│   │       ├── wal.rs         # Write-ahead log (incl. transaction records)
│   │       ├── label_index.rs # HashMap-based label-to-NodeId index
│   │       ├── mvcc.rs        # MVCC transaction manager (snapshot isolation)
│   │       ├── cold_storage.rs# ColdStorage trait + JsonFileColdStorage
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
│   │       ├── protocol.rs    # Request/Response JSON types
│   │       ├── handler.rs     # Request dispatcher (with GQL executor)
│   │       ├── grpc.rs        # gRPC service (14 RPCs via tonic)
│   │       └── server.rs      # Async TCP server
│   └── astraea-cli/           # CLI binary
│       └── src/
│           └── main.rs        # serve, shell (REPL), status, import, export
├── examples/
│   ├── python_client.py       # Python TCP/JSON client
│   ├── cybersecurity_demo.py  # Cybersecurity investigation demo
│   └── r_client.R             # R TCP/JSON client
└── target/                    # Build artifacts
```

## License

MIT
