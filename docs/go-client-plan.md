# AstraeaDB Go Client — Implementation Plan

## Overview

Build a full-featured Go client library for AstraeaDB with feature parity to the existing Python and R clients. The client will support all three transport protocols (JSON/TCP, gRPC, Arrow Flight), provide idiomatic Go APIs with `context.Context` integration, and include comprehensive tests.

**Module:** `github.com/AstraeaDB/AstraeaDB-Official`
**Go Version:** 1.22+
**License:** MIT

---

## 1. Module Structure

```
astraea-go/
├── go.mod
├── go.sum
├── doc.go                          # Package-level documentation
├── client.go                       # UnifiedClient (primary public API)
├── client_test.go
├── json_client.go                  # JSON/TCP transport
├── json_client_test.go
├── grpc_client.go                  # gRPC transport
├── grpc_client_test.go
├── arrow_client.go                 # Arrow Flight transport
├── arrow_client_test.go
├── types.go                        # All request/response structs
├── types_test.go                   # JSON marshal/unmarshal round-trip tests
├── errors.go                       # Sentinel errors & AstraeaError type
├── options.go                      # Functional options (WithTimeout, etc.)
├── internal/
│   ├── protocol/
│   │   ├── ndjson.go               # NDJSON read/write over net.Conn
│   │   └── ndjson_test.go
│   └── backoff/
│       ├── backoff.go              # Exponential backoff with jitter
│       └── backoff_test.go
├── pb/
│   └── astraea/
│       ├── astraea.pb.go           # Generated protobuf messages
│       └── astraea_grpc.pb.go      # Generated gRPC client stub
├── proto/
│   └── astraea.proto               # Copied from server repo
├── Makefile                        # Proto generation, lint, test
└── examples/
    ├── basic/
    │   └── main.go                 # CRUD + traversal demo
    ├── vector_search/
    │   └── main.go                 # Vector + hybrid search demo
    ├── graphrag/
    │   └── main.go                 # GraphRAG pipeline demo
    └── cybersecurity/
        └── main.go                 # Cybersecurity investigation demo
```

### Dependencies (go.mod)

```
module github.com/AstraeaDB/AstraeaDB-Official

go 1.22

require (
    google.golang.org/grpc     v1.78.0
    google.golang.org/protobuf v1.36.0
    github.com/apache/arrow-go/v18 v18.5.1
)
```

---

## 2. Public API Design

### 2.1 Functional Options Pattern

```go
// options.go
type Option func(*clientConfig)

func WithAddress(host string, port int) Option
func WithGRPCPort(port int) Option
func WithFlightURI(uri string) Option
func WithAuthToken(token string) Option
func WithTimeout(d time.Duration) Option
func WithDialTimeout(d time.Duration) Option
func WithTLS(certFile string) Option
func WithMTLS(certFile, keyFile, caFile string) Option
func WithMaxRetries(n int) Option
func WithReconnect(enabled bool) Option
```

### 2.2 Client Interface

All three clients implement this interface, which mirrors the Python/R client API:

