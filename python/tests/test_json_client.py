"""Unit tests for JsonClient.

These tests mock the TCP socket to verify request/response formatting
without requiring a running server.
"""

import json
import unittest
from unittest.mock import MagicMock, patch
from astraeadb.json_client import JsonClient


class TestJsonClient(unittest.TestCase):
    def setUp(self):
        self.client = JsonClient("127.0.0.1", 7687)
        self.mock_sock = MagicMock()
        self.client._sock = self.mock_sock

    def _mock_response(self, data: dict):
        """Configure mock socket to return a JSON response."""
        response = json.dumps({"status": "ok", "data": data}) + "\n"
        self.mock_sock.recv.return_value = response.encode("utf-8")

    def _mock_error(self, message: str):
        response = json.dumps({"status": "error", "message": message}) + "\n"
        self.mock_sock.recv.return_value = response.encode("utf-8")

    def _get_sent_request(self) -> dict:
        """Extract the JSON request sent to the socket."""
        call_args = self.mock_sock.sendall.call_args
        data = call_args[0][0].decode("utf-8").strip()
        return json.loads(data)

    def test_ping(self):
        self._mock_response({"pong": True, "version": "0.1.0"})
        result = self.client.ping()
        req = self._get_sent_request()
        self.assertEqual(req["type"], "Ping")
        self.assertTrue(result["pong"])

    def test_create_node(self):
        self._mock_response({"node_id": 42})
        node_id = self.client.create_node(["Person"], {"name": "Alice"})
        req = self._get_sent_request()
        self.assertEqual(req["type"], "CreateNode")
        self.assertEqual(req["labels"], ["Person"])
        self.assertEqual(node_id, 42)

    def test_create_node_with_embedding(self):
        self._mock_response({"node_id": 1})
        self.client.create_node(["Thing"], {}, embedding=[0.1, 0.2])
        req = self._get_sent_request()
        self.assertEqual(req["embedding"], [0.1, 0.2])

    def test_create_edge(self):
        self._mock_response({"edge_id": 10})
        edge_id = self.client.create_edge(1, 2, "KNOWS", weight=0.9)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "CreateEdge")
        self.assertEqual(req["source"], 1)
        self.assertEqual(req["target"], 2)
        self.assertEqual(edge_id, 10)

    def test_create_edge_with_temporal(self):
        self._mock_response({"edge_id": 11})
        edge_id = self.client.create_edge(
            1, 2, "KNOWS", valid_from=1000, valid_to=2000
        )
        req = self._get_sent_request()
        self.assertEqual(req["valid_from"], 1000)
        self.assertEqual(req["valid_to"], 2000)
        self.assertEqual(edge_id, 11)

    def test_get_node(self):
        self._mock_response({"id": 1, "labels": ["Person"], "properties": {"name": "Alice"}})
        result = self.client.get_node(1)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "GetNode")
        self.assertEqual(req["id"], 1)
        self.assertEqual(result["labels"], ["Person"])

    def test_update_node(self):
        self._mock_response({})
        self.client.update_node(1, {"city": "NYC"})
        req = self._get_sent_request()
        self.assertEqual(req["type"], "UpdateNode")
        self.assertEqual(req["properties"], {"city": "NYC"})

    def test_delete_node(self):
        self._mock_response({})
        self.client.delete_node(1)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "DeleteNode")
        self.assertEqual(req["id"], 1)

    def test_get_edge(self):
        self._mock_response({"id": 10, "source": 1, "target": 2, "edge_type": "KNOWS"})
        result = self.client.get_edge(10)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "GetEdge")
        self.assertEqual(result["edge_type"], "KNOWS")

    def test_update_edge(self):
        self._mock_response({})
        self.client.update_edge(10, {"weight": 0.5})
        req = self._get_sent_request()
        self.assertEqual(req["type"], "UpdateEdge")
        self.assertEqual(req["id"], 10)

    def test_delete_edge(self):
        self._mock_response({})
        self.client.delete_edge(10)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "DeleteEdge")
        self.assertEqual(req["id"], 10)

    def test_neighbors(self):
        self._mock_response({"neighbors": [{"node_id": 2, "edge_id": 10}]})
        result = self.client.neighbors(1, "outgoing")
        req = self._get_sent_request()
        self.assertEqual(req["type"], "Neighbors")
        self.assertEqual(req["direction"], "outgoing")
        self.assertEqual(len(result), 1)

    def test_neighbors_with_edge_type(self):
        self._mock_response({"neighbors": []})
        self.client.neighbors(1, "outgoing", edge_type="KNOWS")
        req = self._get_sent_request()
        self.assertEqual(req["edge_type"], "KNOWS")

    def test_bfs(self):
        self._mock_response({"nodes": [{"node_id": 1, "depth": 0}, {"node_id": 2, "depth": 1}]})
        result = self.client.bfs(1, max_depth=2)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "Bfs")
        self.assertEqual(req["max_depth"], 2)
        self.assertEqual(len(result), 2)

    def test_shortest_path(self):
        self._mock_response({"path": [1, 3, 5], "length": 2})
        result = self.client.shortest_path(1, 5, weighted=False)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "ShortestPath")
        self.assertEqual(req["from"], 1)
        self.assertEqual(req["to"], 5)
        self.assertEqual(result["length"], 2)

    def test_query(self):
        self._mock_response({
            "columns": ["n.name"],
            "rows": [["Alice"], ["Bob"]],
            "stats": {"nodes_created": 0},
        })
        result = self.client.query("MATCH (n:Person) RETURN n.name")
        req = self._get_sent_request()
        self.assertEqual(req["type"], "Query")
        self.assertEqual(req["gql"], "MATCH (n:Person) RETURN n.name")
        self.assertEqual(len(result["rows"]), 2)

    def test_vector_search(self):
        self._mock_response({"results": [{"node_id": 1, "distance": 0.1}]})
        results = self.client.vector_search([0.1, 0.2], k=5)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "VectorSearch")
        self.assertEqual(req["k"], 5)
        self.assertEqual(len(results), 1)

    def test_error_response(self):
        self._mock_error("node 999 not found")
        with self.assertRaises(RuntimeError) as ctx:
            self.client.get_node(999)
        self.assertIn("node 999 not found", str(ctx.exception))

    def test_hybrid_search(self):
        self._mock_response({"results": [{"node_id": 1, "score": 0.3}]})
        results = self.client.hybrid_search(1, [0.1, 0.2], max_hops=2, k=5, alpha=0.7)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "HybridSearch")
        self.assertEqual(req["alpha"], 0.7)

    def test_semantic_neighbors(self):
        self._mock_response({"results": [{"node_id": 2, "similarity": 0.9}]})
        results = self.client.semantic_neighbors(1, [0.1, 0.2], direction="outgoing", k=5)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "SemanticNeighbors")
        self.assertEqual(req["direction"], "outgoing")
        self.assertEqual(req["k"], 5)

    def test_semantic_walk(self):
        self._mock_response({"path": [{"node_id": 1}, {"node_id": 3}, {"node_id": 5}]})
        result = self.client.semantic_walk(1, [0.1, 0.2], max_hops=3)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "SemanticWalk")
        self.assertEqual(req["max_hops"], 3)
        self.assertEqual(len(result), 3)

    def test_not_connected_raises(self):
        client = JsonClient()
        with self.assertRaises(ConnectionError):
            client.ping()

    def test_context_manager(self):
        with patch("socket.socket") as mock_socket_cls:
            mock_sock = MagicMock()
            mock_socket_cls.return_value = mock_sock
            with JsonClient("localhost", 7687) as client:
                self.assertIsNotNone(client._sock)
            mock_sock.close.assert_called_once()


