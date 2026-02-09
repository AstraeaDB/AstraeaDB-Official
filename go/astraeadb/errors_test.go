package astraeadb

import (
	"errors"
	"testing"
)

func TestClassifyError(t *testing.T) {
	tests := []struct {
		msg  string
		want error
	}{
		{"Node not found", ErrNodeNotFound},
		{"node 42 NOT FOUND in storage", ErrNodeNotFound},
		{"Edge not found", ErrEdgeNotFound},
		{"Vector index not configured", ErrNoVectorIndex},
		{"Access denied for this operation", ErrAccessDenied},
		{"Invalid credentials provided", ErrInvalidCreds},
		{"Authentication required", ErrAuthRequired},
		{"some unknown error", nil}, // generic AstraeaError
	}

	for _, tc := range tests {
		err := classifyError(tc.msg)
		if tc.want != nil {
			if !errors.Is(err, tc.want) {
				t.Errorf("classifyError(%q) = %v, want %v", tc.msg, err, tc.want)
			}
		} else {
			var ae *AstraeaError
			if !errors.As(err, &ae) {
				t.Errorf("classifyError(%q) should return *AstraeaError, got %T", tc.msg, err)
			}
			if ae.Message != tc.msg {
				t.Errorf("AstraeaError.Message = %q, want %q", ae.Message, tc.msg)
			}
		}
	}
}

func TestAstraeaErrorMessage(t *testing.T) {
	err := &AstraeaError{Message: "test error"}
	want := "astraeadb: test error"
	if err.Error() != want {
		t.Errorf("Error() = %q, want %q", err.Error(), want)
	}
}