```go
// client.go
type Client interface {
    // Connection lifecycle
    Connect(ctx context.Context) error
    Close() error

    // Health
    Ping(ctx context.Context) (*PingResponse, error)

    // Node CRUD
    CreateNode(ctx context.Context, labels []string, properties map[string]any, embedding []float32) (uint64, error)
    GetNode(ctx context.Context, id uint64) (*Node, error)
    UpdateNode(ctx context.Context, id uint64, properties map[string]any) error
    DeleteNode(ctx context.Context, id uint64) error

    // Edge CRUD
    CreateEdge(ctx context.Context, source, target uint64, edgeType string, opts ...EdgeOption) (uint64, error)
    GetEdge(ctx context.Context, id uint64) (*Edge, error)
    UpdateEdge(ctx context.Context, id uint64, properties map[string]any) error
    DeleteEdge(ctx context.Context, id uint64) error

    // Traversal
    Neighbors(ctx context.Context, id uint64, opts ...NeighborOption) ([]NeighborEntry, error)
    BFS(ctx context.Context, start uint64, maxDepth int) ([]BFSEntry, error)
    ShortestPath(ctx context.Context, from, to uint64, weighted bool) (*PathResult, error)

    // Temporal queries
    NeighborsAt(ctx context.Context, id uint64, direction string, timestamp int64, opts ...NeighborOption) ([]NeighborEntry, error)
    BFSAt(ctx context.Context, start uint64, maxDepth int, timestamp int64) ([]BFSEntry, error)
    ShortestPathAt(ctx context.Context, from, to uint64, timestamp int64, weighted bool) (*PathResult, error)

    // Vector & semantic search
    VectorSearch(ctx context.Context, query []float32, k int) ([]SearchResult, error)
    HybridSearch(ctx context.Context, anchor uint64, query []float32, opts ...HybridOption) ([]SearchResult, error)
    SemanticNeighbors(ctx context.Context, id uint64, concept []float32, opts ...SemanticOption) ([]SearchResult, error)
    SemanticWalk(ctx context.Context, start uint64, concept []float32, maxHops int) ([]WalkStep, error)

    // GQL query
    Query(ctx context.Context, gql string) (*QueryResult, error)

    // GraphRAG
    ExtractSubgraph(ctx context.Context, center uint64, opts ...SubgraphOption) (*SubgraphResult, error)
    GraphRAG(ctx context.Context, question string, opts ...RAGOption) (*RAGResult, error)

    // Batch operations
    CreateNodes(ctx context.Context, nodes []NodeInput) ([]uint64, error)
    CreateEdges(ctx context.Context, edges []EdgeInput) ([]uint64, error)
    DeleteNodes(ctx context.Context, ids []uint64) (int, error)
    DeleteEdges(ctx context.Context, ids []uint64) (int, error)
}
```

### 2.3 Constructor Functions

```go
// JSON/TCP client (zero external deps beyond stdlib)
func NewJSONClient(opts ...Option) *JSONClient

// gRPC client (requires google.golang.org/grpc)
func NewGRPCClient(opts ...Option) *GRPCClient

// Arrow Flight client (requires apache/arrow-go)
func NewArrowClient(opts ...Option) *ArrowClient

// Unified client (auto-selects transport per operation)
func NewClient(opts ...Option) *UnifiedClient
```

---

## 3. Type Definitions

### 3.1 Core Types (types.go)

```go
type Node struct {
    ID           uint64         `json:"id"`
    Labels       []string       `json:"labels"`
    Properties   map[string]any `json:"properties"`
    HasEmbedding bool           `json:"has_embedding"`
}

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

type NeighborEntry struct {
    EdgeID uint64 `json:"edge_id"`
    NodeID uint64 `json:"node_id"`
}

type BFSEntry struct {
    NodeID uint64 `json:"node_id"`
    Depth  int    `json:"depth"`
}

type PathResult struct {
    Found  bool     `json:"found"`
    Path   []uint64 `json:"path"`
    Length int      `json:"length"`
    Cost   *float64 `json:"cost,omitempty"`
}

type SearchResult struct {
    NodeID   uint64  `json:"node_id"`
    Distance float32 `json:"distance,omitempty"`
    Score    float32 `json:"score,omitempty"`
}

type WalkStep struct {
    NodeID   uint64  `json:"node_id"`
    Distance float32 `json:"distance"`
}

type QueryResult struct {
    Columns []string   `json:"columns"`
    Rows    [][]any    `json:"rows"`
    Stats   QueryStats `json:"stats"`
}

type QueryStats struct {
    NodesCreated uint64 `json:"nodes_created"`
    EdgesCreated uint64 `json:"edges_created"`
    NodesDeleted uint64 `json:"nodes_deleted"`
    EdgesDeleted uint64 `json:"edges_deleted"`
}

type SubgraphResult struct {
    Text            string `json:"text"`
    NodeCount       int    `json:"nodes_count"`
    EdgeCount       int    `json:"edges_count"`
    EstimatedTokens int    `json:"estimated_tokens"`
}

type RAGResult struct {
    AnchorNodeID    uint64 `json:"anchor_node_id"`
    Context         string `json:"context"`
    Question        string `json:"question"`
    NodesInContext  int    `json:"nodes_in_context"`
    EdgesInContext  int    `json:"edges_in_context"`
    EstimatedTokens int    `json:"estimated_tokens"`
    Note            string `json:"note"`
}

type PingResponse struct {
    Pong    bool   `json:"pong"`
    Version string `json:"version"`
}
```