class TestTemporalQueries(unittest.TestCase):
    """Tests for temporal query methods."""

    def setUp(self):
        self.client = JsonClient("127.0.0.1", 7687)
        self.mock_sock = MagicMock()
        self.client._sock = self.mock_sock

    def _mock_response(self, data: dict):
        response = json.dumps({"status": "ok", "data": data}) + "\n"
        self.mock_sock.recv.return_value = response.encode("utf-8")

    def _get_sent_request(self) -> dict:
        call_args = self.mock_sock.sendall.call_args
        data = call_args[0][0].decode("utf-8").strip()
        return json.loads(data)

    def test_neighbors_at(self):
        self._mock_response({"neighbors": [{"node_id": 2, "edge_id": 10}]})
        result = self.client.neighbors_at(1, "outgoing", 1700000000000)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "NeighborsAt")
        self.assertEqual(req["id"], 1)
        self.assertEqual(req["direction"], "outgoing")
        self.assertEqual(req["timestamp"], 1700000000000)
        self.assertEqual(len(result), 1)

    def test_neighbors_at_with_edge_type(self):
        self._mock_response({"neighbors": []})
        self.client.neighbors_at(1, "both", 1700000000000, edge_type="KNOWS")
        req = self._get_sent_request()
        self.assertEqual(req["edge_type"], "KNOWS")

    def test_bfs_at(self):
        self._mock_response({"nodes": [{"node_id": 1, "depth": 0}, {"node_id": 2, "depth": 1}]})
        result = self.client.bfs_at(1, 2, 1700000000000)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "BfsAt")
        self.assertEqual(req["start"], 1)
        self.assertEqual(req["max_depth"], 2)
        self.assertEqual(req["timestamp"], 1700000000000)
        self.assertEqual(len(result), 2)

    def test_shortest_path_at(self):
        self._mock_response({"path": [1, 3, 5], "length": 2})
        result = self.client.shortest_path_at(1, 5, 1700000000000)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "ShortestPathAt")
        self.assertEqual(req["from"], 1)
        self.assertEqual(req["to"], 5)
        self.assertEqual(req["timestamp"], 1700000000000)
        self.assertEqual(result["length"], 2)

    def test_shortest_path_at_weighted(self):
        self._mock_response({"path": [1, 3, 5], "cost": 2.5})
        result = self.client.shortest_path_at(1, 5, 1700000000000, weighted=True)
        req = self._get_sent_request()
        self.assertEqual(req["weighted"], True)


