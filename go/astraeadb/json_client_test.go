package astraeadb

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"testing"
	"time"

	"github.com/AstraeaDB/R-AstraeaDB/internal/protocol"
)

// mockServer creates a connected pair of JSONClient + server-side protocol.Conn
// using net.Pipe so that tests run without a real AstraeaDB server.
func mockServer(t *testing.T) (*JSONClient, *protocol.Conn) {
	t.Helper()
	client, server := net.Pipe()
	jc := &JSONClient{
		cfg:  defaultConfig(),
		conn: protocol.NewConn(client),
	}
	return jc, protocol.NewConn(server)
}

// respond reads the request from the server side and writes back a canned
// JSON/TCP response. It returns the request map for assertions.
func respond(t *testing.T, srv *protocol.Conn, data any) map[string]any {
	t.Helper()
	var req map[string]any
	if err := srv.Receive(&req); err != nil {
		t.Fatalf("receive request: %v", err)
	}
	resp := map[string]any{"status": "ok"}
	if data != nil {
		raw, _ := json.Marshal(data)
		resp["data"] = json.RawMessage(raw)
	}
	if err := srv.Send(resp); err != nil {
		t.Fatalf("send response: %v", err)
	}
	return req
}

// respondError sends an error response.
func respondError(t *testing.T, srv *protocol.Conn, msg string) {
	t.Helper()
	var req map[string]any
	if err := srv.Receive(&req); err != nil {
		t.Fatalf("receive request: %v", err)
	}
	if err := srv.Send(map[string]any{"status": "error", "message": msg}); err != nil {
		t.Fatalf("send error response: %v", err)
	}
}

func TestPing(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{"pong": true, "version": "0.8.0"})
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	resp, err := jc.Ping(ctx)
	if err != nil {
		t.Fatalf("Ping: %v", err)
	}
	if !resp.Pong {
		t.Error("expected pong=true")
	}
	if resp.Version != "0.8.0" {
		t.Errorf("version = %q, want %q", resp.Version, "0.8.0")
	}
}

func TestCreateNode(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		req := respond(t, srv, map[string]any{"node_id": 42})
		if req["type"] != "CreateNode" {
			t.Errorf("type = %v, want CreateNode", req["type"])
		}
	}()

	ctx := context.Background()
	id, err := jc.CreateNode(ctx, []string{"Person"}, map[string]any{"name": "Alice"}, nil)
	if err != nil {
		t.Fatalf("CreateNode: %v", err)
	}
	if id != 42 {
		t.Errorf("id = %d, want 42", id)
	}
}

func TestGetNode(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{
			"id":            1,
			"labels":        []string{"Person"},
			"properties":    map[string]any{"name": "Bob"},
			"has_embedding": false,
		})
	}()

	ctx := context.Background()
	n, err := jc.GetNode(ctx, 1)
	if err != nil {
		t.Fatalf("GetNode: %v", err)
	}
	if n.ID != 1 {
		t.Errorf("ID = %d, want 1", n.ID)
	}
	if len(n.Labels) != 1 || n.Labels[0] != "Person" {
		t.Errorf("Labels = %v, want [Person]", n.Labels)
	}
}

func TestUpdateNode(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		req := respond(t, srv, nil)
		if req["type"] != "UpdateNode" {
			t.Errorf("type = %v, want UpdateNode", req["type"])
		}
	}()

	err := jc.UpdateNode(context.Background(), 1, map[string]any{"name": "Charlie"})
	if err != nil {
		t.Fatalf("UpdateNode: %v", err)
	}
}

func TestDeleteNode(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		req := respond(t, srv, nil)
		if req["type"] != "DeleteNode" {
			t.Errorf("type = %v, want DeleteNode", req["type"])
		}
	}()

	err := jc.DeleteNode(context.Background(), 1)
	if err != nil {
		t.Fatalf("DeleteNode: %v", err)
	}
}

func TestCreateEdge(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		req := respond(t, srv, map[string]any{"edge_id": 100})
		if req["type"] != "CreateEdge" {
			t.Errorf("type = %v, want CreateEdge", req["type"])
		}
		if req["edge_type"] != "KNOWS" {
			t.Errorf("edge_type = %v, want KNOWS", req["edge_type"])
		}
	}()

	id, err := jc.CreateEdge(context.Background(), 1, 2, "KNOWS", WithWeight(0.9))
	if err != nil {
		t.Fatalf("CreateEdge: %v", err)
	}
	if id != 100 {
		t.Errorf("id = %d, want 100", id)
	}
}