### 3.2 Wire Protocol Types (internal)

```go
// JSON-TCP request envelope
type jsonRequest struct {
    Type      string  `json:"type"`
    AuthToken *string `json:"auth_token,omitempty"`
    // Remaining fields vary per type, embedded via json.RawMessage
}

// JSON-TCP response envelope
type jsonResponse struct {
    Status  string          `json:"status"`
    Data    json.RawMessage `json:"data,omitempty"`
    Message string          `json:"message,omitempty"`
}
```

### 3.3 Request-Specific Field Maps

Each of the 22 request types maps to a specific JSON struct:

| Request Type | Key Fields |
|---|---|
| `Ping` | *(none)* |
| `CreateNode` | `labels []string`, `properties any`, `embedding []float32` |
| `GetNode` | `id uint64` |
| `UpdateNode` | `id uint64`, `properties any` |
| `DeleteNode` | `id uint64` |
| `CreateEdge` | `source`, `target uint64`, `edge_type string`, `properties any`, `weight float64`, `valid_from *int64`, `valid_to *int64` |
| `GetEdge` | `id uint64` |
| `UpdateEdge` | `id uint64`, `properties any` |
| `DeleteEdge` | `id uint64` |
| `Neighbors` | `id uint64`, `direction string`, `edge_type *string` |
| `Bfs` | `start uint64`, `max_depth int` |
| `ShortestPath` | `from`, `to uint64`, `weighted bool` |
| `VectorSearch` | `query []float32`, `k int` |
| `HybridSearch` | `anchor uint64`, `query []float32`, `max_hops int`, `k int`, `alpha float32` |
| `SemanticNeighbors` | `id uint64`, `concept []float32`, `direction string`, `k int` |
| `SemanticWalk` | `start uint64`, `concept []float32`, `max_hops int` |
| `Query` | `gql string` |
| `ExtractSubgraph` | `center uint64`, `hops int`, `max_nodes int`, `format string` |
| `GraphRag` | `question string`, `anchor *uint64`, `question_embedding []float32`, `hops int`, `max_nodes int`, `format string` |
| `NeighborsAt` | `id uint64`, `direction string`, `timestamp int64`, `edge_type *string` |
| `BfsAt` | `start uint64`, `max_depth int`, `timestamp int64` |
| `ShortestPathAt` | `from`, `to uint64`, `timestamp int64`, `weighted bool` |

---

## 4. Transport Implementations

### 4.1 JSON/TCP Client (json_client.go)

**Protocol:** Newline-delimited JSON over TCP (port 7687)

**Key implementation details:**
- `net.Dialer` with `DialContext` for context-aware connection
- `bufio.Scanner` + `json.Unmarshal` for response reading (1 MB max line buffer)
- `json.Encoder.Encode` for request writing (auto-appends `\n`)
- `sync.Mutex` to protect the shared `net.Conn`
- `conn.SetDeadline` bridged from `context.Context` deadline
- Exponential backoff with jitter for reconnection
- TCP keepalive at 30s intervals
- `TCP_NODELAY` for low-latency request-response

**Core send/receive loop:**
```go
func (c *JSONClient) send(ctx context.Context, req map[string]any) (json.RawMessage, error) {
    c.mu.Lock()
    defer c.mu.Unlock()

    if c.conn == nil {
        return nil, ErrNotConnected
    }

    // Inject auth token
    if c.authToken != "" {
        req["auth_token"] = c.authToken
    }

    // Bridge context deadline to net.Conn
    if deadline, ok := ctx.Deadline(); ok {
        c.conn.SetDeadline(deadline)
    }

    // Write NDJSON request
    if err := json.NewEncoder(c.conn).Encode(req); err != nil {
        c.closeConn()
        return nil, fmt.Errorf("write: %w", err)
    }

    // Read NDJSON response
    if !c.scanner.Scan() {
        err := c.scanner.Err()
        if err == nil { err = io.EOF }
        c.closeConn()
        return nil, fmt.Errorf("read: %w", err)
    }

    var resp jsonResponse
    if err := json.Unmarshal(c.scanner.Bytes(), &resp); err != nil {
        return nil, fmt.Errorf("unmarshal: %w", err)
    }
    if resp.Status == "error" {
        return nil, &AstraeaError{Message: resp.Message}
    }
    return resp.Data, nil
}
```

