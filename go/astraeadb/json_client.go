package astraeadb

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"fmt"
	"net"
	"sync"
	"time"

	"github.com/jimeharrisjr/astraeadb-go/internal/protocol"
)

// JSONClient communicates with AstraeaDB over the newline-delimited JSON/TCP
// protocol (default port 7687). It requires zero external dependencies beyond
// the Go standard library.
type JSONClient struct {
	cfg  *clientConfig
	mu   sync.Mutex
	conn *protocol.Conn
}

// NewJSONClient creates a new JSON/TCP client. Call Connect to establish
// the TCP connection before issuing requests.
func NewJSONClient(opts ...Option) *JSONClient {
	cfg := defaultConfig()
	for _, o := range opts {
		o(cfg)
	}
	return &JSONClient{cfg: cfg}
}

// Connect establishes a TCP connection to the server.
func (c *JSONClient) Connect(ctx context.Context) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.connectLocked(ctx)
}

func (c *JSONClient) connectLocked(ctx context.Context) error {
	dialer := &net.Dialer{
		Timeout:   c.cfg.dialTimeout,
		KeepAlive: 30 * time.Second,
	}

	var (
		rawConn net.Conn
		err     error
	)
	if c.cfg.tlsConfig != nil {
		rawConn, err = tls.DialWithDialer(dialer, "tcp", c.cfg.addr(), c.cfg.tlsConfig)
	} else {
		rawConn, err = dialer.DialContext(ctx, "tcp", c.cfg.addr())
	}
	if err != nil {
		return fmt.Errorf("astraeadb: dial %s: %w", c.cfg.addr(), err)
	}

	if tc, ok := rawConn.(*net.TCPConn); ok {
		tc.SetNoDelay(true)
	}

	c.conn = protocol.NewConn(rawConn)
	return nil
}

// Close closes the TCP connection.
func (c *JSONClient) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.closeLocked()
}

func (c *JSONClient) closeLocked() error {
	if c.conn != nil {
		err := c.conn.Close()
		c.conn = nil
		return err
	}
	return nil
}

// send sends a JSON request and returns the parsed response data.
// The caller must hold c.mu.
func (c *JSONClient) send(ctx context.Context, req map[string]any) (json.RawMessage, error) {
	if c.conn == nil {
		return nil, ErrNotConnected
	}

	if c.cfg.authToken != "" {
		req["auth_token"] = c.cfg.authToken
	}

	// Bridge context deadline to the connection.
	deadline, hasDeadline := ctx.Deadline()
	if !hasDeadline {
		deadline = time.Now().Add(c.cfg.timeout)
	}
	c.conn.SetDeadline(deadline)

	if err := c.conn.Send(req); err != nil {
		c.closeLocked()
		return nil, fmt.Errorf("astraeadb: send: %w", err)
	}

	var resp jsonResponse
	if err := c.conn.Receive(&resp); err != nil {
		c.closeLocked()
		return nil, fmt.Errorf("astraeadb: receive: %w", err)
	}

	if resp.Status == "error" {
		return nil, classifyError(resp.Message)
	}
	return resp.Data, nil
}

// do is a convenience wrapper that acquires the mutex and calls send.
func (c *JSONClient) do(ctx context.Context, req map[string]any) (json.RawMessage, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.send(ctx, req)
}

// unmarshal sends a request and unmarshals the response data into dst.
func (c *JSONClient) unmarshal(ctx context.Context, req map[string]any, dst any) error {
	data, err := c.do(ctx, req)
	if err != nil {
		return err
	}
	return json.Unmarshal(data, dst)
}

// ============================================================================
// Health
// ============================================================================

// Ping sends a health check to the server.
func (c *JSONClient) Ping(ctx context.Context) (*PingResponse, error) {
	var r PingResponse
	if err := c.unmarshal(ctx, map[string]any{"type": "Ping"}, &r); err != nil {
		return nil, err
	}
	return &r, nil
}

// ============================================================================
// Node CRUD
// ============================================================================

