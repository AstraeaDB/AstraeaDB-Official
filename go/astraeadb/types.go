package astraeadb

import "encoding/json"

// --- Core domain types ---

// Node represents a graph node returned by the server.
type Node struct {
	ID           uint64         `json:"id"`
	Labels       []string       `json:"labels"`
	Properties   map[string]any `json:"properties"`
	HasEmbedding bool           `json:"has_embedding"`
}

// Edge represents a graph edge returned by the server.
type Edge struct {
	ID         uint64         `json:"id"`
	Source     uint64         `json:"source"`
	Target     uint64         `json:"target"`
	EdgeType   string         `json:"edge_type"`
	Properties map[string]any `json:"properties"`
	Weight     float64        `json:"weight"`
	ValidFrom  *int64         `json:"valid_from,omitempty"`
	ValidTo    *int64         `json:"valid_to,omitempty"`
}

// NeighborEntry represents a single neighbor in a traversal result.
type NeighborEntry struct {
	EdgeID uint64 `json:"edge_id"`
	NodeID uint64 `json:"node_id"`
}

// BFSEntry represents a single node discovered during BFS traversal.
type BFSEntry struct {
	NodeID uint64 `json:"node_id"`
	Depth  int    `json:"depth"`
}

// PathResult contains the result of a shortest-path query.
type PathResult struct {
	Found  bool     `json:"found"`
	Path   []uint64 `json:"path"`
	Length int      `json:"length"`
	Cost   *float64 `json:"cost,omitempty"`
}

// SearchResult represents a single result from vector, hybrid, or semantic search.
type SearchResult struct {
	NodeID   uint64  `json:"node_id"`
	Distance float64 `json:"distance,omitempty"`
	Score    float64 `json:"score,omitempty"`
}

// WalkStep represents a single step in a semantic walk.
type WalkStep struct {
	NodeID   uint64  `json:"node_id"`
	Distance float64 `json:"distance"`
}

// QueryResult contains the result of a GQL query execution.
type QueryResult struct {
	Columns []string        `json:"columns"`
	Rows    [][]interface{} `json:"rows"`
	Stats   QueryStats      `json:"stats"`
}

// QueryStats contains mutation statistics from query execution.
type QueryStats struct {
	NodesCreated uint64 `json:"nodes_created"`
	EdgesCreated uint64 `json:"edges_created"`
	NodesDeleted uint64 `json:"nodes_deleted"`
	EdgesDeleted uint64 `json:"edges_deleted"`
}

// SubgraphResult contains the result of a subgraph extraction.
type SubgraphResult struct {
	Text            string `json:"text"`
	NodeCount       int    `json:"nodes_count"`
	EdgeCount       int    `json:"edges_count"`
	EstimatedTokens int    `json:"estimated_tokens"`
}

// RAGResult contains the result of a GraphRAG query.
type RAGResult struct {
	AnchorNodeID   uint64 `json:"anchor_node_id"`
	Context        string `json:"context"`
	Question       string `json:"question"`
	NodesInContext int    `json:"nodes_in_context"`
	EdgesInContext int    `json:"edges_in_context"`
	EstimatedTokens int   `json:"estimated_tokens"`
	Note           string `json:"note"`
}

// PingResponse contains the server health check response.
type PingResponse struct {
	Pong    bool   `json:"pong"`
	Version string `json:"version"`
}

// --- Batch input types ---

// NodeInput describes a node to create in a batch operation.
type NodeInput struct {
	Labels     []string       `json:"labels"`
	Properties map[string]any `json:"properties,omitempty"`
	Embedding  []float32      `json:"embedding,omitempty"`
}

// EdgeInput describes an edge to create in a batch operation.
type EdgeInput struct {
	Source     uint64         `json:"source"`
	Target     uint64         `json:"target"`
	EdgeType   string         `json:"edge_type"`
	Properties map[string]any `json:"properties,omitempty"`
	Weight     float64        `json:"weight,omitempty"`
	ValidFrom  *int64         `json:"valid_from,omitempty"`
	ValidTo    *int64         `json:"valid_to,omitempty"`
}

// --- Wire protocol types (internal) ---

// jsonResponse is the wire format for JSON/TCP server responses.
type jsonResponse struct {
	Status  string          `json:"status"`
	Data    json.RawMessage `json:"data,omitempty"`
	Message string          `json:"message,omitempty"`
}
