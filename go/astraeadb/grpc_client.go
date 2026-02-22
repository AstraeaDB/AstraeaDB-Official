package astraeadb

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/wrapperspb"

	pb "github.com/AstraeaDB/AstraeaDB-Official/pb/astraea"
)

// GRPCClient communicates with AstraeaDB over gRPC (default port 7688).
// It supports 14 of the 22 server operations; the remaining 8 (temporal,
// semantic, GraphRAG) require the JSON/TCP transport.
type GRPCClient struct {
	cfg  *clientConfig
	mu   sync.Mutex
	conn *grpc.ClientConn
	stub pb.AstraeaServiceClient
}

// NewGRPCClient creates a new gRPC client. Call Connect to establish
// the connection before issuing requests.
func NewGRPCClient(opts ...Option) *GRPCClient {
	cfg := defaultConfig()
	for _, o := range opts {
		o(cfg)
	}
	return &GRPCClient{cfg: cfg}
}

// Connect establishes a gRPC connection to the server.
func (c *GRPCClient) Connect(ctx context.Context) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	var creds grpc.DialOption
	if c.cfg.tlsConfig != nil {
		creds = grpc.WithTransportCredentials(credentials.NewTLS(c.cfg.tlsConfig))
	} else {
		creds = grpc.WithTransportCredentials(insecure.NewCredentials())
	}

	conn, err := grpc.NewClient(c.cfg.grpcAddr(), creds)
	if err != nil {
		return fmt.Errorf("astraeadb: grpc connect %s: %w", c.cfg.grpcAddr(), err)
	}
	c.conn = conn
	c.stub = pb.NewAstraeaServiceClient(conn)
	return nil
}

// Close closes the gRPC connection.
func (c *GRPCClient) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.conn != nil {
		err := c.conn.Close()
		c.conn = nil
		c.stub = nil
		return err
	}
	return nil
}

func (c *GRPCClient) withTimeout(ctx context.Context) (context.Context, context.CancelFunc) {
	return context.WithTimeout(ctx, c.cfg.timeout)
}

func wrapGRPCError(err error) error {
	if err == nil {
		return nil
	}
	st, ok := status.FromError(err)
	if !ok {
		return err
	}
	switch st.Code() {
	case codes.Unavailable:
		return fmt.Errorf("astraeadb: server unavailable: %w", err)
	case codes.DeadlineExceeded:
		return fmt.Errorf("astraeadb: request timed out: %w", err)
	case codes.Unauthenticated:
		return ErrInvalidCreds
	case codes.PermissionDenied:
		return ErrAccessDenied
	default:
		return &AstraeaError{Message: st.Message()}
	}
}

func checkMutation(resp *pb.MutationResponse, err error) (string, error) {
	if err != nil {
		return "", wrapGRPCError(err)
	}
	if !resp.Success {
		return "", classifyError(resp.Error)
	}
	return resp.ResultJson, nil
}

// ============================================================================
// Health
// ============================================================================

// Ping sends a health check.
func (c *GRPCClient) Ping(ctx context.Context) (*PingResponse, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()
	resp, err := c.stub.Ping(ctx, &pb.PingRequest{})
	if err != nil {
		return nil, wrapGRPCError(err)
	}
	return &PingResponse{Pong: resp.Pong, Version: resp.Version}, nil
}

// ============================================================================
// Node CRUD
// ============================================================================

// CreateNode creates a node and returns its ID.
func (c *GRPCClient) CreateNode(ctx context.Context, labels []string, properties map[string]any, embedding []float32) (uint64, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	propsJSON, err := json.Marshal(orEmpty(properties))
	if err != nil {
		return 0, fmt.Errorf("astraeadb: marshal properties: %w", err)
	}

	resultJSON, err := checkMutation(c.stub.CreateNode(ctx, &pb.CreateNodeRequest{
		Labels:         labels,
		PropertiesJson: string(propsJSON),
		Embedding:      embedding,
	}))
	if err != nil {
		return 0, err
	}

	var r struct{ NodeID uint64 `json:"node_id"` }
	json.Unmarshal([]byte(resultJSON), &r)
	return r.NodeID, nil
}

// GetNode retrieves a node by ID.
func (c *GRPCClient) GetNode(ctx context.Context, id uint64) (*Node, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	resp, err := c.stub.GetNode(ctx, &pb.GetNodeRequest{Id: id})
	if err != nil {
		return nil, wrapGRPCError(err)
	}
	if !resp.Found {
		return nil, classifyError(resp.Error)
	}

	var props map[string]any
	if resp.PropertiesJson != "" {
		json.Unmarshal([]byte(resp.PropertiesJson), &props)
	}

	return &Node{
		ID:           resp.Id,
		Labels:       resp.Labels,
		Properties:   props,
		HasEmbedding: resp.HasEmbedding,
	}, nil
}

