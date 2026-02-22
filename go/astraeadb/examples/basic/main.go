// Command basic demonstrates core AstraeaDB operations using the Go client.
//
// Prerequisites:
//
//	AstraeaDB server running on localhost with default ports (7687/7688/7689).
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

	"github.com/AstraeaDB/AstraeaDB-Official"
)

func main() {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	// ── Connect ──────────────────────────────────────────────────────────
	client := astraeadb.NewClient(
		astraeadb.WithAddress("127.0.0.1", 7687),
	)
	if err := client.Connect(ctx); err != nil {
		log.Fatalf("Connect: %v", err)
	}
	defer client.Close()

	// ── Health check ─────────────────────────────────────────────────────
	ping, err := client.Ping(ctx)
	if err != nil {
		log.Fatalf("Ping: %v", err)
	}
	fmt.Printf("Connected to AstraeaDB %s\n", ping.Version)

	// ── Create nodes ─────────────────────────────────────────────────────
	alice, err := client.CreateNode(ctx, []string{"Person"}, map[string]any{
		"name": "Alice",
		"age":  30,
	}, nil)
	if err != nil {
		log.Fatalf("CreateNode(Alice): %v", err)
	}
	fmt.Printf("Created Alice: node %d\n", alice)

	bob, err := client.CreateNode(ctx, []string{"Person"}, map[string]any{
		"name": "Bob",
		"age":  25,
	}, nil)
	if err != nil {
		log.Fatalf("CreateNode(Bob): %v", err)
	}
	fmt.Printf("Created Bob:   node %d\n", bob)

	// ── Create edge ──────────────────────────────────────────────────────
	edgeID, err := client.CreateEdge(ctx, alice, bob, "KNOWS",
		astraeadb.WithWeight(0.9),
		astraeadb.WithProperties(map[string]any{"since": 2020}),
	)
	if err != nil {
		log.Fatalf("CreateEdge: %v", err)
	}
	fmt.Printf("Created edge %d: Alice -> KNOWS -> Bob\n", edgeID)

	// ── Read back ────────────────────────────────────────────────────────
	node, err := client.GetNode(ctx, alice)
	if err != nil {
		log.Fatalf("GetNode: %v", err)
	}
	fmt.Printf("Got node: %s (labels=%v)\n", node.Properties["name"], node.Labels)

	// ── Traverse ─────────────────────────────────────────────────────────
	neighbors, err := client.Neighbors(ctx, alice, astraeadb.WithDirection("outgoing"))
	if err != nil {
		log.Fatalf("Neighbors: %v", err)
	}
	fmt.Printf("Alice has %d outgoing neighbor(s)\n", len(neighbors))

	// ── BFS ──────────────────────────────────────────────────────────────
	bfs, err := client.BFS(ctx, alice, 3)
	if err != nil {
		log.Fatalf("BFS: %v", err)
	}
	fmt.Printf("BFS from Alice found %d node(s)\n", len(bfs))

	// ── GQL query ────────────────────────────────────────────────────────
	result, err := client.Query(ctx, "MATCH (n:Person) RETURN n.name, n.age")
	if err != nil {
		log.Fatalf("Query: %v", err)
	}
	fmt.Printf("Query returned %d row(s), columns: %v\n", len(result.Rows), result.Columns)

	// ── Cleanup ──────────────────────────────────────────────────────────
	if err := client.DeleteEdge(ctx, edgeID); err != nil {
		log.Fatalf("DeleteEdge: %v", err)
	}
	if err := client.DeleteNode(ctx, alice); err != nil {
		log.Fatalf("DeleteNode(alice): %v", err)
	}
	if err := client.DeleteNode(ctx, bob); err != nil {
		log.Fatalf("DeleteNode(bob): %v", err)
	}

	fmt.Println("Done! All resources cleaned up.")
}
