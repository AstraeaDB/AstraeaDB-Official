package com.astraeadb.json;

import org.junit.jupiter.api.Test;

import java.time.Duration;

import static org.assertj.core.api.Assertions.assertThat;

class BackoffTest {

    @Test
    void defaultValues() {
        ExponentialBackoff backoff = new ExponentialBackoff();
        Duration d = backoff.nextDelay();
        // 100ms with +/-25% jitter → 75ms to 125ms
        assertThat(d.toMillis()).isBetween(75L, 125L);
    }

    @Test
    void exponentialIncrease() {
        ExponentialBackoff backoff = new ExponentialBackoff();
        long first = backoff.nextDelay().toMillis();
        long second = backoff.nextDelay().toMillis();
        long third = backoff.nextDelay().toMillis();
        // The base doubles each time: 100 → 200 → 400
        // With jitter, second should generally be larger than first
        // We check the third is larger than the initial base to confirm growth
        assertThat(third).isGreaterThan(first);
    }

    @Test
    void maxCap() {
        ExponentialBackoff backoff = new ExponentialBackoff();
        // Call many times to reach the cap
        Duration last = Duration.ZERO;
        for (int i = 0; i < 50; i++) {
            last = backoff.nextDelay();
        }
        // 30_000ms cap with 25% jitter → max possible is 37500ms
        assertThat(last.toMillis()).isLessThanOrEqualTo(37_500L);
        // Base should be capped at 30000, so with -25% jitter, min is 22500
        assertThat(last.toMillis()).isGreaterThanOrEqualTo(22_500L);
    }

    @Test
    void reset() {
        ExponentialBackoff backoff = new ExponentialBackoff();
        // Advance several times
        for (int i = 0; i < 10; i++) {
            backoff.nextDelay();
        }
        backoff.reset();
        Duration d = backoff.nextDelay();
        // After reset, should be back to ~100ms (75-125ms with jitter)
        assertThat(d.toMillis()).isBetween(75L, 125L);
    }
}
