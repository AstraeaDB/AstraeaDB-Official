package com.astraeadb.examples;

import com.astraeadb.json.JsonClient;
import com.astraeadb.json.JsonClientBuilder;
import com.astraeadb.model.*;
import com.astraeadb.options.*;

import java.util.List;
import java.util.Map;

/**
 * Cybersecurity threat investigation demo using temporal edges and graph traversal.
 *
 * <p>Models a multi-stage attack (recon, exploitation, lateral movement,
 * exfiltration) as a temporal graph, then demonstrates time-travel queries,
 * BFS from the attacker node, shortest-path analysis, and GraphRAG-based
 * investigation.
 */
public class CybersecurityExample {
    public static void main(String[] args) throws Exception {
        try (JsonClient client = new JsonClientBuilder()
                .host("127.0.0.1").port(7687).build()) {

            client.connect();
            System.out.println("=== AstraeaDB Cybersecurity Demo ===");

            // Build threat graph
            long server = client.createNode(List.of("Host"), Map.of("hostname", "web-server-01", "ip", "10.0.1.5"));
            long attacker = client.createNode(List.of("Host"), Map.of("hostname", "unknown", "ip", "203.0.113.42"));
            long db = client.createNode(List.of("Host"), Map.of("hostname", "db-server-01", "ip", "10.0.1.10"));
            long exfil = client.createNode(List.of("Host"), Map.of("hostname", "drop-server", "ip", "198.51.100.7"));

            // Temporal edges: attack progression over time
            long t1 = 1700000000L; // Initial recon
            long t2 = 1700003600L; // Exploitation (+1h)
            long t3 = 1700007200L; // Lateral movement (+2h)
            long t4 = 1700010800L; // Exfiltration (+3h)

            client.createEdge(attacker, server, "SCANNED",
                EdgeOptions.builder().validFrom(t1).validTo(t2).properties(Map.of("port", 443)).build());
            client.createEdge(attacker, server, "EXPLOITED",
                EdgeOptions.builder().validFrom(t2).validTo(t3).properties(Map.of("cve", "CVE-2024-1234")).build());
            client.createEdge(server, db, "LATERAL_MOVE",
                EdgeOptions.builder().validFrom(t3).validTo(t4).properties(Map.of("method", "ssh")).build());
            client.createEdge(db, exfil, "EXFILTRATED",
                EdgeOptions.builder().validFrom(t4).properties(Map.of("bytes", 1048576)).build());

            System.out.println("Threat graph built: " + server + ", " + attacker + ", " + db + ", " + exfil);

            // Time-travel query: what was happening at t2 (exploitation phase)?
            List<NeighborEntry> atT2 = client.neighborsAt(server, "incoming", t2);
            System.out.println("\nConnections to web-server at exploitation time: " + atT2.size());

            // BFS from attacker node
            List<BfsEntry> bfs = client.bfs(attacker, 5);
            System.out.println("BFS from attacker: " + bfs.size() + " nodes reachable");

            // Shortest path from attacker to exfil server
            PathResult path = client.shortestPath(attacker, exfil);
            System.out.println("Attack path: " + path.path() + " (length=" + path.length() + ")");

            // GraphRAG: investigate the attack
            RagResult rag = client.graphRag("Describe the attack progression from the external host",
                RagOptions.builder().anchor(attacker).hops(4).maxNodes(100).build());
            System.out.println("\nInvestigation context:");
            System.out.println("  Nodes analyzed: " + rag.nodesInContext());
            System.out.println("  Edges analyzed: " + rag.edgesInContext());

            // Cleanup
            client.deleteNode(server);
            client.deleteNode(attacker);
            client.deleteNode(db);
            client.deleteNode(exfil);
            System.out.println("\n=== Demo Complete ===");
        }
    }
}
