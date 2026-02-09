package com.astraeadb.json;

import java.time.Duration;
import java.util.concurrent.ThreadLocalRandom;

/**
 * Exponential backoff with jitter for retry logic.
 * Initial delay: 100ms, max: 30s, multiplier: 2.0, jitter: +/-25%.
 */
final class ExponentialBackoff {

    private static final long INITIAL_DELAY_MS = 100;
    private static final long MAX_DELAY_MS = 30_000;
    private static final double MULTIPLIER = 2.0;
    private static final double JITTER_FRACTION = 0.25;

    private long currentDelayMs = INITIAL_DELAY_MS;

    /**
     * Returns the next delay duration, applying exponential growth and random jitter.
     * Each call advances the internal state so subsequent calls yield longer delays.
     */
    Duration nextDelay() {
        long base = currentDelayMs;
        // Apply jitter: +/- 25%
        double jitter = 1.0 + ThreadLocalRandom.current().nextDouble(-JITTER_FRACTION, JITTER_FRACTION);
        long jittered = Math.round(base * jitter);
        // Advance for next call
        currentDelayMs = Math.min((long) (currentDelayMs * MULTIPLIER), MAX_DELAY_MS);
        return Duration.ofMillis(jittered);
    }

    /**
     * Resets the backoff to its initial state.
     */
    void reset() {
        currentDelayMs = INITIAL_DELAY_MS;
    }
}
