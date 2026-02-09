package astraeadb

import (
	"context"
	"log"
)

// Client is the unified AstraeaDB client that auto-selects the best transport
// for each operation. It uses gRPC for operations the proto supports, and falls
// back to JSON/TCP for temporal queries, semantic search, and GraphRAG.
type Client struct {
	cfg   *clientConfig
	json  *JSONClient
	grpc  *GRPCClient
	arrow *ArrowClient

	useGRPC  bool
	useArrow bool
}

// NewClient creates a unified client that auto-selects the optimal transport.
func NewClient(opts ...Option) *Client {
	cfg := defaultConfig()
	for _, o := range opts {
		o(cfg)
	}
	return &Client{
		cfg:   cfg,
		json:  &JSONClient{cfg: cfg},
		grpc:  &GRPCClient{cfg: cfg},
		arrow: &ArrowClient{cfg: cfg},
	}
}

// Connect establishes connections to all available transports.
// JSON/TCP is required; gRPC and Arrow Flight are optional.
func (c *Client) Connect(ctx context.Context) error {
	// JSON/TCP is always required.
	if err := c.json.Connect(ctx); err != nil {
		return err
	}

	// gRPC is optional; log but don't fail.
	if err := c.grpc.Connect(ctx); err != nil {
		log.Printf("astraeadb: gRPC unavailable (%s), using JSON/TCP", err)
		c.useGRPC = false
	} else {
		c.useGRPC = true
	}

	// Arrow Flight is optional.
	if err := c.arrow.Connect(ctx); err != nil {
		c.useArrow = false
	} else {
		c.useArrow = true
	}

	return nil
}

// Close closes all transport connections.
func (c *Client) Close() error {
	var firstErr error
	if err := c.json.Close(); err != nil && firstErr == nil {
		firstErr = err
	}
	if c.useGRPC {
		if err := c.grpc.Close(); err != nil && firstErr == nil {
			firstErr = err
		}
	}
	if c.useArrow {
		c.arrow.Close()
	}
	return firstErr
}

// IsGRPCAvailable returns whether the gRPC transport is connected.
func (c *Client) IsGRPCAvailable() bool { return c.useGRPC }

// IsArrowAvailable returns whether the Arrow Flight transport is connected.
func (c *Client) IsArrowAvailable() bool { return c.useArrow }

// ============================================================================
// Health
// ============================================================================

func (c *Client) Ping(ctx context.Context) (*PingResponse, error) {
	if c.useGRPC {
		return c.grpc.Ping(ctx)
	}
	return c.json.Ping(ctx)
}

// ============================================================================
// Node CRUD - prefer gRPC when available
// ============================================================================

func (c *Client) CreateNode(ctx context.Context, labels []string, properties map[string]any, embedding []float32) (uint64, error) {
	if c.useGRPC {
		return c.grpc.CreateNode(ctx, labels, properties, embedding)
	}
	return c.json.CreateNode(ctx, labels, properties, embedding)
}

func (c *Client) GetNode(ctx context.Context, id uint64) (*Node, error) {
	if c.useGRPC {
		return c.grpc.GetNode(ctx, id)
	}
	return c.json.GetNode(ctx, id)
}

func (c *Client) UpdateNode(ctx context.Context, id uint64, properties map[string]any) error {
	if c.useGRPC {
		return c.grpc.UpdateNode(ctx, id, properties)
	}
	return c.json.UpdateNode(ctx, id, properties)
}

func (c *Client) DeleteNode(ctx context.Context, id uint64) error {
	if c.useGRPC {
		return c.grpc.DeleteNode(ctx, id)
	}
	return c.json.DeleteNode(ctx, id)
}

// ============================================================================
// Edge CRUD - prefer gRPC when available
// ============================================================================

func (c *Client) CreateEdge(ctx context.Context, source, target uint64, edgeType string, opts ...EdgeOption) (uint64, error) {
	if c.useGRPC {
		return c.grpc.CreateEdge(ctx, source, target, edgeType, opts...)
	}
	return c.json.CreateEdge(ctx, source, target, edgeType, opts...)
}

func (c *Client) GetEdge(ctx context.Context, id uint64) (*Edge, error) {
	if c.useGRPC {
		return c.grpc.GetEdge(ctx, id)
	}
	return c.json.GetEdge(ctx, id)
}

func (c *Client) UpdateEdge(ctx context.Context, id uint64, properties map[string]any) error {
	if c.useGRPC {
		return c.grpc.UpdateEdge(ctx, id, properties)
	}
	return c.json.UpdateEdge(ctx, id, properties)
}

func (c *Client) DeleteEdge(ctx context.Context, id uint64) error {
	if c.useGRPC {
		return c.grpc.DeleteEdge(ctx, id)
	}
	return c.json.DeleteEdge(ctx, id)
}

