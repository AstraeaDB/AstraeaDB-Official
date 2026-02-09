package astraeadb

import (
	"errors"
	"strings"
)

// Sentinel errors for common server responses.
var (
	ErrNotConnected  = errors.New("astraeadb: not connected; call Connect() first")
	ErrNodeNotFound  = errors.New("astraeadb: node not found")
	ErrEdgeNotFound  = errors.New("astraeadb: edge not found")
	ErrNoVectorIndex = errors.New("astraeadb: vector index not configured")
	ErrAccessDenied  = errors.New("astraeadb: access denied")
	ErrInvalidCreds  = errors.New("astraeadb: invalid credentials")
	ErrAuthRequired  = errors.New("astraeadb: authentication required")
)

// AstraeaError represents a server-side error returned by AstraeaDB.
type AstraeaError struct {
	Message string
}

func (e *AstraeaError) Error() string {
	return "astraeadb: " + e.Message
}

// classifyError inspects a server error message and returns the appropriate
// sentinel error if recognized, or a generic AstraeaError otherwise.
func classifyError(msg string) error {
	lower := strings.ToLower(msg)
	switch {
	case strings.Contains(lower, "node") && strings.Contains(lower, "not found"):
		return ErrNodeNotFound
	case strings.Contains(lower, "edge") && strings.Contains(lower, "not found"):
		return ErrEdgeNotFound
	case strings.Contains(lower, "vector index not configured"):
		return ErrNoVectorIndex
	case strings.Contains(lower, "access denied"):
		return ErrAccessDenied
	case strings.Contains(lower, "invalid credentials"):
		return ErrInvalidCreds
	case strings.Contains(lower, "authentication required"):
		return ErrAuthRequired
	}
	return &AstraeaError{Message: msg}
}