class TestGraphRAG(unittest.TestCase):
    """Tests for GraphRAG methods."""

    def setUp(self):
        self.client = JsonClient("127.0.0.1", 7687)
        self.mock_sock = MagicMock()
        self.client._sock = self.mock_sock

    def _mock_response(self, data: dict):
        response = json.dumps({"status": "ok", "data": data}) + "\n"
        self.mock_sock.recv.return_value = response.encode("utf-8")

    def _get_sent_request(self) -> dict:
        call_args = self.mock_sock.sendall.call_args
        data = call_args[0][0].decode("utf-8").strip()
        return json.loads(data)

    def test_extract_subgraph_structured(self):
        self._mock_response({
            "center": 1,
            "node_count": 5,
            "edge_count": 4,
            "text": "Node 1 (Person): Alice..."
        })
        result = self.client.extract_subgraph(1, hops=2, max_nodes=50, format="structured")
        req = self._get_sent_request()
        self.assertEqual(req["type"], "ExtractSubgraph")
        self.assertEqual(req["center"], 1)
        self.assertEqual(req["hops"], 2)
        self.assertEqual(req["max_nodes"], 50)
        self.assertEqual(req["format"], "structured")
        self.assertEqual(result["node_count"], 5)

    def test_extract_subgraph_prose(self):
        self._mock_response({"text": "Alice is a person who knows Bob..."})
        self.client.extract_subgraph(1, format="prose")
        req = self._get_sent_request()
        self.assertEqual(req["format"], "prose")

    def test_graph_rag_with_anchor(self):
        self._mock_response({
            "answer": "Alice works at TechCorp.",
            "anchor_node_id": 1,
            "context_text": "Node 1 (Person): Alice...",
            "nodes_in_context": 5,
            "estimated_tokens": 120
        })
        result = self.client.graph_rag("Where does Alice work?", anchor=1)
        req = self._get_sent_request()
        self.assertEqual(req["type"], "GraphRag")
        self.assertEqual(req["question"], "Where does Alice work?")
        self.assertEqual(req["anchor"], 1)
        self.assertIn("answer", result)

    def test_graph_rag_with_embedding(self):
        self._mock_response({
            "answer": "Alice is 30 years old.",
            "anchor_node_id": 1,
            "nodes_in_context": 3,
            "estimated_tokens": 80
        })
        result = self.client.graph_rag(
            "How old is Alice?",
            question_embedding=[0.1, 0.2, 0.3],
            hops=3,
            max_nodes=25
        )
        req = self._get_sent_request()
        self.assertEqual(req["question_embedding"], [0.1, 0.2, 0.3])
        self.assertEqual(req["hops"], 3)
        self.assertEqual(req["max_nodes"], 25)