**TLS support:**
```go
// Plain TCP
conn, err := dialer.DialContext(ctx, "tcp", addr)

// TLS
conn, err := tls.DialWithDialer(dialer, "tcp", addr, tlsConfig)

// mTLS
tlsConfig := &tls.Config{
    Certificates: []tls.Certificate{clientCert},
    RootCAs:      caCertPool,
    MinVersion:   tls.VersionTLS13,
}
```

### 4.2 gRPC Client (grpc_client.go)

**Protocol:** Protobuf over gRPC (port 7688)

**Key implementation details:**
- Use `grpc.NewClient` (NOT deprecated `grpc.Dial`)
- Generated client stub from `astraea.proto`
- Per-RPC `context.WithTimeout` for deadline propagation
- `status.FromError` for gRPC error code inspection
- Lazy connection (connects on first RPC, not at construction)

**Proto code generation (Makefile):**
```makefile
.PHONY: proto
proto:
	protoc \
		--go_out=pb --go_opt=paths=source_relative \
		--go-grpc_out=pb --go-grpc_opt=paths=source_relative \
		proto/astraea.proto
```

**Implementation pattern:**
```go
func (c *GRPCClient) CreateNode(ctx context.Context, labels []string, properties map[string]any, embedding []float32) (uint64, error) {
    ctx, cancel := context.WithTimeout(ctx, c.timeout)
    defer cancel()

    propsJSON, err := json.Marshal(properties)
    if err != nil {
        return 0, fmt.Errorf("marshal properties: %w", err)
    }

    resp, err := c.stub.CreateNode(ctx, &pb.CreateNodeRequest{
        Labels:         labels,
        PropertiesJson: string(propsJSON),
        Embedding:      embedding,
    })
    if err != nil {
        return 0, c.wrapGRPCError(err)
    }
    if !resp.Success {
        return 0, &AstraeaError{Message: resp.Error}
    }

    var result struct{ NodeID uint64 `json:"node_id"` }
    json.Unmarshal([]byte(resp.ResultJson), &result)
    return result.NodeID, nil
}
```

**gRPC-specific methods mapping (14 RPCs):**

| Go Method | gRPC RPC | Request | Response |
|---|---|---|---|
| `CreateNode` | `CreateNode` | `CreateNodeRequest` | `MutationResponse` |
| `GetNode` | `GetNode` | `GetNodeRequest` | `GetNodeResponse` |
| `UpdateNode` | `UpdateNode` | `UpdateNodeRequest` | `MutationResponse` |
| `DeleteNode` | `DeleteNode` | `DeleteNodeRequest` | `MutationResponse` |
| `CreateEdge` | `CreateEdge` | `CreateEdgeRequest` | `MutationResponse` |
| `GetEdge` | `GetEdge` | `GetEdgeRequest` | `GetEdgeResponse` |
| `UpdateEdge` | `UpdateEdge` | `UpdateEdgeRequest` | `MutationResponse` |
| `DeleteEdge` | `DeleteEdge` | `DeleteEdgeRequest` | `MutationResponse` |
| `Neighbors` | `Neighbors` | `NeighborsRequest` | `NeighborsResponse` |
| `BFS` | `Bfs` | `BfsRequest` | `BfsResponse` |
| `ShortestPath` | `ShortestPath` | `ShortestPathRequest` | `ShortestPathResponse` |
| `VectorSearch` | `VectorSearch` | `VectorSearchRequest` | `VectorSearchResponse` |
| `Query` | `Query` | `QueryRequest` | `QueryResponse` |
| `Ping` | `Ping` | `PingRequest` | `PingResponse` |

**Note:** gRPC proto currently covers 14 of the 22 request types. The remaining 8 (temporal queries, semantic search, GraphRAG) are only available via JSON/TCP. The UnifiedClient falls back to JSON for these.