// CreateNode creates a new graph node and returns its ID.
func (c *JSONClient) CreateNode(ctx context.Context, labels []string, properties map[string]any, embedding []float32) (uint64, error) {
	req := map[string]any{
		"type":       "CreateNode",
		"labels":     labels,
		"properties": orEmpty(properties),
	}
	if embedding != nil {
		req["embedding"] = embedding
	}
	var r struct{ NodeID uint64 `json:"node_id"` }
	if err := c.unmarshal(ctx, req, &r); err != nil {
		return 0, err
	}
	return r.NodeID, nil
}

// GetNode retrieves a node by ID.
func (c *JSONClient) GetNode(ctx context.Context, id uint64) (*Node, error) {
	var n Node
	if err := c.unmarshal(ctx, map[string]any{"type": "GetNode", "id": id}, &n); err != nil {
		return nil, err
	}
	return &n, nil
}

// UpdateNode updates a node's properties with merge semantics.
func (c *JSONClient) UpdateNode(ctx context.Context, id uint64, properties map[string]any) error {
	_, err := c.do(ctx, map[string]any{
		"type":       "UpdateNode",
		"id":         id,
		"properties": orEmpty(properties),
	})
	return err
}

// DeleteNode deletes a node and all its connected edges.
func (c *JSONClient) DeleteNode(ctx context.Context, id uint64) error {
	_, err := c.do(ctx, map[string]any{"type": "DeleteNode", "id": id})
	return err
}

// ============================================================================
// Edge CRUD
// ============================================================================

// EdgeOption configures optional parameters for edge creation.
type EdgeOption func(map[string]any)

// WithWeight sets the edge weight (default 1.0).
func WithWeight(w float64) EdgeOption {
	return func(m map[string]any) { m["weight"] = w }
}

// WithProperties sets the edge properties.
func WithProperties(p map[string]any) EdgeOption {
	return func(m map[string]any) { m["properties"] = p }
}

// WithValidFrom sets the temporal validity start (epoch milliseconds).
func WithValidFrom(ts int64) EdgeOption {
	return func(m map[string]any) { m["valid_from"] = ts }
}

// WithValidTo sets the temporal validity end (epoch milliseconds).
func WithValidTo(ts int64) EdgeOption {
	return func(m map[string]any) { m["valid_to"] = ts }
}

// CreateEdge creates a directed edge between two nodes and returns its ID.
func (c *JSONClient) CreateEdge(ctx context.Context, source, target uint64, edgeType string, opts ...EdgeOption) (uint64, error) {
	req := map[string]any{
		"type":       "CreateEdge",
		"source":     source,
		"target":     target,
		"edge_type":  edgeType,
		"properties": map[string]any{},
		"weight":     1.0,
	}
	for _, o := range opts {
		o(req)
	}
	var r struct{ EdgeID uint64 `json:"edge_id"` }
	if err := c.unmarshal(ctx, req, &r); err != nil {
		return 0, err
	}
	return r.EdgeID, nil
}

// GetEdge retrieves an edge by ID.
func (c *JSONClient) GetEdge(ctx context.Context, id uint64) (*Edge, error) {
	var e Edge
	if err := c.unmarshal(ctx, map[string]any{"type": "GetEdge", "id": id}, &e); err != nil {
		return nil, err
	}
	return &e, nil
}

// UpdateEdge updates an edge's properties with merge semantics.
func (c *JSONClient) UpdateEdge(ctx context.Context, id uint64, properties map[string]any) error {
	_, err := c.do(ctx, map[string]any{
		"type":       "UpdateEdge",
		"id":         id,
		"properties": orEmpty(properties),
	})
	return err
}

// DeleteEdge deletes an edge.
func (c *JSONClient) DeleteEdge(ctx context.Context, id uint64) error {
	_, err := c.do(ctx, map[string]any{"type": "DeleteEdge", "id": id})
	return err
}

// ============================================================================
// Traversal
// ============================================================================

// NeighborOption configures optional parameters for neighbor queries.
type NeighborOption func(map[string]any)

// WithDirection sets the traversal direction: "outgoing", "incoming", or "both".
func WithDirection(d string) NeighborOption {
	return func(m map[string]any) { m["direction"] = d }
}

