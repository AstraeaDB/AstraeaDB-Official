package com.astraeadb;

import org.junit.jupiter.api.Test;

import java.time.Duration;

import static org.assertj.core.api.Assertions.*;

class UnifiedClientTest {

    @Test
    void builderDefaults() {
        // Just verify the builder doesn't throw
        var client = UnifiedClient.builder()
            .host("localhost")
            .jsonPort(7687)
            .grpcPort(7688)
            .flightPort(7689)
            .timeout(Duration.ofSeconds(5))
            .build();
        assertThat(client).isNotNull();
    }

    @Test
    void builderWithAuth() {
        var client = UnifiedClient.builder()
            .host("127.0.0.1")
            .authToken("test-token")
            .build();
        assertThat(client).isNotNull();
    }

    @Test
    void connectFails_whenNoServer() {
        var client = UnifiedClient.builder()
            .host("192.0.2.1") // non-routable
            .connectTimeout(Duration.ofMillis(100))
            .timeout(Duration.ofMillis(100))
            .build();
        // JSON connect will fail since there's no server
        assertThatThrownBy(client::connect).isInstanceOf(Exception.class);
    }
}