func TestGetEdge(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{
			"id":         100,
			"source":     1,
			"target":     2,
			"edge_type":  "KNOWS",
			"properties": map[string]any{},
			"weight":     1.0,
		})
	}()

	e, err := jc.GetEdge(context.Background(), 100)
	if err != nil {
		t.Fatalf("GetEdge: %v", err)
	}
	if e.Source != 1 || e.Target != 2 {
		t.Errorf("source=%d target=%d, want 1,2", e.Source, e.Target)
	}
}

func TestNeighbors(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		req := respond(t, srv, map[string]any{
			"neighbors": []map[string]any{
				{"edge_id": 100, "node_id": 2},
				{"edge_id": 101, "node_id": 3},
			},
		})
		if req["direction"] != "both" {
			t.Errorf("direction = %v, want both", req["direction"])
		}
	}()

	entries, err := jc.Neighbors(context.Background(), 1, WithDirection("both"))
	if err != nil {
		t.Fatalf("Neighbors: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("len = %d, want 2", len(entries))
	}
	if entries[0].NodeID != 2 {
		t.Errorf("entries[0].NodeID = %d, want 2", entries[0].NodeID)
	}
}

func TestBFS(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{
			"nodes": []map[string]any{
				{"node_id": 1, "depth": 0},
				{"node_id": 2, "depth": 1},
			},
		})
	}()

	entries, err := jc.BFS(context.Background(), 1, 3)
	if err != nil {
		t.Fatalf("BFS: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("len = %d, want 2", len(entries))
	}
}

func TestShortestPath(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{
			"found":  true,
			"path":   []uint64{1, 3, 5},
			"length": 2,
			"cost":   1.5,
		})
	}()

	res, err := jc.ShortestPath(context.Background(), 1, 5, true)
	if err != nil {
		t.Fatalf("ShortestPath: %v", err)
	}
	if !res.Found {
		t.Error("expected found=true")
	}
	if len(res.Path) != 3 {
		t.Errorf("path length = %d, want 3", len(res.Path))
	}
}

func TestVectorSearch(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{
			"results": []map[string]any{
				{"node_id": 10, "score": 0.95},
				{"node_id": 20, "score": 0.88},
			},
		})
	}()

	results, err := jc.VectorSearch(context.Background(), []float32{0.1, 0.2, 0.3}, 5)
	if err != nil {
		t.Fatalf("VectorSearch: %v", err)
	}
	if len(results) != 2 {
		t.Fatalf("len = %d, want 2", len(results))
	}
	if results[0].Score != 0.95 {
		t.Errorf("score = %f, want 0.95", results[0].Score)
	}
}

func TestHybridSearch(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		req := respond(t, srv, map[string]any{
			"results": []map[string]any{
				{"node_id": 5, "score": 0.92},
			},
		})
		if req["type"] != "HybridSearch" {
			t.Errorf("type = %v, want HybridSearch", req["type"])
		}
	}()

	results, err := jc.HybridSearch(context.Background(), 1, []float32{0.1, 0.2}, WithK(5))
	if err != nil {
		t.Fatalf("HybridSearch: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("len = %d, want 1", len(results))
	}
}

func TestQuery(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{
			"columns": []string{"name"},
			"rows":    [][]any{{"Alice"}, {"Bob"}},
			"stats":   map[string]any{},
		})
	}()

	res, err := jc.Query(context.Background(), "MATCH (n:Person) RETURN n.name")
	if err != nil {
		t.Fatalf("Query: %v", err)
	}
	if len(res.Columns) != 1 || res.Columns[0] != "name" {
		t.Errorf("columns = %v, want [name]", res.Columns)
	}
	if len(res.Rows) != 2 {
		t.Errorf("rows = %d, want 2", len(res.Rows))
	}
}

func TestExtractSubgraph(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		req := respond(t, srv, map[string]any{
			"text":             "Node 1 -> Node 2",
			"nodes_count":      2,
			"edges_count":      1,
			"estimated_tokens": 10,
		})
		if req["type"] != "ExtractSubgraph" {
			t.Errorf("type = %v, want ExtractSubgraph", req["type"])
		}
	}()

	res, err := jc.ExtractSubgraph(context.Background(), 1, WithHops(2), WithMaxNodes(10))
	if err != nil {
		t.Fatalf("ExtractSubgraph: %v", err)
	}
	if res.NodeCount != 2 {
		t.Errorf("NodeCount = %d, want 2", res.NodeCount)
	}
}