// WithEdgeType filters neighbors by edge type.
func WithEdgeType(t string) NeighborOption {
	return func(m map[string]any) { m["edge_type"] = t }
}

// Neighbors returns the immediate neighbors of a node.
func (c *JSONClient) Neighbors(ctx context.Context, id uint64, opts ...NeighborOption) ([]NeighborEntry, error) {
	req := map[string]any{
		"type":      "Neighbors",
		"id":        id,
		"direction": "outgoing",
	}
	for _, o := range opts {
		o(req)
	}
	var r struct{ Neighbors []NeighborEntry `json:"neighbors"` }
	if err := c.unmarshal(ctx, req, &r); err != nil {
		return nil, err
	}
	return r.Neighbors, nil
}

// BFS performs a breadth-first search from a starting node.
func (c *JSONClient) BFS(ctx context.Context, start uint64, maxDepth int) ([]BFSEntry, error) {
	var r struct{ Nodes []BFSEntry `json:"nodes"` }
	if err := c.unmarshal(ctx, map[string]any{
		"type":      "Bfs",
		"start":     start,
		"max_depth": maxDepth,
	}, &r); err != nil {
		return nil, err
	}
	return r.Nodes, nil
}

// ShortestPath finds the shortest path between two nodes.
func (c *JSONClient) ShortestPath(ctx context.Context, from, to uint64, weighted bool) (*PathResult, error) {
	var r PathResult
	if err := c.unmarshal(ctx, map[string]any{
		"type":     "ShortestPath",
		"from":     from,
		"to":       to,
		"weighted": weighted,
	}, &r); err != nil {
		return nil, err
	}
	return &r, nil
}

// ============================================================================
// Temporal Queries
// ============================================================================

// NeighborsAt returns neighbors at a specific point in time.
func (c *JSONClient) NeighborsAt(ctx context.Context, id uint64, direction string, timestamp int64, opts ...NeighborOption) ([]NeighborEntry, error) {
	req := map[string]any{
		"type":      "NeighborsAt",
		"id":        id,
		"direction": direction,
		"timestamp": timestamp,
	}
	for _, o := range opts {
		o(req)
	}
	var r struct{ Neighbors []NeighborEntry `json:"neighbors"` }
	if err := c.unmarshal(ctx, req, &r); err != nil {
		return nil, err
	}
	return r.Neighbors, nil
}

// BFSAt performs a BFS traversal at a specific point in time.
func (c *JSONClient) BFSAt(ctx context.Context, start uint64, maxDepth int, timestamp int64) ([]BFSEntry, error) {
	var r struct{ Nodes []BFSEntry `json:"nodes"` }
	if err := c.unmarshal(ctx, map[string]any{
		"type":      "BfsAt",
		"start":     start,
		"max_depth": maxDepth,
		"timestamp": timestamp,
	}, &r); err != nil {
		return nil, err
	}
	return r.Nodes, nil
}

// ShortestPathAt finds the shortest path at a specific point in time.
func (c *JSONClient) ShortestPathAt(ctx context.Context, from, to uint64, timestamp int64, weighted bool) (*PathResult, error) {
	var r PathResult
	if err := c.unmarshal(ctx, map[string]any{
		"type":      "ShortestPathAt",
		"from":      from,
		"to":        to,
		"timestamp": timestamp,
		"weighted":  weighted,
	}, &r); err != nil {
		return nil, err
	}
	return &r, nil
}

// ============================================================================
// Vector & Semantic Search
// ============================================================================

// VectorSearch performs k-nearest-neighbor search using embeddings.
func (c *JSONClient) VectorSearch(ctx context.Context, query []float32, k int) ([]SearchResult, error) {
	var r struct{ Results []SearchResult `json:"results"` }
	if err := c.unmarshal(ctx, map[string]any{
		"type":  "VectorSearch",
		"query": query,
		"k":     k,
	}, &r); err != nil {
		return nil, err
	}
	return r.Results, nil
}

// HybridOption configures optional parameters for hybrid search.
type HybridOption func(map[string]any)

