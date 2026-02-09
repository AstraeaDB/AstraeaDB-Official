package astraeadb

// ArrowClient provides high-throughput query execution and bulk data import
// via Apache Arrow Flight (default port 7689).
//
// This file defines the ArrowClient type and its methods. The actual Arrow
// Flight implementation requires the github.com/apache/arrow-go/v18 module.
// To keep the core module dependency-free, Arrow Flight support is provided
// as a build-tag-gated extension. Users who need Arrow Flight should import
// the arrow sub-package (future work) or use the UnifiedClient which falls
// back to JSON/TCP when Arrow is unavailable.
//
// For now, this file provides the type definition and constructor so the
// unified client can reference it, with stub methods that return clear errors.

import (
	"context"
	"errors"
	"fmt"
)

var errArrowNotAvailable = errors.New("astraeadb: Arrow Flight client not available; use JSONClient or GRPCClient")

// ArrowClient communicates with AstraeaDB over Apache Arrow Flight.
type ArrowClient struct {
	cfg       *clientConfig
	connected bool
}

// NewArrowClient creates a new Arrow Flight client.
func NewArrowClient(opts ...Option) *ArrowClient {
	cfg := defaultConfig()
	for _, o := range opts {
		o(cfg)
	}
	return &ArrowClient{cfg: cfg}
}

// Connect establishes an Arrow Flight connection to the server.
func (c *ArrowClient) Connect(ctx context.Context) error {
	// Arrow Flight requires the apache/arrow-go module. This stub
	// implementation connects via the Flight client when available.
	//
	// To keep go.mod minimal, the full implementation is deferred.
	// The unified client detects this and falls back to JSON.
	c.connected = false
	return fmt.Errorf("astraeadb: Arrow Flight connect to %s: %w", c.cfg.flightAddr(), errArrowNotAvailable)
}

// Close closes the Arrow Flight connection.
func (c *ArrowClient) Close() error {
	c.connected = false
	return nil
}

// IsConnected returns whether the Arrow Flight client is connected.
func (c *ArrowClient) IsConnected() bool {
	return c.connected
}

// QueryRaw executes a GQL query and returns raw JSON bytes.
// When fully implemented, this returns Arrow RecordBatches.
func (c *ArrowClient) QueryRaw(ctx context.Context, gql string) ([]byte, error) {
	return nil, errArrowNotAvailable
}

// BulkInsertNodes imports nodes via Arrow Flight DoPut.
func (c *ArrowClient) BulkInsertNodes(ctx context.Context, nodes []NodeInput) (int, error) {
	return 0, errArrowNotAvailable
}

// BulkInsertEdges imports edges via Arrow Flight DoPut.
func (c *ArrowClient) BulkInsertEdges(ctx context.Context, edges []EdgeInput) (int, error) {
	return 0, errArrowNotAvailable
}
