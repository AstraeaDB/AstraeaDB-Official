// Package backoff provides exponential backoff with jitter for reconnection.
package backoff

import (
	"math"
	"math/rand"
	"time"
)

// Backoff implements exponential backoff with jitter.
type Backoff struct {
	min     time.Duration
	max     time.Duration
	factor  float64
	current time.Duration
}

// New creates a Backoff with the given minimum delay, maximum delay, and factor.
func New(min, max time.Duration, factor float64) *Backoff {
	return &Backoff{
		min:     min,
		max:     max,
		factor:  factor,
		current: min,
	}
}

// Default returns a Backoff with sensible defaults (100ms min, 10s max, 2x factor).
func Default() *Backoff {
	return New(100*time.Millisecond, 10*time.Second, 2.0)
}

// Next returns the next backoff delay and advances the state.
// Jitter of +/- 25% is applied to prevent thundering herd.
func (b *Backoff) Next() time.Duration {
	d := b.current
	b.current = time.Duration(math.Min(float64(b.max), float64(b.current)*b.factor))

	// Add jitter: +/- 25%
	jitter := time.Duration(rand.Int63n(int64(d)/2)) - d/4
	return d + jitter
}

// Reset resets the backoff to the minimum delay.
func (b *Backoff) Reset() {
	b.current = b.min
}