// UpdateNode updates a node's properties.
func (c *GRPCClient) UpdateNode(ctx context.Context, id uint64, properties map[string]any) error {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	propsJSON, err := json.Marshal(orEmpty(properties))
	if err != nil {
		return fmt.Errorf("astraeadb: marshal properties: %w", err)
	}

	_, err = checkMutation(c.stub.UpdateNode(ctx, &pb.UpdateNodeRequest{
		Id:             id,
		PropertiesJson: string(propsJSON),
	}))
	return err
}

// DeleteNode deletes a node.
func (c *GRPCClient) DeleteNode(ctx context.Context, id uint64) error {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()
	_, err := checkMutation(c.stub.DeleteNode(ctx, &pb.DeleteNodeRequest{Id: id}))
	return err
}

// ============================================================================
// Edge CRUD
// ============================================================================

// CreateEdge creates an edge and returns its ID.
func (c *GRPCClient) CreateEdge(ctx context.Context, source, target uint64, edgeType string, opts ...EdgeOption) (uint64, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	params := map[string]any{
		"properties": map[string]any{},
		"weight":     1.0,
	}
	for _, o := range opts {
		o(params)
	}

	propsJSON, _ := json.Marshal(params["properties"])

	req := &pb.CreateEdgeRequest{
		Source:         source,
		Target:         target,
		EdgeType:       edgeType,
		PropertiesJson: string(propsJSON),
		Weight:         params["weight"].(float64),
	}
	if vf, ok := params["valid_from"]; ok {
		req.ValidFrom = wrapperspb.Int64(vf.(int64))
	}
	if vt, ok := params["valid_to"]; ok {
		req.ValidTo = wrapperspb.Int64(vt.(int64))
	}

	resultJSON, err := checkMutation(c.stub.CreateEdge(ctx, req))
	if err != nil {
		return 0, err
	}

	var r struct{ EdgeID uint64 `json:"edge_id"` }
	json.Unmarshal([]byte(resultJSON), &r)
	return r.EdgeID, nil
}

// GetEdge retrieves an edge by ID.
func (c *GRPCClient) GetEdge(ctx context.Context, id uint64) (*Edge, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	resp, err := c.stub.GetEdge(ctx, &pb.GetEdgeRequest{Id: id})
	if err != nil {
		return nil, wrapGRPCError(err)
	}
	if !resp.Found {
		return nil, classifyError(resp.Error)
	}

	var props map[string]any
	if resp.PropertiesJson != "" {
		json.Unmarshal([]byte(resp.PropertiesJson), &props)
	}

	e := &Edge{
		ID:         resp.Id,
		Source:     resp.Source,
		Target:     resp.Target,
		EdgeType:   resp.EdgeType,
		Properties: props,
		Weight:     resp.Weight,
	}
	if resp.ValidFrom != nil {
		v := resp.ValidFrom.Value
		e.ValidFrom = &v
	}
	if resp.ValidTo != nil {
		v := resp.ValidTo.Value
		e.ValidTo = &v
	}
	return e, nil
}

// UpdateEdge updates an edge's properties.
func (c *GRPCClient) UpdateEdge(ctx context.Context, id uint64, properties map[string]any) error {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	propsJSON, err := json.Marshal(orEmpty(properties))
	if err != nil {
		return fmt.Errorf("astraeadb: marshal properties: %w", err)
	}

	_, err = checkMutation(c.stub.UpdateEdge(ctx, &pb.UpdateEdgeRequest{
		Id:             id,
		PropertiesJson: string(propsJSON),
	}))
	return err
}

// DeleteEdge deletes an edge.
func (c *GRPCClient) DeleteEdge(ctx context.Context, id uint64) error {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()
	_, err := checkMutation(c.stub.DeleteEdge(ctx, &pb.DeleteEdgeRequest{Id: id}))
	return err
}

// ============================================================================
// Traversal
// ============================================================================