### 4.3 Arrow Flight Client (arrow_client.go)

**Protocol:** Apache Arrow Flight over gRPC (port 7689)

**Key implementation details:**
- `flight.NewClientWithMiddleware` for connection
- `DoGet` with GQL query as Ticket bytes for query execution
- `DoPut` with RecordBatch for bulk node/edge import
- `flight.NewRecordReader` for streaming results
- `array.RecordBuilder` for constructing import batches
- Schema detection: "labels" column → nodes, "edge_type" column → edges

**DoGet (Query execution):**
```go
func (c *ArrowClient) Query(ctx context.Context, gql string) (*arrow.Table, error) {
    ticket := &flight.Ticket{Ticket: []byte(gql)}
    stream, err := c.client.DoGet(ctx, ticket)
    if err != nil {
        return nil, err
    }
    reader, err := flight.NewRecordReader(stream)
    if err != nil {
        return nil, err
    }
    defer reader.Release()
    return reader.ReadAll() // returns arrow.Table
}
```

**DoPut (Bulk import):**
```go
func (c *ArrowClient) BulkInsertNodes(ctx context.Context, records []NodeInput) (*ImportResult, error) {
    // Build Arrow schema matching server's node_schema
    schema := arrow.NewSchema([]arrow.Field{
        {Name: "id", Type: arrow.PrimitiveTypes.Uint64},
        {Name: "labels", Type: arrow.BinaryTypes.String},
        {Name: "properties", Type: arrow.BinaryTypes.String},
        {Name: "has_embedding", Type: arrow.FixedWidthTypes.Boolean},
    }, nil)

    // Build RecordBatch from input
    builder := array.NewRecordBuilder(memory.DefaultAllocator, schema)
    defer builder.Release()

    for _, n := range records {
        builder.Field(0).(*array.Uint64Builder).Append(0) // auto-assigned
        labelsJSON, _ := json.Marshal(n.Labels)
        builder.Field(1).(*array.StringBuilder).Append(string(labelsJSON))
        propsJSON, _ := json.Marshal(n.Properties)
        builder.Field(2).(*array.StringBuilder).Append(string(propsJSON))
        builder.Field(3).(*array.BooleanBuilder).Append(false)
    }

    rec := builder.NewRecord()
    defer rec.Release()

    // Open DoPut stream and write
    stream, err := c.client.DoPut(ctx)
    // ... write rec, read response metadata ...
}
```

**Arrow schemas (must match server):**

| Schema | Columns | Types |
|---|---|---|
| Node | `id`, `labels`, `properties`, `has_embedding` | UInt64, Utf8, Utf8, Boolean |
| Edge | `id`, `source`, `target`, `edge_type`, `properties`, `weight`, `valid_from`, `valid_to` | UInt64, UInt64, UInt64, Utf8, Utf8, Float64, Int64?, Int64? |
| Query Result | *(dynamic from RETURN clause)* | All nullable Utf8 |

### 4.4 Unified Client (client.go)

Routes operations to the optimal transport:

| Operation Category | Primary Transport | Fallback |
|---|---|---|
| CRUD (Node/Edge) | JSON or gRPC | — |
| Traversal (Neighbors, BFS, ShortestPath) | JSON or gRPC | — |
| Query (GQL) | Arrow Flight | JSON |
| Temporal (NeighborsAt, BFSAt, ShortestPathAt) | JSON | — |
| Vector (VectorSearch, HybridSearch) | JSON or gRPC (VectorSearch) | — |
| Semantic (SemanticNeighbors, SemanticWalk) | JSON | — |
| GraphRAG (ExtractSubgraph, GraphRag) | JSON | — |
| Bulk Insert | Arrow Flight | JSON (loop) |

**Graceful degradation:** Arrow Flight is optional. If unavailable (connection fails or not imported), the client silently falls back to JSON/TCP for all operations.

---

## 5. Error Handling

### 5.1 Error Types (errors.go)