// WithMaxHops sets the maximum BFS depth for hybrid search (default 3).
func WithMaxHops(n int) HybridOption {
	return func(m map[string]any) { m["max_hops"] = n }
}

// WithK sets the number of results to return (default 10).
func WithK(k int) HybridOption {
	return func(m map[string]any) { m["k"] = k }
}

// WithAlpha sets the graph/vector blend factor (default 0.5).
func WithAlpha(a float32) HybridOption {
	return func(m map[string]any) { m["alpha"] = a }
}

// HybridSearch combines graph proximity with vector similarity.
func (c *JSONClient) HybridSearch(ctx context.Context, anchor uint64, query []float32, opts ...HybridOption) ([]SearchResult, error) {
	req := map[string]any{
		"type":     "HybridSearch",
		"anchor":   anchor,
		"query":    query,
		"max_hops": 3,
		"k":        10,
		"alpha":    0.5,
	}
	for _, o := range opts {
		o(req)
	}
	var r struct{ Results []SearchResult `json:"results"` }
	if err := c.unmarshal(ctx, req, &r); err != nil {
		return nil, err
	}
	return r.Results, nil
}

// SemanticOption configures optional parameters for semantic operations.
type SemanticOption func(map[string]any)

// WithSemanticDirection sets the direction for semantic neighbor search.
func WithSemanticDirection(d string) SemanticOption {
	return func(m map[string]any) { m["direction"] = d }
}

// WithSemanticK sets the number of results for semantic search.
func WithSemanticK(k int) SemanticOption {
	return func(m map[string]any) { m["k"] = k }
}

// SemanticNeighbors ranks neighbors by embedding similarity to a concept.
func (c *JSONClient) SemanticNeighbors(ctx context.Context, id uint64, concept []float32, opts ...SemanticOption) ([]SearchResult, error) {
	req := map[string]any{
		"type":      "SemanticNeighbors",
		"id":        id,
		"concept":   concept,
		"direction": "outgoing",
		"k":         10,
	}
	for _, o := range opts {
		o(req)
	}
	var r struct{ Results []SearchResult `json:"results"` }
	if err := c.unmarshal(ctx, req, &r); err != nil {
		return nil, err
	}
	return r.Results, nil
}

// SemanticWalk performs a greedy multi-hop walk toward a concept embedding.
func (c *JSONClient) SemanticWalk(ctx context.Context, start uint64, concept []float32, maxHops int) ([]WalkStep, error) {
	var r struct{ Path []WalkStep `json:"path"` }
	if err := c.unmarshal(ctx, map[string]any{
		"type":     "SemanticWalk",
		"start":    start,
		"concept":  concept,
		"max_hops": maxHops,
	}, &r); err != nil {
		return nil, err
	}
	return r.Path, nil
}

// ============================================================================
// GQL Query
// ============================================================================

// Query executes a GQL query string.
func (c *JSONClient) Query(ctx context.Context, gql string) (*QueryResult, error) {
	var r QueryResult
	if err := c.unmarshal(ctx, map[string]any{
		"type": "Query",
		"gql":  gql,
	}, &r); err != nil {
		return nil, err
	}
	return &r, nil
}

// ============================================================================
// GraphRAG
// ============================================================================

// SubgraphOption configures optional parameters for subgraph extraction.
type SubgraphOption func(map[string]any)

// WithHops sets the BFS depth for subgraph extraction (default 3).
func WithHops(n int) SubgraphOption {
	return func(m map[string]any) { m["hops"] = n }
}

// WithMaxNodes sets the maximum nodes in the subgraph (default 50).
func WithMaxNodes(n int) SubgraphOption {
	return func(m map[string]any) { m["max_nodes"] = n }
}

// WithFormat sets the text format: "structured", "prose", "triples", or "json".
func WithFormat(f string) SubgraphOption {
	return func(m map[string]any) { m["format"] = f }
}

// ExtractSubgraph extracts and linearizes a subgraph around a center node.
func (c *JSONClient) ExtractSubgraph(ctx context.Context, center uint64, opts ...SubgraphOption) (*SubgraphResult, error) {
	req := map[string]any{
		"type":      "ExtractSubgraph",
		"center":    center,
		"hops":      3,
		"max_nodes": 50,
		"format":    "structured",
	}
	for _, o := range opts {
		o(req)
	}
	var r SubgraphResult
	if err := c.unmarshal(ctx, req, &r); err != nil {
		return nil, err
	}
	return &r, nil
}