class TestBatchOperations(unittest.TestCase):
    """Tests for batch operation methods."""

    def setUp(self):
        self.client = JsonClient("127.0.0.1", 7687)
        self.mock_sock = MagicMock()
        self.client._sock = self.mock_sock
        self.call_count = 0

    def _mock_responses(self, responses: list[dict]):
        """Configure mock to return multiple sequential responses."""
        encoded = [json.dumps({"status": "ok", "data": r}) + "\n" for r in responses]
        self.mock_sock.recv.side_effect = [r.encode("utf-8") for r in encoded]

    def test_create_nodes_batch(self):
        self._mock_responses([
            {"node_id": 1},
            {"node_id": 2},
            {"node_id": 3}
        ])
        nodes = [
            {"labels": ["Person"], "properties": {"name": "Alice"}},
            {"labels": ["Person"], "properties": {"name": "Bob"}},
            {"labels": ["Company"], "properties": {"name": "TechCorp"}, "embedding": [0.1, 0.2]}
        ]
        result = self.client.create_nodes(nodes)
        self.assertEqual(result, [1, 2, 3])
        self.assertEqual(self.mock_sock.sendall.call_count, 3)

    def test_create_edges_batch(self):
        self._mock_responses([
            {"edge_id": 10},
            {"edge_id": 11}
        ])
        edges = [
            {"source": 1, "target": 2, "edge_type": "KNOWS"},
            {"source": 2, "target": 3, "edge_type": "WORKS_AT", "weight": 0.9}
        ]
        result = self.client.create_edges(edges)
        self.assertEqual(result, [10, 11])

    def test_delete_nodes_batch(self):
        self._mock_responses([{}, {}])
        count = self.client.delete_nodes([1, 2])
        self.assertEqual(count, 2)

    def test_delete_edges_batch(self):
        self._mock_responses([{}, {}])
        count = self.client.delete_edges([10, 11])
        self.assertEqual(count, 2)


class TestAuthentication(unittest.TestCase):
    """Tests for authentication support."""

    def setUp(self):
        self.mock_sock = MagicMock()

    def _mock_response(self, data: dict):
        response = json.dumps({"status": "ok", "data": data}) + "\n"
        self.mock_sock.recv.return_value = response.encode("utf-8")

    def _get_sent_request(self) -> dict:
        call_args = self.mock_sock.sendall.call_args
        data = call_args[0][0].decode("utf-8").strip()
        return json.loads(data)

    def test_auth_token_sent(self):
        client = JsonClient("127.0.0.1", 7687, auth_token="secret-token-123")
        client._sock = self.mock_sock
        self._mock_response({"pong": True})
        client.ping()
        req = self._get_sent_request()
        self.assertEqual(req["auth_token"], "secret-token-123")

    def test_no_auth_token_when_not_set(self):
        client = JsonClient("127.0.0.1", 7687)
        client._sock = self.mock_sock
        self._mock_response({"pong": True})
        client.ping()
        req = self._get_sent_request()
        self.assertNotIn("auth_token", req)


if __name__ == "__main__":
    unittest.main()
