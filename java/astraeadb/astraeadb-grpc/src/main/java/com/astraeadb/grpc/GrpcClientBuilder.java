package com.astraeadb.grpc;

import java.time.Duration;

/**
 * Builder for constructing {@link GrpcClient} instances with configurable connection parameters.
 *
 * <pre>{@code
 * AstraeaClient client = new GrpcClientBuilder()
 *     .host("db.example.com")
 *     .port(7688)
 *     .authToken("secret")
 *     .timeout(Duration.ofSeconds(15))
 *     .build();
 * }</pre>
 */
public final class GrpcClientBuilder {

    private String host = "127.0.0.1";
    private int port = 7688;
    private String authToken;
    private Duration timeout = Duration.ofSeconds(10);
    private boolean usePlaintext = true;

    /**
     * Sets the server host address. Defaults to {@code "127.0.0.1"}.
     */
    public GrpcClientBuilder host(String host) {
        this.host = host;
        return this;
    }

    /**
     * Sets the server gRPC port. Defaults to {@code 7688}.
     */
    public GrpcClientBuilder port(int port) {
        this.port = port;
        return this;
    }

    /**
     * Sets the authentication token. If null or not set, no auth metadata is sent.
     */
    public GrpcClientBuilder authToken(String token) {
        this.authToken = token;
        return this;
    }

    /**
     * Sets the per-RPC deadline timeout. Defaults to 10 seconds.
     */
    public GrpcClientBuilder timeout(Duration timeout) {
        this.timeout = timeout;
        return this;
    }

    /**
     * Enables or disables plaintext (non-TLS) connections. Defaults to {@code true}.
     */
    public GrpcClientBuilder usePlaintext(boolean usePlaintext) {
        this.usePlaintext = usePlaintext;
        return this;
    }

    // Package-private accessors used by GrpcClient constructor
    String host() { return host; }
    int port() { return port; }
    String authToken() { return authToken; }
    Duration timeout() { return timeout; }
    boolean usePlaintext() { return usePlaintext; }

    /**
     * Builds and returns a new {@link GrpcClient} instance.
     * The client is not connected until {@link GrpcClient#connect()} is called.
     */
    public GrpcClient build() {
        return new GrpcClient(this);
    }
}
