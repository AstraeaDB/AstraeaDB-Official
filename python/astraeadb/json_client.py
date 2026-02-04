"""JSON/TCP client for AstraeaDB."""

from __future__ import annotations

import json
import socket
from typing import Any, Optional


class JsonClient:
    """AstraeaDB client using newline-delimited JSON over TCP.

    This client has no external dependencies beyond the Python standard library.

    Usage:
        with JsonClient("127.0.0.1", 7687) as client:
            node_id = client.create_node(["Person"], {"name": "Alice"})
    """

    def __init__(self, host: str = "127.0.0.1", port: int = 7687):
        self.host = host
        self.port = port
        self._sock: Optional[socket.socket] = None

    def connect(self) -> None:
        """Open the TCP connection."""
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.connect((self.host, self.port))

    def close(self) -> None:
        """Close the TCP connection."""
        if self._sock:
            self._sock.close()
            self._sock = None

    def __enter__(self):
        self.connect()
        return self

    def __exit__(self, *args):
        self.close()

    def _send(self, request: dict) -> dict:
        """Send a request and return the response."""
        if not self._sock:
            raise ConnectionError("not connected; call connect() or use context manager")
        data = json.dumps(request) + "\n"
        self._sock.sendall(data.encode("utf-8"))
        # Read response (single line)
        buf = b""
        while b"\n" not in buf:
            chunk = self._sock.recv(4096)
            if not chunk:
                raise ConnectionError("server closed connection")
            buf += chunk
        line = buf.split(b"\n", 1)[0]
        return json.loads(line)

    def _send_ok(self, request: dict) -> dict:
        """Send a request and return the data, raising on error."""
        resp = self._send(request)
        if resp.get("status") == "error":
            raise RuntimeError(f"AstraeaDB error: {resp.get('message', 'unknown')}")
        return resp.get("data", {})

    # --- CRUD ---

    def ping(self) -> dict:
        """Health check. Returns server version."""
        return self._send_ok({"type": "Ping"})

    def create_node(
        self,
        labels: list[str],
        properties: dict | None = None,
        embedding: list[float] | None = None,
    ) -> int:
        """Create a node. Returns the node ID."""
        req: dict[str, Any] = {
            "type": "CreateNode",
            "labels": labels,
            "properties": properties or {},
        }
        if embedding is not None:
            req["embedding"] = embedding
        data = self._send_ok(req)
        return data["node_id"]

    def get_node(self, node_id: int) -> dict:
        """Get a node by ID."""
        return self._send_ok({"type": "GetNode", "id": node_id})

    def update_node(self, node_id: int, properties: dict) -> None:
        """Update a node's properties (merge semantics)."""
        self._send_ok({"type": "UpdateNode", "id": node_id, "properties": properties})

    def delete_node(self, node_id: int) -> None:
        """Delete a node and all its connected edges."""
        self._send_ok({"type": "DeleteNode", "id": node_id})

    def create_edge(
        self,
        source: int,
        target: int,
        edge_type: str,
        properties: dict | None = None,
        weight: float = 1.0,
        valid_from: int | None = None,
        valid_to: int | None = None,
    ) -> int:
        """Create an edge. Returns the edge ID."""
        req: dict[str, Any] = {
            "type": "CreateEdge",
            "source": source,
            "target": target,
            "edge_type": edge_type,
            "properties": properties or {},
            "weight": weight,
        }
        if valid_from is not None:
            req["valid_from"] = valid_from
        if valid_to is not None:
            req["valid_to"] = valid_to
        data = self._send_ok(req)
        return data["edge_id"]

    def get_edge(self, edge_id: int) -> dict:
        """Get an edge by ID."""
        return self._send_ok({"type": "GetEdge", "id": edge_id})

    def update_edge(self, edge_id: int, properties: dict) -> None:
        """Update an edge's properties (merge semantics)."""
        self._send_ok({"type": "UpdateEdge", "id": edge_id, "properties": properties})

    def delete_edge(self, edge_id: int) -> None:
        """Delete an edge."""
        self._send_ok({"type": "DeleteEdge", "id": edge_id})

    # --- Traversals ---

    def neighbors(
        self,
        node_id: int,
        direction: str = "outgoing",
        edge_type: str | None = None,
    ) -> list[dict]:
        """Get neighbors of a node."""
        req: dict[str, Any] = {
            "type": "Neighbors",
            "id": node_id,
            "direction": direction,
        }
        if edge_type is not None:
            req["edge_type"] = edge_type
        data = self._send_ok(req)
        return data.get("neighbors", [])

    def bfs(self, start: int, max_depth: int = 3) -> list[dict]:
        """Breadth-first traversal from a node."""
        data = self._send_ok({"type": "Bfs", "start": start, "max_depth": max_depth})
        return data.get("nodes", [])

    def shortest_path(self, from_node: int, to_node: int, weighted: bool = False) -> dict:
        """Find shortest path between two nodes."""
        return self._send_ok({
            "type": "ShortestPath",
            "from": from_node,
            "to": to_node,
            "weighted": weighted,
        })

    # --- Query ---

    def query(self, gql: str) -> dict:
        """Execute a GQL query. Returns {columns, rows, stats}."""
        return self._send_ok({"type": "Query", "gql": gql})

    # --- Vector ---

    def vector_search(self, query_vector: list[float], k: int = 10) -> list[dict]:
        """k-nearest-neighbor vector search."""
        data = self._send_ok({"type": "VectorSearch", "query": query_vector, "k": k})
        return data.get("results", [])

    # --- Hybrid / Semantic (Phase 2) ---

    def hybrid_search(
        self,
        anchor: int,
        query_vector: list[float],
        max_hops: int = 3,
        k: int = 10,
        alpha: float = 0.5,
    ) -> list[dict]:
        """Hybrid graph + vector search."""
        data = self._send_ok({
            "type": "HybridSearch",
            "anchor": anchor,
            "query": query_vector,
            "max_hops": max_hops,
            "k": k,
            "alpha": alpha,
        })
        return data.get("results", [])

    def semantic_neighbors(
        self,
        node_id: int,
        concept: list[float],
        direction: str = "outgoing",
        k: int = 10,
    ) -> list[dict]:
        """Find neighbors ranked by semantic similarity."""
        data = self._send_ok({
            "type": "SemanticNeighbors",
            "id": node_id,
            "concept": concept,
            "direction": direction,
            "k": k,
        })
        return data.get("results", [])

    def semantic_walk(
        self,
        start: int,
        concept: list[float],
        max_hops: int = 3,
    ) -> list[dict]:
        """Greedy semantic walk toward a concept."""
        data = self._send_ok({
            "type": "SemanticWalk",
            "start": start,
            "concept": concept,
            "max_hops": max_hops,
        })
        return data.get("path", [])
