// Package protocol implements the newline-delimited JSON wire protocol
// used by AstraeaDB's JSON/TCP transport.
package protocol

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"time"
)

// MaxLineSize is the maximum size of a single NDJSON line (1 MB).
const MaxLineSize = 1024 * 1024

// Conn wraps a net.Conn with NDJSON read/write capabilities.
type Conn struct {
	conn    net.Conn
	scanner *bufio.Scanner
}

// NewConn wraps an existing net.Conn for NDJSON communication.
func NewConn(c net.Conn) *Conn {
	scanner := bufio.NewScanner(c)
	scanner.Buffer(make([]byte, 0, MaxLineSize), MaxLineSize)
	return &Conn{conn: c, scanner: scanner}
}

// Send marshals v to JSON and writes it as a single NDJSON line.
func (c *Conn) Send(v any) error {
	return json.NewEncoder(c.conn).Encode(v)
}

// Receive reads one NDJSON line and unmarshals it into v.
func (c *Conn) Receive(v any) error {
	if !c.scanner.Scan() {
		if err := c.scanner.Err(); err != nil {
			return fmt.Errorf("read: %w", err)
		}
		return io.EOF
	}
	return json.Unmarshal(c.scanner.Bytes(), v)
}

// SetDeadline sets the read/write deadline on the underlying connection.
func (c *Conn) SetDeadline(t time.Time) error {
	return c.conn.SetDeadline(t)
}

// Close closes the underlying connection.
func (c *Conn) Close() error {
	return c.conn.Close()
}

// Raw returns the underlying net.Conn.
func (c *Conn) Raw() net.Conn {
	return c.conn
}