// Neighbors returns the immediate neighbors of a node.
func (c *GRPCClient) Neighbors(ctx context.Context, id uint64, opts ...NeighborOption) ([]NeighborEntry, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	params := map[string]any{"direction": "outgoing"}
	for _, o := range opts {
		o(params)
	}

	req := &pb.NeighborsRequest{
		Id:        id,
		Direction: params["direction"].(string),
	}
	if et, ok := params["edge_type"]; ok {
		req.EdgeType = et.(string)
	}

	resp, err := c.stub.Neighbors(ctx, req)
	if err != nil {
		return nil, wrapGRPCError(err)
	}
	if resp.Error != "" {
		return nil, classifyError(resp.Error)
	}

	entries := make([]NeighborEntry, len(resp.Neighbors))
	for i, n := range resp.Neighbors {
		entries[i] = NeighborEntry{EdgeID: n.EdgeId, NodeID: n.NodeId}
	}
	return entries, nil
}

// BFS performs a breadth-first search.
func (c *GRPCClient) BFS(ctx context.Context, start uint64, maxDepth int) ([]BFSEntry, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	resp, err := c.stub.Bfs(ctx, &pb.BfsRequest{
		Start:    start,
		MaxDepth: uint32(maxDepth),
	})
	if err != nil {
		return nil, wrapGRPCError(err)
	}
	if resp.Error != "" {
		return nil, classifyError(resp.Error)
	}

	entries := make([]BFSEntry, len(resp.Nodes))
	for i, n := range resp.Nodes {
		entries[i] = BFSEntry{NodeID: n.NodeId, Depth: int(n.Depth)}
	}
	return entries, nil
}

// ShortestPath finds the shortest path between two nodes.
func (c *GRPCClient) ShortestPath(ctx context.Context, from, to uint64, weighted bool) (*PathResult, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	resp, err := c.stub.ShortestPath(ctx, &pb.ShortestPathRequest{
		From:     from,
		To:       to,
		Weighted: weighted,
	})
	if err != nil {
		return nil, wrapGRPCError(err)
	}
	if resp.Error != "" {
		return nil, classifyError(resp.Error)
	}

	result := &PathResult{
		Found:  resp.Found,
		Path:   resp.Path,
		Length: int(resp.Length),
	}
	if resp.Cost != nil {
		v := resp.Cost.Value
		result.Cost = &v
	}
	return result, nil
}

// ============================================================================
// Vector Search
// ============================================================================

// VectorSearch performs k-nearest-neighbor search.
func (c *GRPCClient) VectorSearch(ctx context.Context, query []float32, k int) ([]SearchResult, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	resp, err := c.stub.VectorSearch(ctx, &pb.VectorSearchRequest{
		Query: query,
		K:     uint32(k),
	})
	if err != nil {
		return nil, wrapGRPCError(err)
	}
	if resp.Error != "" {
		return nil, classifyError(resp.Error)
	}

	results := make([]SearchResult, len(resp.Results))
	for i, r := range resp.Results {
		results[i] = SearchResult{NodeID: r.NodeId, Score: float64(r.Score)}
	}
	return results, nil
}

// ============================================================================
// Query
// ============================================================================

// Query executes a GQL query string.
func (c *GRPCClient) Query(ctx context.Context, gql string) (*QueryResult, error) {
	ctx, cancel := c.withTimeout(ctx)
	defer cancel()

	resp, err := c.stub.Query(ctx, &pb.QueryRequest{Gql: gql})
	if err != nil {
		return nil, wrapGRPCError(err)
	}
	if !resp.Success {
		return nil, classifyError(resp.Error)
	}

	var r QueryResult
	if err := json.Unmarshal([]byte(resp.ResultJson), &r); err != nil {
		return nil, fmt.Errorf("astraeadb: parse query result: %w", err)
	}
	return &r, nil
}

// ============================================================================
// Batch Operations
// ============================================================================

// CreateNodes creates multiple nodes and returns their IDs.
func (c *GRPCClient) CreateNodes(ctx context.Context, nodes []NodeInput) ([]uint64, error) {
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
func (c *GRPCClient) CreateEdges(ctx context.Context, edges []EdgeInput) ([]uint64, error) {
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

// DeleteNodes deletes multiple nodes, returning count deleted.
func (c *GRPCClient) DeleteNodes(ctx context.Context, ids []uint64) int {
	count := 0
	for _, id := range ids {
		if err := c.DeleteNode(ctx, id); err == nil {
			count++
		}
	}
	return count
}

// DeleteEdges deletes multiple edges, returning count deleted.
func (c *GRPCClient) DeleteEdges(ctx context.Context, ids []uint64) int {
	count := 0
	for _, id := range ids {
		if err := c.DeleteEdge(ctx, id); err == nil {
			count++
		}
	}
	return count
}

// assertTimeout is a compile-time check that Duration is exported from time.
var _ time.Duration
