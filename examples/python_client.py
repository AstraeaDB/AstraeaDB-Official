#!/usr/bin/env python3
"""
AstraeaDB Python Client

A demonstration client showing how to interface with AstraeaDB from Python.
Connects via TCP using the newline-delimited JSON protocol.

Usage:
    # Start the server first:
    #   cargo run -p astraea-cli -- serve

    # Then run this script:
    #   python3 examples/python_client.py

    # Or connect to a custom address:
    #   python3 examples/python_client.py --host 127.0.0.1 --port 7687
"""

import json
import socket
import sys
from typing import Any, Optional


class AstraeaClient:
    """Client for communicating with AstraeaDB over TCP."""

    def __init__(self, host: str = "127.0.0.1", port: int = 7687):
        self.host = host
        self.port = port
        self._sock: Optional[socket.socket] = None

    def connect(self):
        """Open a TCP connection to the server."""
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.connect((self.host, self.port))
        self._sock.settimeout(5.0)

    def close(self):
        """Close the connection."""
        if self._sock:
            self._sock.close()
            self._sock = None

    def __enter__(self):
        self.connect()
        return self

    def __exit__(self, *args):
        self.close()

    def _send(self, request: dict) -> dict:
        """Send a JSON request and return the parsed response."""
        if not self._sock:
            raise ConnectionError("Not connected. Call connect() first.")
        line = json.dumps(request) + "\n"
        self._sock.sendall(line.encode("utf-8"))
        # Read response (newline-delimited)
        buf = b""
        while b"\n" not in buf:
            chunk = self._sock.recv(4096)
            if not chunk:
                raise ConnectionError("Server closed connection")
            buf += chunk
        response_line = buf.split(b"\n", 1)[0]
        return json.loads(response_line)

    def _check(self, response: dict) -> Any:
        """Check response status and return data or raise on error."""
        if response.get("status") == "error":
            raise RuntimeError(f"AstraeaDB error: {response.get('message')}")
        return response.get("data")

    # ── Health ──────────────────────────────────────────────

    def ping(self) -> dict:
        """Health check. Returns server version info."""
        return self._check(self._send({"type": "Ping"}))

    # ── Node Operations ────────────────────────────────────

    def create_node(
        self,
        labels: list[str],
        properties: dict,
        embedding: Optional[list[float]] = None,
    ) -> int:
        """Create a node. Returns the node ID."""
        req = {
            "type": "CreateNode",
            "labels": labels,
            "properties": properties,
        }
        if embedding is not None:
            req["embedding"] = embedding
        data = self._check(self._send(req))
        return data["node_id"]

    def get_node(self, node_id: int) -> dict:
        """Get a node by ID."""
        return self._check(self._send({"type": "GetNode", "id": node_id}))

    def update_node(self, node_id: int, properties: dict) -> None:
        """Update a node's properties (merge semantics)."""
        self._check(
            self._send(
                {"type": "UpdateNode", "id": node_id, "properties": properties}
            )
        )

    def delete_node(self, node_id: int) -> None:
        """Delete a node and all its connected edges."""
        self._check(self._send({"type": "DeleteNode", "id": node_id}))

    # ── Edge Operations ────────────────────────────────────

    def create_edge(
        self,
        source: int,
        target: int,
        edge_type: str,
        properties: Optional[dict] = None,
        weight: float = 1.0,
        valid_from: Optional[int] = None,
        valid_to: Optional[int] = None,
    ) -> int:
        """Create an edge. Returns the edge ID.

        Args:
            source: Source node ID.
            target: Target node ID.
            edge_type: Relationship type label.
            properties: Optional JSON properties.
            weight: Edge weight (default 1.0).
            valid_from: Optional temporal start (epoch milliseconds, inclusive).
            valid_to: Optional temporal end (epoch milliseconds, exclusive).
        """
        req = {
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
        data = self._check(self._send(req))
        return data["edge_id"]

    def get_edge(self, edge_id: int) -> dict:
        """Get an edge by ID."""
        return self._check(self._send({"type": "GetEdge", "id": edge_id}))

    def delete_edge(self, edge_id: int) -> None:
        """Delete an edge."""
        self._check(self._send({"type": "DeleteEdge", "id": edge_id}))

    # ── Traversal ──────────────────────────────────────────

    def neighbors(
        self,
        node_id: int,
        direction: str = "outgoing",
        edge_type: Optional[str] = None,
    ) -> list[dict]:
        """Get neighbors of a node.

        Args:
            node_id: The node to query.
            direction: "outgoing", "incoming", or "both".
            edge_type: Optional filter by edge type.
        """
        req: dict = {
            "type": "Neighbors",
            "id": node_id,
            "direction": direction,
        }
        if edge_type is not None:
            req["edge_type"] = edge_type
        data = self._check(self._send(req))
        return data["neighbors"]

    def bfs(self, start: int, max_depth: int = 3) -> list[dict]:
        """Breadth-first search from a node.

        Returns a list of {"node_id": int, "depth": int}.
        """
        data = self._check(
            self._send({"type": "Bfs", "start": start, "max_depth": max_depth})
        )
        return data["nodes"]

    def shortest_path(
        self, from_node: int, to_node: int, weighted: bool = False
    ) -> Optional[dict]:
        """Find the shortest path between two nodes.

        Args:
            from_node: Starting node.
            to_node: Target node.
            weighted: If True, use Dijkstra with edge weights.
        """
        data = self._check(
            self._send(
                {
                    "type": "ShortestPath",
                    "from": from_node,
                    "to": to_node,
                    "weighted": weighted,
                }
            )
        )
        return data


# ── Demo ────────────────────────────────────────────────────


def demo_social_network(client: AstraeaClient):
    """Build and query a small social network graph."""
    print("=" * 60)
    print("AstraeaDB Python Client Demo: Social Network")
    print("=" * 60)

    # ── Create people ──
    print("\n1. Creating nodes (people)...")
    alice = client.create_node(["Person"], {"name": "Alice", "age": 30, "city": "NYC"})
    bob = client.create_node(["Person"], {"name": "Bob", "age": 25, "city": "London"})
    charlie = client.create_node(
        ["Person"], {"name": "Charlie", "age": 35, "city": "Tokyo"}
    )
    diana = client.create_node(
        ["Person"], {"name": "Diana", "age": 28, "city": "Paris"}
    )
    eve = client.create_node(
        ["Person"], {"name": "Eve", "age": 32, "city": "Berlin"}
    )
    print(f"   Created: Alice(id={alice}), Bob(id={bob}), Charlie(id={charlie}), "
          f"Diana(id={diana}), Eve(id={eve})")

    # ── Create relationships ──
    print("\n2. Creating edges (relationships)...")
    client.create_edge(alice, bob, "KNOWS", {"since": 2020}, weight=0.9)
    client.create_edge(alice, charlie, "KNOWS", {"since": 2018}, weight=0.7)
    client.create_edge(bob, diana, "KNOWS", {"since": 2021}, weight=0.8)
    client.create_edge(charlie, diana, "KNOWS", {"since": 2019}, weight=0.6)
    client.create_edge(diana, eve, "KNOWS", {"since": 2022}, weight=0.95)
    client.create_edge(alice, eve, "FOLLOWS", {"since": 2023}, weight=0.3)
    print("   Created 6 edges (5 KNOWS + 1 FOLLOWS)")

    # ── Read back ──
    print("\n3. Reading nodes...")
    alice_data = client.get_node(alice)
    print(f"   Alice: labels={alice_data['labels']}, "
          f"properties={alice_data['properties']}")

    # ── Update ──
    print("\n4. Updating Alice's properties...")
    client.update_node(alice, {"city": "San Francisco", "title": "Engineer"})
    alice_data = client.get_node(alice)
    print(f"   Alice now: {alice_data['properties']}")

    # ── Neighbors ──
    print("\n5. Querying neighbors...")

    out_neighbors = client.neighbors(alice, "outgoing")
    print(f"   Alice's outgoing neighbors: {len(out_neighbors)} connections")
    for n in out_neighbors:
        target = client.get_node(n["node_id"])
        print(f"     -> {target['properties']['name']} (edge_id={n['edge_id']})")

    knows_only = client.neighbors(alice, "outgoing", edge_type="KNOWS")
    print(f"   Alice KNOWS: {len(knows_only)} people")

    incoming = client.neighbors(diana, "incoming")
    print(f"   Who knows Diana: {len(incoming)} people")
    for n in incoming:
        source = client.get_node(n["node_id"])
        print(f"     <- {source['properties']['name']}")

    # ── BFS ──
    print("\n6. BFS traversal from Alice (depth=2)...")
    bfs_result = client.bfs(alice, max_depth=2)
    for entry in bfs_result:
        node = client.get_node(entry["node_id"])
        print(f"   Depth {entry['depth']}: {node['properties']['name']}")

    # ── Shortest path ──
    print("\n7. Shortest path from Alice to Eve...")

    unweighted = client.shortest_path(alice, eve, weighted=False)
    if unweighted.get("path"):
        names = []
        for nid in unweighted["path"]:
            node = client.get_node(nid)
            names.append(node["properties"]["name"])
        print(f"   Unweighted (fewest hops): {' -> '.join(names)} "
              f"({unweighted['length']} hops)")

    weighted = client.shortest_path(alice, eve, weighted=True)
    if weighted.get("path"):
        names = []
        for nid in weighted["path"]:
            node = client.get_node(nid)
            names.append(node["properties"]["name"])
        print(f"   Weighted (lowest cost):   {' -> '.join(names)} "
              f"(cost={weighted['cost']:.2f})")

    # ── Delete ──
    print("\n8. Deleting Eve...")
    client.delete_node(eve)
    result = client.shortest_path(alice, eve, weighted=False)
    if result.get("path") is None:
        print("   No path from Alice to Eve (Eve was deleted)")

    # ── Ping ──
    print("\n9. Server health check...")
    status = client.ping()
    print(f"   Server version: {status.get('version')}, pong: {status.get('pong')}")

    print("\n" + "=" * 60)
    print("Demo complete.")
    print("=" * 60)


def main():
    import argparse

    parser = argparse.ArgumentParser(description="AstraeaDB Python Client Demo")
    parser.add_argument("--host", default="127.0.0.1", help="Server host")
    parser.add_argument("--port", type=int, default=7687, help="Server port")
    args = parser.parse_args()

    try:
        with AstraeaClient(args.host, args.port) as client:
            demo_social_network(client)
    except ConnectionRefusedError:
        print(
            f"Could not connect to AstraeaDB at {args.host}:{args.port}",
            file=sys.stderr,
        )
        print(
            "Start the server first: cargo run -p astraea-cli -- serve",
            file=sys.stderr,
        )
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
