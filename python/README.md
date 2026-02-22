# AstraeaDB Python Client

Python client for [AstraeaDB](https://github.com/...) graph database.

## Installation

```bash
# Basic (JSON/TCP only, no external dependencies)
pip install astraeadb

# With Arrow Flight support (high-throughput)
pip install 'astraeadb[arrow]'
```

## Quick Start

```python
from astraeadb import AstraeaClient

with AstraeaClient(host="127.0.0.1", port=7687) as client:
    # Create nodes
    alice = client.create_node(["Person"], {"name": "Alice", "age": 30})
    bob = client.create_node(["Person"], {"name": "Bob", "age": 25})

    # Create an edge
    client.create_edge(alice, bob, "KNOWS", weight=0.9)

    # Execute a GQL query
    results = client.query("MATCH (n:Person) WHERE n.age > 25 RETURN n.name")

    # Vector search
    results = client.vector_search([0.1, 0.2, 0.3], k=5)

    # Hybrid search (graph + vector)
    results = client.hybrid_search(
        anchor=alice,
        query_vector=[0.1, 0.2, 0.3],
        max_hops=2,
        k=10,
        alpha=0.5,
    )
```

## Arrow Flight (High-Throughput)

When `pyarrow` is installed, query results are returned as Arrow Tables:

```python
from astraeadb import ArrowClient

with ArrowClient("grpc://localhost:50051") as client:
    # Query returns Arrow Table (zero-copy)
    table = client.query("MATCH (n:Person) RETURN n.name, n.age")

    # Convert to pandas
    df = table.to_pandas()

    # Or use with Polars
    import polars as pl
    df = pl.from_arrow(table)
```

## API Reference

See the docstrings in each client class for detailed API documentation.

## Other Client Libraries

AstraeaDB also provides clients for other languages:

- **Go** — `go get github.com/AstraeaDB/R-AstraeaDB` — JSON/TCP, gRPC, and unified client with auto-transport selection. See `go/astraeadb/`.
- **Java** — JSON/TCP, gRPC, Arrow Flight, and unified client with auto-transport selection. Requires Java 17+. See `java/astraeadb/`.
- **R** — JSON/TCP and Arrow Flight client. See `examples/r_client.R`.
- **Rust (embedded)** — Use AstraeaDB directly as a library with no network overhead.