```go
// Sentinel errors
var (
    ErrNotConnected    = errors.New("astraea: not connected; call Connect()")
    ErrNodeNotFound    = errors.New("astraea: node not found")
    ErrEdgeNotFound    = errors.New("astraea: edge not found")
    ErrNoVectorIndex   = errors.New("astraea: vector index not configured")
    ErrAccessDenied    = errors.New("astraea: access denied")
    ErrInvalidCreds    = errors.New("astraea: invalid credentials")
    ErrAuthRequired    = errors.New("astraea: authentication required")
)

// Structured error from server
type AstraeaError struct {
    Message string
    Code    string // optional gRPC status code
}

func (e *AstraeaError) Error() string { return e.Message }

// Detect known error types from server message
func classifyError(msg string) error {
    switch {
    case strings.Contains(msg, "not found"):
        if strings.Contains(msg, "node") { return ErrNodeNotFound }
        if strings.Contains(msg, "edge") { return ErrEdgeNotFound }
    case strings.Contains(msg, "vector index not configured"):
        return ErrNoVectorIndex
    case strings.Contains(msg, "access denied"):
        return ErrAccessDenied
    case strings.Contains(msg, "invalid credentials"):
        return ErrInvalidCreds
    case strings.Contains(msg, "authentication required"):
        return ErrAuthRequired
    }
    return &AstraeaError{Message: msg}
}
```

### 5.2 gRPC Error Translation

```go
func (c *GRPCClient) wrapGRPCError(err error) error {
    st, ok := status.FromError(err)
    if !ok {
        return err
    }
    switch st.Code() {
    case codes.Unavailable:
        return fmt.Errorf("astraea: server unavailable: %w", err)
    case codes.DeadlineExceeded:
        return fmt.Errorf("astraea: request timed out: %w", err)
    case codes.InvalidArgument:
        return &AstraeaError{Message: st.Message(), Code: "INVALID_ARGUMENT"}
    case codes.Unauthenticated:
        return ErrInvalidCreds
    case codes.PermissionDenied:
        return ErrAccessDenied
    default:
        return &AstraeaError{Message: st.Message(), Code: st.Code().String()}
    }
}
```

---

## 6. Authentication & TLS

### 6.1 API Key Auth (JSON/TCP)

Injected as `auth_token` field in every JSON request:
```json
{"type":"CreateNode","labels":["Person"],"properties":{},"auth_token":"my-key"}
```

### 6.2 TLS Configuration

```go
// Server TLS only (verify server cert)
client := astraea.NewClient(
    astraea.WithTLS("ca-cert.pem"),
)

// Mutual TLS (client presents cert, server maps CN to role)
client := astraea.NewClient(
    astraea.WithMTLS("client-cert.pem", "client-key.pem", "ca-cert.pem"),
)
```

**CN-to-role mapping (server-side):**
- CN ending in `-admin` → Admin role
- CN ending in `-writer` → Writer role
- All others → Reader role

### 6.3 gRPC Auth

- Passed via `grpc.WithPerRPCCredentials` or metadata injection
- Bearer token in `authorization` metadata header

---

## 7. Testing Strategy

### 7.1 Test Structure (mirroring Python test suite: 41 tests)

| Test Category | Count | Description |
|---|---|---|
| Node CRUD | 5 | Create, create with embedding, get, update, delete |
| Edge CRUD | 5 | Create, create with temporal, get, update, delete |
| Traversal | 4 | Neighbors, neighbors with edge_type, BFS, shortest path |
| Query | 1 | GQL execution |
| Vector Search | 1 | k-NN search |
| Hybrid/Semantic | 3 | Hybrid search, semantic neighbors, semantic walk |
| Temporal | 5 | NeighborsAt, NeighborsAt+edge_type, BFSAt, ShortestPathAt, ShortestPathAt weighted |
| GraphRAG | 4 | Extract structured, extract prose, RAG with anchor, RAG with embedding |
| Batch | 4 | Create nodes batch, create edges batch, delete nodes, delete edges |
| Auth | 2 | Token sent when set, token absent when not set |
| Connection | 2 | Not-connected error, context manager equivalent |
| Wire Protocol | 3 | JSON marshal round-trip, NDJSON framing, partial read |
| gRPC | 7 | Ping, create+get node, create+get edge, delete, neighbors, query, not-found |
| Arrow Flight | 3 | DoGet query, DoPut nodes, DoPut edges |
| **Total** | **49** | |

### 7.2 Mock Strategy

