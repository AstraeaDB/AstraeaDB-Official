"""Unified AstraeaDB client that auto-selects the best transport."""

from typing import Any, Optional
from astraeadb.json_client import JsonClient


class AstraeaClient:
    """Unified AstraeaDB client.

    Uses Arrow Flight when pyarrow is available (for query results),
    falling back to JSON/TCP otherwise. CRUD operations always use
    JSON/TCP as they are simple request-response.

    Usage:
        with AstraeaClient() as client:
            node_id = client.create_node(["Person"], {"name": "Alice"})
            results = client.query("MATCH (n:Person) RETURN n.name")
    """

    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 7687,
        flight_uri: Optional[str] = None,
        auth_token: Optional[str] = None,
    ):
        """Create a new AstraeaDB client.

        Args:
            host: Server host for JSON/TCP protocol
            port: Server port for JSON/TCP protocol
            flight_uri: Arrow Flight URI (e.g., "grpc://localhost:50051").
                        If None and pyarrow is available, defaults to
                        grpc://{host}:50051
            auth_token: Optional authentication token for RBAC
        """
        self._json = JsonClient(host, port, auth_token)
        self._arrow = None
        self._flight_uri = flight_uri or f"grpc://{host}:50051"

        try:
            from astraeadb.arrow_client import ArrowClient
            self._arrow_cls = ArrowClient
        except ImportError:
            self._arrow_cls = None

    def connect(self) -> None:
        """Establish connections."""
        self._json.connect()
        if self._arrow_cls is not None:
            try:
                self._arrow = self._arrow_cls(self._flight_uri)
                self._arrow.connect()
            except Exception:
                self._arrow = None  # Flight server not available

    def close(self) -> None:
        """Close all connections."""
        self._json.close()
        if self._arrow:
            self._arrow.close()
            self._arrow = None

    def __enter__(self):
        self.connect()
        return self

    def __exit__(self, *args):
        self.close()

    # --- Delegate CRUD to JSON client ---

    def ping(self) -> dict:
        return self._json.ping()

    def create_node(self, labels, properties=None, embedding=None) -> int:
        return self._json.create_node(labels, properties, embedding)

    def get_node(self, node_id: int) -> dict:
        return self._json.get_node(node_id)

    def update_node(self, node_id: int, properties: dict) -> None:
        self._json.update_node(node_id, properties)

    def delete_node(self, node_id: int) -> None:
        self._json.delete_node(node_id)

    def create_edge(self, source, target, edge_type, properties=None, weight=1.0, valid_from=None, valid_to=None) -> int:
        return self._json.create_edge(source, target, edge_type, properties, weight, valid_from, valid_to)

    def get_edge(self, edge_id: int) -> dict:
        return self._json.get_edge(edge_id)

    def update_edge(self, edge_id: int, properties: dict) -> None:
        self._json.update_edge(edge_id, properties)

    def delete_edge(self, edge_id: int) -> None:
        self._json.delete_edge(edge_id)

    # --- Traversals ---

    def neighbors(self, node_id, direction="outgoing", edge_type=None):
        return self._json.neighbors(node_id, direction, edge_type)

    def bfs(self, start, max_depth=3):
        return self._json.bfs(start, max_depth)

    def shortest_path(self, from_node, to_node, weighted=False):
        return self._json.shortest_path(from_node, to_node, weighted)

    # --- Query (use Arrow if available) ---

    def query(self, gql: str):
        """Execute a GQL query.

        Returns an Arrow Table if pyarrow is available, otherwise a dict.
        """
        if self._arrow is not None:
            try:
                return self._arrow.query(gql)
            except Exception:
                pass  # Fall back to JSON
        return self._json.query(gql)

    def query_dict(self, gql: str) -> dict:
        """Execute a GQL query. Always returns a dict (JSON protocol)."""
        return self._json.query(gql)

    # --- Vector ---

    def vector_search(self, query_vector, k=10):
        return self._json.vector_search(query_vector, k)

    def hybrid_search(self, anchor, query_vector, max_hops=3, k=10, alpha=0.5):
        return self._json.hybrid_search(anchor, query_vector, max_hops, k, alpha)

    def semantic_neighbors(self, node_id, concept, direction="outgoing", k=10):
        return self._json.semantic_neighbors(node_id, concept, direction, k)

    def semantic_walk(self, start, concept, max_hops=3):
        return self._json.semantic_walk(start, concept, max_hops)

    # --- Temporal Queries ---

    def neighbors_at(self, node_id, direction, timestamp, edge_type=None):
        """Get neighbors at a specific timestamp."""
        return self._json.neighbors_at(node_id, direction, timestamp, edge_type)

    def bfs_at(self, start, max_depth, timestamp):
        """BFS traversal at a specific timestamp."""
        return self._json.bfs_at(start, max_depth, timestamp)

    def shortest_path_at(self, from_node, to_node, timestamp, weighted=False):
        """Shortest path at a specific timestamp."""
        return self._json.shortest_path_at(from_node, to_node, timestamp, weighted)

    # --- GraphRAG ---

    def extract_subgraph(self, center, hops=2, max_nodes=50, format="structured"):
        """Extract subgraph and linearize to text."""
        return self._json.extract_subgraph(center, hops, max_nodes, format)

    def graph_rag(self, question, anchor=None, question_embedding=None, hops=2, max_nodes=50, format="structured"):
        """Execute a GraphRAG query."""
        return self._json.graph_rag(question, anchor, question_embedding, hops, max_nodes, format)

    # --- Batch Operations ---

    def create_nodes(self, nodes):
        """Create multiple nodes."""
        return self._json.create_nodes(nodes)

    def create_edges(self, edges):
        """Create multiple edges."""
        return self._json.create_edges(edges)

    def delete_nodes(self, node_ids):
        """Delete multiple nodes."""
        return self._json.delete_nodes(node_ids)

    def delete_edges(self, edge_ids):
        """Delete multiple edges."""
        return self._json.delete_edges(edge_ids)

    # --- Bulk operations (Arrow only) ---

    def bulk_insert_nodes(self, table):
        """Bulk insert nodes from an Arrow Table. Requires pyarrow."""
        if self._arrow is None:
            raise RuntimeError("bulk_insert_nodes requires pyarrow and an Arrow Flight server")
        return self._arrow.bulk_insert_nodes(table)

    def bulk_insert_edges(self, table):
        """Bulk insert edges from an Arrow Table. Requires pyarrow."""
        if self._arrow is None:
            raise RuntimeError("bulk_insert_edges requires pyarrow and an Arrow Flight server")
        return self._arrow.bulk_insert_edges(table)