// ============================================================================
// Traversal - prefer gRPC when available
// ============================================================================

func (c *Client) Neighbors(ctx context.Context, id uint64, opts ...NeighborOption) ([]NeighborEntry, error) {
	if c.useGRPC {
		return c.grpc.Neighbors(ctx, id, opts...)
	}
	return c.json.Neighbors(ctx, id, opts...)
}

func (c *Client) BFS(ctx context.Context, start uint64, maxDepth int) ([]BFSEntry, error) {
	if c.useGRPC {
		return c.grpc.BFS(ctx, start, maxDepth)
	}
	return c.json.BFS(ctx, start, maxDepth)
}

func (c *Client) ShortestPath(ctx context.Context, from, to uint64, weighted bool) (*PathResult, error) {
	if c.useGRPC {
		return c.grpc.ShortestPath(ctx, from, to, weighted)
	}
	return c.json.ShortestPath(ctx, from, to, weighted)
}

// ============================================================================
// Temporal Queries - JSON/TCP only (not in gRPC proto)
// ============================================================================

func (c *Client) NeighborsAt(ctx context.Context, id uint64, direction string, timestamp int64, opts ...NeighborOption) ([]NeighborEntry, error) {
	return c.json.NeighborsAt(ctx, id, direction, timestamp, opts...)
}

func (c *Client) BFSAt(ctx context.Context, start uint64, maxDepth int, timestamp int64) ([]BFSEntry, error) {
	return c.json.BFSAt(ctx, start, maxDepth, timestamp)
}

func (c *Client) ShortestPathAt(ctx context.Context, from, to uint64, timestamp int64, weighted bool) (*PathResult, error) {
	return c.json.ShortestPathAt(ctx, from, to, timestamp, weighted)
}

// ============================================================================
// Vector & Semantic Search
// ============================================================================

func (c *Client) VectorSearch(ctx context.Context, query []float32, k int) ([]SearchResult, error) {
	if c.useGRPC {
		return c.grpc.VectorSearch(ctx, query, k)
	}
	return c.json.VectorSearch(ctx, query, k)
}

func (c *Client) HybridSearch(ctx context.Context, anchor uint64, query []float32, opts ...HybridOption) ([]SearchResult, error) {
	return c.json.HybridSearch(ctx, anchor, query, opts...)
}

func (c *Client) SemanticNeighbors(ctx context.Context, id uint64, concept []float32, opts ...SemanticOption) ([]SearchResult, error) {
	return c.json.SemanticNeighbors(ctx, id, concept, opts...)
}

func (c *Client) SemanticWalk(ctx context.Context, start uint64, concept []float32, maxHops int) ([]WalkStep, error) {
	return c.json.SemanticWalk(ctx, start, concept, maxHops)
}

// ============================================================================
// GQL Query - prefer gRPC when available
// ============================================================================

func (c *Client) Query(ctx context.Context, gql string) (*QueryResult, error) {
	if c.useGRPC {
		return c.grpc.Query(ctx, gql)
	}
	return c.json.Query(ctx, gql)
}

// ============================================================================
// GraphRAG - JSON/TCP only
// ============================================================================

func (c *Client) ExtractSubgraph(ctx context.Context, center uint64, opts ...SubgraphOption) (*SubgraphResult, error) {
	return c.json.ExtractSubgraph(ctx, center, opts...)
}

func (c *Client) GraphRAG(ctx context.Context, question string, opts ...RAGOption) (*RAGResult, error) {
	return c.json.GraphRAG(ctx, question, opts...)
}

// ============================================================================
// Batch Operations - prefer gRPC when available
// ============================================================================

func (c *Client) CreateNodes(ctx context.Context, nodes []NodeInput) ([]uint64, error) {
	if c.useGRPC {
		return c.grpc.CreateNodes(ctx, nodes)
	}
	return c.json.CreateNodes(ctx, nodes)
}

func (c *Client) CreateEdges(ctx context.Context, edges []EdgeInput) ([]uint64, error) {
	if c.useGRPC {
		return c.grpc.CreateEdges(ctx, edges)
	}
	return c.json.CreateEdges(ctx, edges)
}

func (c *Client) DeleteNodes(ctx context.Context, ids []uint64) int {
	if c.useGRPC {
		return c.grpc.DeleteNodes(ctx, ids)
	}
	return c.json.DeleteNodes(ctx, ids)
}

func (c *Client) DeleteEdges(ctx context.Context, ids []uint64) int {
	if c.useGRPC {
		return c.grpc.DeleteEdges(ctx, ids)
	}
	return c.json.DeleteEdges(ctx, ids)
}
