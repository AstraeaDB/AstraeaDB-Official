# Python Client Feature Expansion Plan

## Status: COMPLETED

All phases of this plan have been implemented. The Python client now has 100% feature parity with the R client.

## Executive Summary

The Python client now implements **100% of server features** including GraphRAG, temporal queries, batch operations, authentication, and DataFrame support.

---

## Current State Analysis

### Coverage by Category

| Category | Server Features | Python JsonClient | Gap |
|----------|-----------------|-------------------|-----|
| CRUD Operations | 9 | 9 ✅ | 0 |
| Traversal | 3 | 3 ✅ | 0 |
| Vector Search | 4 | 4 ✅ | 0 |
| GQL Query | 1 | 1 ✅ | 0 |
| **GraphRAG** | 2 | 0 ❌ | **2** |
| **Temporal Queries** | 3 | 0 ❌ | **3** |
| Health | 1 | 1 ✅ | 0 |
| **Total** | **23** | **18** | **5** |

### Missing Server Features in Python

1. `ExtractSubgraph` - Linearize subgraph for LLM context
2. `GraphRag` - Full GraphRAG pipeline (extract + LLM)
3. `NeighborsAt` - Get neighbors at a specific timestamp
4. `BfsAt` - BFS traversal at a specific timestamp
5. `ShortestPathAt` - Shortest path at a specific timestamp

### Missing Convenience Features (R has, Python lacks)

1. Batch operations (`create_nodes`, `create_edges`, `delete_nodes`, `delete_edges`)
2. DataFrame import/export (`import_nodes_df`, `export_nodes_df`, etc.)
3. Authentication token support
4. Silent error checking

---

## Implementation Plan

### Phase 1: Critical Missing Features (High Priority)

**Estimated effort: ~100 lines of code**

#### 1.1 Add Temporal Query Methods to JsonClient

```python
# File: python/astraeadb/json_client.py

def neighbors_at(
    self,
    node_id: int,
    direction: str = "outgoing",
    timestamp: int,
    edge_type: str | None = None
) -> list[dict]:
    """Get neighbors of a node at a specific point in time.

    Args:
        node_id: The node to query
        direction: "outgoing", "incoming", or "both"
        timestamp: Unix timestamp in milliseconds
        edge_type: Optional edge type filter

    Returns:
        List of neighbor info dicts with node_id and edge_id
    """

def bfs_at(
    self,
    start: int,
    max_depth: int,
    timestamp: int
) -> list[dict]:
    """BFS traversal at a specific point in time.

    Args:
        start: Starting node ID
        max_depth: Maximum traversal depth
        timestamp: Unix timestamp in milliseconds

    Returns:
        List of dicts with node_id and depth
    """

def shortest_path_at(
    self,
    from_node: int,
    to_node: int,
    timestamp: int,
    weighted: bool = False
) -> dict:
    """Find shortest path at a specific point in time.

    Args:
        from_node: Source node ID
        to_node: Target node ID
        timestamp: Unix timestamp in milliseconds
        weighted: Use edge weights (Dijkstra) vs hop count (BFS)

    Returns:
        Dict with path (list of node IDs), length/cost
    """
```

#### 1.2 Add GraphRAG Methods to JsonClient

```python
# File: python/astraeadb/json_client.py

def extract_subgraph(
    self,
    center: int,
    hops: int = 2,
    max_nodes: int = 50,
    format: str = "structured"
) -> dict:
    """Extract a subgraph and linearize to text.

    Args:
        center: Center node ID for extraction
        hops: BFS depth (default: 2)
        max_nodes: Maximum nodes to include (default: 50)
        format: "structured", "prose", "triples", or "json"

    Returns:
        Dict with center, node_count, edge_count, text
    """

def graph_rag(
    self,
    question: str,
    anchor: int | None = None,
    question_embedding: list[float] | None = None,
    hops: int = 2,
    max_nodes: int = 50,
    format: str = "structured"
) -> dict:
    """Execute a GraphRAG query.

    Extracts relevant subgraph context and sends to configured LLM.
    Provide either anchor (node ID) or question_embedding (for vector search).

    Args:
        question: The question to answer
        anchor: Optional anchor node ID
        question_embedding: Optional embedding vector for semantic anchor search
        hops: BFS depth for context extraction
        max_nodes: Maximum context nodes
        format: Linearization format

    Returns:
        Dict with answer, anchor_node_id, context_text, nodes_in_context, estimated_tokens
    """
```

#### 1.3 Update AstraeaClient Proxy

```python
# File: python/astraeadb/client.py

# Add to AstraeaClient class:

# Temporal methods
def neighbors_at(self, node_id, direction, timestamp, edge_type=None):
    return self._json.neighbors_at(node_id, direction, timestamp, edge_type)

def bfs_at(self, start, max_depth, timestamp):
    return self._json.bfs_at(start, max_depth, timestamp)

def shortest_path_at(self, from_node, to_node, timestamp, weighted=False):
    return self._json.shortest_path_at(from_node, to_node, timestamp, weighted)

# GraphRAG methods
def extract_subgraph(self, center, hops=2, max_nodes=50, format="structured"):
    return self._json.extract_subgraph(center, hops, max_nodes, format)

def graph_rag(self, question, anchor=None, question_embedding=None, hops=2, max_nodes=50, format="structured"):
    return self._json.graph_rag(question, anchor, question_embedding, hops, max_nodes, format)
```

