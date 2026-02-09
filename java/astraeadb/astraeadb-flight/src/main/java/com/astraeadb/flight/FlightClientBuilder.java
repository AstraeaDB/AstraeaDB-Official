package com.astraeadb.flight;

import java.time.Duration;

/**
 * Builder for constructing {@link FlightAstraeaClient} instances with configurable connection parameters.
 *
 * <pre>{@code
 * AstraeaClient client = new FlightClientBuilder()
 *     .host("db.example.com")
 *     .port(7689)
 *     .authToken("secret")
 *     .timeout(Duration.ofSeconds(15))
 *     .useTls(true)
 *     .build();
 * }</pre>
 */
public class FlightClientBuilder {
    private String host = "127.0.0.1";
    private int port = 7689;
    private String authToken;
    private Duration timeout = Duration.ofSeconds(10);
    private boolean useTls = false;

    /**
     * Sets the server host address. Defaults to {@code "127.0.0.1"}.
     */
    public FlightClientBuilder host(String host) { this.host = host; return this; }

    /**
     * Sets the server port. Defaults to {@code 7689}.
     */
    public FlightClientBuilder port(int port) { this.port = port; return this; }

    /**
     * Sets the authentication token. If null or not set, no auth token is sent.
     */
    public FlightClientBuilder authToken(String token) { this.authToken = token; return this; }

    /**
     * Sets the request timeout. Defaults to 10 seconds.
     */
    public FlightClientBuilder timeout(Duration timeout) { this.timeout = timeout; return this; }

    /**
     * Enables or disables TLS for the gRPC connection. Defaults to {@code false}.
     */
    public FlightClientBuilder useTls(boolean tls) { this.useTls = tls; return this; }

    String host() { return host; }
    int port() { return port; }
    String authToken() { return authToken; }
    Duration timeout() { return timeout; }
    boolean useTls() { return useTls; }

    /**
     * Builds and returns a new {@link FlightAstraeaClient} instance.
     * The client is not connected until {@link FlightAstraeaClient#connect()} is called.
     */
    public FlightAstraeaClient build() {
        return new FlightAstraeaClient(this);
    }
}