// RAGOption configures optional parameters for GraphRAG queries.
type RAGOption func(map[string]any)

// WithAnchor sets the explicit anchor node for GraphRAG.
func WithAnchor(id uint64) RAGOption {
	return func(m map[string]any) { m["anchor"] = id }
}

// WithQuestionEmbedding sets the question embedding for semantic anchor search.
func WithQuestionEmbedding(e []float32) RAGOption {
	return func(m map[string]any) { m["question_embedding"] = e }
}

// WithRAGHops sets the BFS depth for GraphRAG context (default 3).
func WithRAGHops(n int) RAGOption {
	return func(m map[string]any) { m["hops"] = n }
}

// WithRAGMaxNodes sets the maximum nodes in GraphRAG context (default 50).
func WithRAGMaxNodes(n int) RAGOption {
	return func(m map[string]any) { m["max_nodes"] = n }
}

// WithRAGFormat sets the text format for GraphRAG context.
func WithRAGFormat(f string) RAGOption {
	return func(m map[string]any) { m["format"] = f }
}

// GraphRAG executes a Retrieval-Augmented Generation query.
func (c *JSONClient) GraphRAG(ctx context.Context, question string, opts ...RAGOption) (*RAGResult, error) {
	req := map[string]any{
		"type":      "GraphRag",
		"question":  question,
		"hops":      3,
		"max_nodes": 50,
		"format":    "structured",
	}
	for _, o := range opts {
		o(req)
	}
	var r RAGResult
	if err := c.unmarshal(ctx, req, &r); err != nil {
		return nil, err
	}
	return &r, nil
}

// ============================================================================
// Batch Operations
// ============================================================================

// CreateNodes creates multiple nodes and returns their IDs.
func (c *JSONClient) CreateNodes(ctx context.Context, nodes []NodeInput) ([]uint64, error) {
	ids := make([]uint64, 0, len(nodes))
	for _, n := range nodes {
		id, err := c.CreateNode(ctx, n.Labels, n.Properties, n.Embedding)
		if err != nil {
			return ids, err
		}
		ids = append(ids, id)
	}
	return ids, nil
}

// CreateEdges creates multiple edges and returns their IDs.
func (c *JSONClient) CreateEdges(ctx context.Context, edges []EdgeInput) ([]uint64, error) {
	ids := make([]uint64, 0, len(edges))
	for _, e := range edges {
		var opts []EdgeOption
		if e.Properties != nil {
			opts = append(opts, WithProperties(e.Properties))
		}
		if e.Weight != 0 {
			opts = append(opts, WithWeight(e.Weight))
		}
		if e.ValidFrom != nil {
			opts = append(opts, WithValidFrom(*e.ValidFrom))
		}
		if e.ValidTo != nil {
			opts = append(opts, WithValidTo(*e.ValidTo))
		}
		id, err := c.CreateEdge(ctx, e.Source, e.Target, e.EdgeType, opts...)
		if err != nil {
			return ids, err
		}
		ids = append(ids, id)
	}
	return ids, nil
}

// DeleteNodes deletes multiple nodes, skipping failures. Returns the count deleted.
func (c *JSONClient) DeleteNodes(ctx context.Context, ids []uint64) int {
	count := 0
	for _, id := range ids {
		if err := c.DeleteNode(ctx, id); err == nil {
			count++
		}
	}
	return count
}

// DeleteEdges deletes multiple edges, skipping failures. Returns the count deleted.
func (c *JSONClient) DeleteEdges(ctx context.Context, ids []uint64) int {
	count := 0
	for _, id := range ids {
		if err := c.DeleteEdge(ctx, id); err == nil {
			count++
		}
	}
	return count
}

// ============================================================================
// Helpers
// ============================================================================

func orEmpty(m map[string]any) map[string]any {
	if m == nil {
		return map[string]any{}
	}
	return m
}