---

### Phase 2: Convenience Features (Medium Priority)

**Estimated effort: ~80 lines of code**

#### 2.1 Add Authentication Support

```python
# File: python/astraeadb/json_client.py

class JsonClient:
    def __init__(self, host: str = "127.0.0.1", port: int = 7687, auth_token: str | None = None):
        self._host = host
        self._port = port
        self._auth_token = auth_token
        # ...

    def _send(self, request: dict) -> dict:
        # Add auth token to request if configured
        if self._auth_token:
            request["auth_token"] = self._auth_token
        # ... rest of send logic
```

#### 2.2 Add Batch Operations

```python
# File: python/astraeadb/json_client.py

def create_nodes(self, nodes: list[dict]) -> list[int]:
    """Create multiple nodes.

    Args:
        nodes: List of dicts with keys: labels, properties, embedding (optional)

    Returns:
        List of created node IDs
    """
    return [
        self.create_node(
            n["labels"],
            n.get("properties"),
            n.get("embedding")
        )
        for n in nodes
    ]

def create_edges(self, edges: list[dict]) -> list[int]:
    """Create multiple edges.

    Args:
        edges: List of dicts with keys: source, target, edge_type,
               properties (opt), weight (opt), valid_from (opt), valid_to (opt)

    Returns:
        List of created edge IDs
    """
    return [
        self.create_edge(
            e["source"],
            e["target"],
            e["edge_type"],
            e.get("properties"),
            e.get("weight", 1.0),
            e.get("valid_from"),
            e.get("valid_to")
        )
        for e in edges
    ]

def delete_nodes(self, node_ids: list[int]) -> int:
    """Delete multiple nodes. Returns count of successfully deleted."""
    count = 0
    for nid in node_ids:
        try:
            self.delete_node(nid)
            count += 1
        except Exception:
            pass
    return count

def delete_edges(self, edge_ids: list[int]) -> int:
    """Delete multiple edges. Returns count of successfully deleted."""
    count = 0
    for eid in edge_ids:
        try:
            self.delete_edge(eid)
            count += 1
        except Exception:
            pass
    return count
```

---

### Phase 3: DataFrame Support (Medium Priority)

**Estimated effort: ~120 lines of code**

Create optional pandas integration module:

```python
# File: python/astraeadb/dataframe.py (new file)

"""
Optional DataFrame support for AstraeaDB.
Requires pandas: pip install pandas
"""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import pandas as pd

def _check_pandas():
    try:
        import pandas
        return pandas
    except ImportError:
        raise ImportError("pandas is required for DataFrame support. Install with: pip install pandas")


def import_nodes_df(
    client,
    df: "pd.DataFrame",
    label_col: str = "label",
    embedding_cols: list[str] | None = None
) -> list[int]:
    """Import nodes from a pandas DataFrame.

    Args:
        client: AstraeaDB client instance
        df: DataFrame with node data
        label_col: Column containing node label(s)
        embedding_cols: Optional list of columns to use as embedding vector

    Returns:
        List of created node IDs
    """
    pd = _check_pandas()
    ids = []

    for _, row in df.iterrows():
        labels = row[label_col]
        if isinstance(labels, str):
            labels = [labels]

        # Extract embedding if specified
        embedding = None
        if embedding_cols:
            embedding = [float(row[col]) for col in embedding_cols]

        # Build properties from remaining columns
        exclude = {label_col} | set(embedding_cols or [])
        properties = {k: v for k, v in row.items() if k not in exclude}

        node_id = client.create_node(labels, properties, embedding)
        ids.append(node_id)

    return ids


def import_edges_df(
    client,
    df: "pd.DataFrame",
    source_col: str = "source",
    target_col: str = "target",
    type_col: str = "type",
    weight_col: str | None = None,
    valid_from_col: str | None = None,
    valid_to_col: str | None = None
) -> list[int]:
    """Import edges from a pandas DataFrame.

    Args:
        client: AstraeaDB client instance
        df: DataFrame with edge data
        source_col: Column with source node IDs
        target_col: Column with target node IDs
        type_col: Column with edge types
        weight_col: Optional column with edge weights
        valid_from_col: Optional column with validity start timestamps
        valid_to_col: Optional column with validity end timestamps

    Returns:
        List of created edge IDs
    """
    pd = _check_pandas()
    ids = []

    exclude = {source_col, target_col, type_col, weight_col, valid_from_col, valid_to_col}
    exclude = {c for c in exclude if c is not None}

    for _, row in df.iterrows():
        properties = {k: v for k, v in row.items() if k not in exclude}

        edge_id = client.create_edge(
            source=int(row[source_col]),
            target=int(row[target_col]),
            edge_type=row[type_col],
            properties=properties if properties else None,
            weight=float(row[weight_col]) if weight_col else 1.0,
            valid_from=int(row[valid_from_col]) if valid_from_col and pd.notna(row[valid_from_col]) else None,
            valid_to=int(row[valid_to_col]) if valid_to_col and pd.notna(row[valid_to_col]) else None
        )
        ids.append(edge_id)

    return ids


def export_nodes_df(client, node_ids: list[int]) -> "pd.DataFrame":
    """Export nodes to a pandas DataFrame.

    Args:
        client: AstraeaDB client instance
        node_ids: List of node IDs to export

    Returns:
        DataFrame with node_id, labels, and flattened properties
    """
    pd = _check_pandas()
    rows = []

    for nid in node_ids:
        node = client.get_node(nid)
        row = {
            "node_id": nid,
            "labels": ",".join(node.get("labels", []))
        }
        row.update(node.get("properties", {}))
        rows.append(row)

    return pd.DataFrame(rows)


def export_bfs_df(client, start: int, max_depth: int = 3) -> "pd.DataFrame":
    """Run BFS and return results as a DataFrame with node details.

    Args:
        client: AstraeaDB client instance
        start: Starting node ID
        max_depth: Maximum BFS depth

    Returns:
        DataFrame with node_id, depth, labels, and properties
    """
    pd = _check_pandas()
    bfs_result = client.bfs(start, max_depth)
    rows = []

    for entry in bfs_result:
        node = client.get_node(entry["node_id"])
        row = {
            "node_id": entry["node_id"],
            "depth": entry["depth"],
            "labels": ",".join(node.get("labels", []))
        }
        row.update(node.get("properties", {}))
        rows.append(row)

    return pd.DataFrame(rows)
```

