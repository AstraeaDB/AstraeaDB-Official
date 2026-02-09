package com.astraeadb.examples;

import com.astraeadb.json.JsonClient;
import com.astraeadb.json.JsonClientBuilder;
import com.astraeadb.model.*;
import com.astraeadb.options.*;

import java.util.List;
import java.util.Map;

/**
 * Basic CRUD and traversal demo for AstraeaDB.
 *
 * <p>This example connects to a local AstraeaDB instance over JSON/TCP,
 * creates a small social graph (three people connected by KNOWS edges),
 * demonstrates node/edge CRUD, BFS traversal, shortest-path, and GQL
 * query, then cleans up all created resources.
 */
public class BasicExample {
    public static void main(String[] args) throws Exception {
        try (JsonClient client = new JsonClientBuilder()
                .host("127.0.0.1")
                .port(7687)
                .build()) {

            client.connect();
            System.out.println("Connected to AstraeaDB");

            // Ping
            PingResponse ping = client.ping();
            System.out.println("Server version: " + ping.version());

            // Create nodes
            long alice = client.createNode(List.of("Person"), Map.of("name", "Alice", "age", 30));
            long bob = client.createNode(List.of("Person"), Map.of("name", "Bob", "age", 25));
            long charlie = client.createNode(List.of("Person"), Map.of("name", "Charlie", "age", 35));
            System.out.println("Created nodes: Alice=" + alice + ", Bob=" + bob + ", Charlie=" + charlie);

            // Create edges
            long e1 = client.createEdge(alice, bob, "KNOWS", 0.9);
            long e2 = client.createEdge(bob, charlie, "KNOWS", 0.7);
            System.out.println("Created edges: " + e1 + ", " + e2);

            // Get a node
            Node aliceNode = client.getNode(alice);
            System.out.println("Alice: " + aliceNode);

            // Neighbors
            List<NeighborEntry> neighbors = client.neighbors(alice);
            System.out.println("Alice's neighbors: " + neighbors.size());

            // BFS
            List<BfsEntry> bfs = client.bfs(alice, 3);
            System.out.println("BFS from Alice: " + bfs.size() + " nodes discovered");

            // Shortest path
            PathResult path = client.shortestPath(alice, charlie);
            System.out.println("Shortest path Alice->Charlie: " + path.path() + " (length=" + path.length() + ")");

            // GQL Query
            QueryResult result = client.query("MATCH (n:Person) WHERE n.age > 25 RETURN n.name");
            System.out.println("Query result: " + result.columns() + " -> " + result.rows().size() + " rows");

            // Cleanup
            client.deleteEdge(e1);
            client.deleteEdge(e2);
            client.deleteNode(alice);
            client.deleteNode(bob);
            client.deleteNode(charlie);
            System.out.println("Cleanup complete");
        }
    }
}
