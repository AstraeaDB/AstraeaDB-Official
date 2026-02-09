package backoff

import (
	"testing"
	"time"
)

func TestDefaultBackoff(t *testing.T) {
	b := Default()
	if b.min != 100*time.Millisecond {
		t.Errorf("min = %v, want 100ms", b.min)
	}
	if b.max != 10*time.Second {
		t.Errorf("max = %v, want 10s", b.max)
	}
}

func TestBackoffIncreases(t *testing.T) {
	b := New(100*time.Millisecond, 10*time.Second, 2.0)

	// First call should be around 100ms (with jitter).
	d1 := b.Next()
	if d1 < 75*time.Millisecond || d1 > 125*time.Millisecond {
		t.Errorf("first delay = %v, expected ~100ms", d1)
	}

	// Second call should be around 200ms (with jitter).
	d2 := b.Next()
	if d2 < 150*time.Millisecond || d2 > 250*time.Millisecond {
		t.Errorf("second delay = %v, expected ~200ms", d2)
	}
}

func TestBackoffCapsAtMax(t *testing.T) {
	b := New(1*time.Second, 2*time.Second, 10.0)

	// First call: ~1s
	b.Next()
	// Second call should cap at max (~2s with jitter).
	d := b.Next()
	if d > 3*time.Second {
		t.Errorf("delay = %v, should not exceed max by much", d)
	}
}

func TestBackoffReset(t *testing.T) {
	b := New(100*time.Millisecond, 10*time.Second, 2.0)

	b.Next()
	b.Next()
	b.Reset()

	d := b.Next()
	if d < 75*time.Millisecond || d > 125*time.Millisecond {
		t.Errorf("after reset, delay = %v, expected ~100ms", d)
	}
}