**JSON/TCP tests:** Use `net.Pipe()` to create an in-memory connection pair. Write mock server responses on one end, test client behavior on the other.

```go
func TestCreateNode(t *testing.T) {
    serverConn, clientConn := net.Pipe()
    defer serverConn.Close()
    defer clientConn.Close()

    // Mock server: read request, verify, write response
    go func() {
        scanner := bufio.NewScanner(serverConn)
        scanner.Scan()
        var req map[string]any
        json.Unmarshal(scanner.Bytes(), &req)

        assert.Equal(t, "CreateNode", req["type"])
        assert.Equal(t, []any{"Person"}, req["labels"])

        resp := `{"status":"ok","data":{"node_id":42}}` + "\n"
        serverConn.Write([]byte(resp))
    }()

    client := &JSONClient{conn: clientConn, scanner: bufio.NewScanner(clientConn)}
    id, err := client.CreateNode(context.Background(), []string{"Person"}, nil, nil)
    assert.NoError(t, err)
    assert.Equal(t, uint64(42), id)
}
```

**gRPC tests:** Use `google.golang.org/grpc/test/bufconn` for in-process gRPC testing without a real network.

**Arrow Flight tests:** Use `flight.NewServerWithMiddleware` to create an in-process Flight server with a mock service.

### 7.3 Table-Driven Tests (Go idiom)

```go
func TestNeighborsDirection(t *testing.T) {
    tests := []struct {
        name      string
        direction string
        wantField string
    }{
        {"outgoing", "outgoing", "outgoing"},
        {"incoming", "incoming", "incoming"},
        {"both", "both", "both"},
        {"default", "", "outgoing"},
    }
    for _, tt := range tests {
        t.Run(tt.name, func(t *testing.T) {
            // ... test each direction ...
        })
    }
}
```

---

## 8. Implementation Phases

### Phase 1: JSON/TCP Foundation (Week 1-2)

**Goal:** Core client with CRUD, traversal, and query support over JSON/TCP.

| Task | Files | Tests |
|---|---|---|
| Module scaffolding (`go.mod`, package structure) | `go.mod`, `doc.go` | — |
| Type definitions (all request/response structs) | `types.go` | `types_test.go` (JSON round-trip) |
| Error types and sentinel errors | `errors.go` | — |
| Functional options | `options.go` | — |
| NDJSON protocol (read/write over `net.Conn`) | `internal/protocol/ndjson.go` | `ndjson_test.go` |
| Exponential backoff with jitter | `internal/backoff/backoff.go` | `backoff_test.go` |
| JSON client: Connect, Close, TLS, mTLS | `json_client.go` | Connection tests |
| JSON client: Ping | `json_client.go` | `TestPing` |
| JSON client: Node CRUD (4 methods) | `json_client.go` | 5 tests |
| JSON client: Edge CRUD (4 methods) | `json_client.go` | 5 tests |
| JSON client: Traversal (3 methods) | `json_client.go` | 4 tests |
| JSON client: Query | `json_client.go` | 1 test |
| JSON client: Auth token injection | `json_client.go` | 2 tests |

**Deliverable:** `astraea.NewJSONClient()` supporting 13 operations with 17+ tests.

### Phase 2: Vector, Semantic & Temporal (Week 3)

**Goal:** Add all remaining JSON/TCP operations.

| Task | Files | Tests |
|---|---|---|
| VectorSearch | `json_client.go` | 1 test |
| HybridSearch, SemanticNeighbors, SemanticWalk | `json_client.go` | 3 tests |
| NeighborsAt, BFSAt, ShortestPathAt | `json_client.go` | 5 tests |
| ExtractSubgraph, GraphRAG | `json_client.go` | 4 tests |
| Batch operations (CreateNodes, CreateEdges, etc.) | `json_client.go` | 4 tests |

**Deliverable:** Full 22-operation JSON client with 34+ tests.

### Phase 3: gRPC Transport (Week 4)

**Goal:** gRPC client with protobuf code generation.