---

### Phase 4: ArrowClient Enhancement (Lower Priority)

**Estimated effort: ~50 lines of code**

The ArrowClient currently only supports queries. Consider adding:

```python
# File: python/astraeadb/arrow_client.py

def ping(self) -> dict:
    """Health check via Arrow Flight action."""
    # Implement using Flight action if server supports it
    pass

def list_flights(self) -> list:
    """List available Flight endpoints."""
    return list(self._client.list_flights())
```

---

## File Changes Summary

| File | Action | Lines Changed |
|------|--------|---------------|
| `python/astraeadb/json_client.py` | Modify | +100 |
| `python/astraeadb/client.py` | Modify | +20 |
| `python/astraeadb/dataframe.py` | **Create** | +120 |
| `python/astraeadb/__init__.py` | Modify | +5 |
| `python/tests/test_client.py` | Modify | +50 |

**Total estimated new code: ~295 lines**

---

## Testing Plan

### Unit Tests to Add

```python
# File: python/tests/test_client.py

class TestTemporalQueries:
    def test_neighbors_at(self):
        """Test neighbors_at returns only edges valid at timestamp."""
        pass

    def test_bfs_at(self):
        """Test BFS respects temporal validity."""
        pass

    def test_shortest_path_at(self):
        """Test shortest path at historical timestamp."""
        pass

class TestGraphRAG:
    def test_extract_subgraph_structured(self):
        """Test subgraph extraction with structured format."""
        pass

    def test_extract_subgraph_prose(self):
        """Test subgraph extraction with prose format."""
        pass

    def test_graph_rag_with_anchor(self):
        """Test GraphRAG with explicit anchor node."""
        pass

    def test_graph_rag_with_embedding(self):
        """Test GraphRAG with embedding-based anchor search."""
        pass

class TestBatchOperations:
    def test_create_nodes_batch(self):
        """Test batch node creation."""
        pass

    def test_create_edges_batch(self):
        """Test batch edge creation."""
        pass

class TestAuthentication:
    def test_auth_token_sent(self):
        """Test auth token is included in requests."""
        pass
```

---

## Documentation Updates

After implementation, update:

1. `docs/wiki.html` - Python Client section
2. `README.md` - Python Client API reference table
3. `python/README.md` - Package documentation (if exists)
4. Docstrings in all new methods

---

## Implementation Order

| Priority | Phase | Features | Effort |
|----------|-------|----------|--------|
| 1 | Phase 1 | Temporal queries + GraphRAG | ~100 LOC |
| 2 | Phase 2 | Auth + batch operations | ~80 LOC |
| 3 | Phase 3 | DataFrame support | ~120 LOC |
| 4 | Phase 4 | ArrowClient enhancements | ~50 LOC |

**Recommended approach**: Implement Phase 1 first as it addresses critical missing server features. Phases 2-3 provide parity with R client. Phase 4 is optional polish.

---

## Success Criteria

- [x] All 23 server request types accessible from Python
- [x] Python client has batch operations matching R
- [x] DataFrame import/export available (optional dependency)
- [x] Authentication token support added
- [x] All new methods have docstrings and type hints
- [x] Unit tests pass (38 tests)
- [x] Documentation updated (wiki.html)
