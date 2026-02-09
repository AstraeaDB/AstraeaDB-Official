// Package astraeadb provides a Go client library for AstraeaDB, a cloud-native,
// AI-first graph database written in Rust.
//
// AstraeaDB unifies property graphs, vector search, and graph neural networks
// in a single system. This client supports three transport protocols:
//
//   - JSON/TCP (port 7687): Newline-delimited JSON over TCP. Zero external dependencies.
//   - gRPC (port 7688): Protocol Buffers over gRPC for type-safe, high-performance access.
//   - Arrow Flight (port 7689): Apache Arrow Flight for zero-copy bulk data exchange.
//
// # Quick Start
//
// The simplest way to get started is with the unified client:
//
//	client := astraeadb.NewClient(
//	    astraeadb.WithAddress("127.0.0.1", 7687),
//	)
//	if err := client.Connect(context.Background()); err != nil {
//	    log.Fatal(err)
//	}
//	defer client.Close()
//
//	// Create a node
//	id, err := client.CreateNode(ctx, []string{"Person"}, map[string]any{"name": "Alice"}, nil)
//
//	// Query with GQL
//	result, err := client.Query(ctx, "MATCH (n:Person) RETURN n.name")
//
// # Client Types
//
//   - [NewJSONClient]: JSON/TCP transport (zero external deps)
//   - [NewGRPCClient]: gRPC transport with protobuf
//   - [NewClient]: Unified client that auto-selects the best transport
package astraeadb