func TestGraphRAG(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{
			"anchor_node_id":   1,
			"context":          "some context",
			"question":         "Who is Alice?",
			"nodes_in_context": 5,
			"edges_in_context": 3,
			"estimated_tokens": 50,
			"note":             "",
		})
	}()

	res, err := jc.GraphRAG(context.Background(), "Who is Alice?", WithAnchor(1))
	if err != nil {
		t.Fatalf("GraphRAG: %v", err)
	}
	if res.AnchorNodeID != 1 {
		t.Errorf("AnchorNodeID = %d, want 1", res.AnchorNodeID)
	}
}

func TestTemporalNeighborsAt(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		req := respond(t, srv, map[string]any{
			"neighbors": []map[string]any{
				{"edge_id": 200, "node_id": 3},
			},
		})
		if req["type"] != "NeighborsAt" {
			t.Errorf("type = %v, want NeighborsAt", req["type"])
		}
		// Verify timestamp is passed
		ts, _ := req["timestamp"].(float64)
		if int64(ts) != 1704067200 {
			t.Errorf("timestamp = %v, want 1704067200", req["timestamp"])
		}
	}()

	entries, err := jc.NeighborsAt(context.Background(), 1, "outgoing", 1704067200)
	if err != nil {
		t.Fatalf("NeighborsAt: %v", err)
	}
	if len(entries) != 1 {
		t.Fatalf("len = %d, want 1", len(entries))
	}
}

func TestServerError(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respondError(t, srv, "Node not found")
	}()

	_, err := jc.GetNode(context.Background(), 999)
	if !errors.Is(err, ErrNodeNotFound) {
		t.Errorf("err = %v, want ErrNodeNotFound", err)
	}
}

func TestNotConnected(t *testing.T) {
	jc := &JSONClient{cfg: defaultConfig()}

	_, err := jc.Ping(context.Background())
	if !errors.Is(err, ErrNotConnected) {
		t.Errorf("err = %v, want ErrNotConnected", err)
	}
}

func TestAuthTokenInjected(t *testing.T) {
	jc, srv := mockServer(t)
	jc.cfg.authToken = "my-secret-token"
	defer jc.Close()

	go func() {
		req := respond(t, srv, map[string]any{"pong": true, "version": "0.8.0"})
		if req["auth_token"] != "my-secret-token" {
			t.Errorf("auth_token = %v, want my-secret-token", req["auth_token"])
		}
	}()

	_, err := jc.Ping(context.Background())
	if err != nil {
		t.Fatalf("Ping: %v", err)
	}
}

func TestSemanticWalk(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{
			"path": []map[string]any{
				{"node_id": 1, "distance": 0.0},
				{"node_id": 5, "distance": 0.3},
				{"node_id": 8, "distance": 0.6},
			},
		})
	}()

	steps, err := jc.SemanticWalk(context.Background(), 1, []float32{0.5, 0.5}, 3)
	if err != nil {
		t.Fatalf("SemanticWalk: %v", err)
	}
	if len(steps) != 3 {
		t.Fatalf("len = %d, want 3", len(steps))
	}
	if steps[2].NodeID != 8 {
		t.Errorf("steps[2].NodeID = %d, want 8", steps[2].NodeID)
	}
}

func TestBatchCreateNodes(t *testing.T) {
	jc, srv := mockServer(t)
	defer jc.Close()

	go func() {
		respond(t, srv, map[string]any{"node_id": 1})
		respond(t, srv, map[string]any{"node_id": 2})
	}()

	nodes := []NodeInput{
		{Labels: []string{"A"}, Properties: map[string]any{"x": 1}},
		{Labels: []string{"B"}, Properties: map[string]any{"x": 2}},
	}
	ids, err := jc.CreateNodes(context.Background(), nodes)
	if err != nil {
		t.Fatalf("CreateNodes: %v", err)
	}
	if len(ids) != 2 {
		t.Fatalf("len = %d, want 2", len(ids))
	}
	if ids[0] != 1 || ids[1] != 2 {
		t.Errorf("ids = %v, want [1 2]", ids)
	}
}
