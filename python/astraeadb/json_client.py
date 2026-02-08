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

        # With authentication:
        with JsonClient("127.0.0.1", 7687, auth_token="secret") as client:
            node_id = client.create_node(["Person"], {"name": "Alice"})
    """

    def __init__(self, host: str = "127.0.0.1", port: int = 7687, auth_token: str | None = None):
        self.host = host
        self.port = port
        self.auth_token = auth_token
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
        # Add auth token if configured
        if self.auth_token:
            request["auth_token"] = self.auth_token
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

    # --- Temporal Queries ---

    def neighbors_at(
        self,
        node_id: int,
        direction: str,
        timestamp: int,
        edge_type: str | None = None,
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
        req: dict[str, Any] = {
            "type": "NeighborsAt",
            "id": node_id,
            "direction": direction,
            "timestamp": timestamp,
        }
        if edge_type is not None:
            req["edge_type"] = edge_type
        data = self._send_ok(req)
        return data.get("neighbors", [])

    def bfs_at(
        self,
        start: int,
        max_depth: int,
        timestamp: int,
    ) -> list[dict]:
        """BFS traversal at a specific point in time.

        Args:
            start: Starting node ID
            max_depth: Maximum traversal depth
            timestamp: Unix timestamp in milliseconds

        Returns:
            List of dicts with node_id and depth
        """
        data = self._send_ok({
            "type": "BfsAt",
            "start": start,
            "max_depth": max_depth,
            "timestamp": timestamp,
        })
        return data.get("nodes", [])

    def shortest_path_at(
        self,
        from_node: int,
        to_node: int,
        timestamp: int,
        weighted: bool = False,
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
        return self._send_ok({
            "type": "ShortestPathAt",
            "from": from_node,
            "to": to_node,
            "timestamp": timestamp,
            "weighted": weighted,
        })

    # --- GraphRAG ---

    def extract_subgraph(
        self,
        center: int,
        hops: int = 2,
        max_nodes: int = 50,
        format: str = "structured",
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
        return self._send_ok({
            "type": "ExtractSubgraph",
            "center": center,
            "hops": hops,
            "max_nodes": max_nodes,
            "format": format,
        })

    def graph_rag(
        self,
        question: str,
        anchor: int | None = None,
        question_embedding: list[float] | None = None,
        hops: int = 2,
        max_nodes: int = 50,
        format: str = "structured",
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
        req: dict[str, Any] = {
            "type": "GraphRag",
            "question": question,
            "hops": hops,
            "max_nodes": max_nodes,
            "format": format,
        }
        if anchor is not None:
            req["anchor"] = anchor
        if question_embedding is not None:
            req["question_embedding"] = question_embedding
        return self._send_ok(req)

    # --- Batch Operations ---

    def create_nodes(self, nodes: list[dict]) -> list[int]:
        """Create multiple nodes.

        Args:
            nodes: List of dicts with keys: labels, properties (optional), embedding (optional)

        Returns:
            List of created node IDs
        """
        return [
            self.create_node(
                n["labels"],
                n.get("properties"),
                n.get("embedding"),
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
                e.get("valid_to"),
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