| Task | Files | Tests |
|---|---|---|
| Copy `astraea.proto` and generate Go code | `proto/`, `pb/`, `Makefile` | — |
| gRPC client: Connection, Close, TLS | `grpc_client.go` | — |
| gRPC client: 14 RPC methods | `grpc_client.go` | 7 tests |
| gRPC error translation | `grpc_client.go` | 2 tests |

**Deliverable:** `astraea.NewGRPCClient()` supporting 14 operations with 9+ tests.

### Phase 4: Arrow Flight Transport (Week 5)

**Goal:** Arrow Flight client for high-throughput queries and bulk import.

| Task | Files | Tests |
|---|---|---|
| Arrow Flight connection | `arrow_client.go` | — |
| DoGet: GQL query → Arrow Table | `arrow_client.go` | 1 test |
| DoPut: Bulk node import | `arrow_client.go` | 1 test |
| DoPut: Bulk edge import | `arrow_client.go` | 1 test |
| Query result to Go structs conversion | `arrow_client.go` | 1 test |

**Deliverable:** `astraea.NewArrowClient()` with query + bulk import, 4+ tests.

### Phase 5: Unified Client & Polish (Week 6)

**Goal:** Unified client, examples, documentation.

| Task | Files | Tests |
|---|---|---|
| UnifiedClient with transport auto-selection | `client.go` | `client_test.go` |
| Graceful degradation (Arrow → JSON fallback) | `client.go` | 2 tests |
| Basic CRUD example | `examples/basic/main.go` | — |
| Vector search example | `examples/vector_search/main.go` | — |
| GraphRAG example | `examples/graphrag/main.go` | — |
| Cybersecurity demo (port from Python) | `examples/cybersecurity/main.go` | — |
| Package documentation (`doc.go`) | `doc.go` | — |
| README.md | `README.md` | — |

**Deliverable:** Production-ready Go client with 49+ tests and 4 examples.

---

## 9. Default Parameter Values

Matching server defaults for consistency:

| Parameter | Default | Used By |
|---|---|---|
| `max_depth` | 3 | BFS, BFSAt |
| `weighted` | false | ShortestPath, ShortestPathAt |
| `k` | 10 | VectorSearch, HybridSearch, SemanticNeighbors |
| `max_hops` | 3 | HybridSearch, SemanticWalk |
| `alpha` | 0.5 | HybridSearch |
| `direction` | "outgoing" | Neighbors, SemanticNeighbors, NeighborsAt |
| `hops` | 3 | ExtractSubgraph, GraphRag |
| `max_nodes` | 50 | ExtractSubgraph, GraphRag |
| `format` | "structured" | ExtractSubgraph, GraphRag |
| `weight` | 1.0 | CreateEdge |
| `properties` | `{}` | CreateNode, CreateEdge |
| TCP port | 7687 | JSONClient |
| gRPC port | 7688 | GRPCClient |
| Flight port | 7689 | ArrowClient |
| Timeout | 10s | All operations |
| Dial timeout | 5s | Connection |

---

## 10. Key Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| **Connection model** | `net.Dialer.DialContext` | Context integration, keepalive, timeout distribution |
| **NDJSON reading** | `bufio.Scanner` + `json.Unmarshal` | Line-level error isolation, 1MB buffer limit, OOM protection |
| **NDJSON writing** | `json.Encoder.Encode` | Zero-copy to `io.Writer`, auto-appends newline |
| **Thread safety** | `sync.Mutex` on client struct | Simpler than channels for guarding shared `net.Conn` |
| **Reconnection** | Exponential backoff with jitter | Prevents thundering herd; respects context cancellation |
| **Configuration** | Functional options pattern | Extensible without breaking callers |
| **gRPC connection** | `grpc.NewClient` (not `Dial`) | `Dial` deprecated since gRPC-Go v1.63; lazy connect |
| **Arrow library** | `github.com/apache/arrow-go/v18` | Latest stable; moved from monorepo |
| **Optional params** | Variadic option functions | `Neighbors(ctx, id, WithDirection("incoming"), WithEdgeType("KNOWS"))` |
| **Error handling** | Sentinel errors + `AstraeaError` struct | `errors.Is(err, ErrNodeNotFound)` for programmatic checks |
| **Zero deps for JSON** | Only `net`, `encoding/json`, `bufio`, `sync` | Matches Python client's zero-dep design |
