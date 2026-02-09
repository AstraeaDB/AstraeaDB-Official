package com.astraeadb.json;

import javax.net.ssl.SSLContext;
import java.time.Duration;

/**
 * Builder for constructing {@link JsonClient} instances with configurable connection parameters.
 *
 * <pre>{@code
 * AstraeaClient client = new JsonClientBuilder()
 *     .host("db.example.com")
 *     .port(7687)
 *     .authToken("secret")
 *     .timeout(Duration.ofSeconds(15))
 *     .build();
 * }</pre>
 */
public final class JsonClientBuilder {

    private String host = "127.0.0.1";
    private int port = 7687;
    private String authToken;
    private Duration timeout = Duration.ofSeconds(10);
    private Duration connectTimeout = Duration.ofSeconds(5);
    private int maxRetries = 3;
    private boolean reconnect = true;
    private SSLContext sslContext;

    /**
     * Sets the server host address. Defaults to {@code "127.0.0.1"}.
     */
    public JsonClientBuilder host(String host) {
        this.host = host;
        return this;
    }

    /**
     * Sets the server port. Defaults to {@code 7687}.
     */
    public JsonClientBuilder port(int port) {
        this.port = port;
        return this;
    }

    /**
     * Sets the authentication token. If null or not set, no auth_token is sent.
     */
    public JsonClientBuilder authToken(String authToken) {
        this.authToken = authToken;
        return this;
    }

    /**
     * Sets the read timeout (SO_TIMEOUT). Defaults to 10 seconds.
     */
    public JsonClientBuilder timeout(Duration timeout) {
        this.timeout = timeout;
        return this;
    }

    /**
     * Sets the connection timeout. Defaults to 5 seconds.
     */
    public JsonClientBuilder connectTimeout(Duration connectTimeout) {
        this.connectTimeout = connectTimeout;
        return this;
    }

    /**
     * Sets the maximum number of retries for transient failures. Defaults to 3.
     */
    public JsonClientBuilder maxRetries(int maxRetries) {
        this.maxRetries = maxRetries;
        return this;
    }

    /**
     * Enables or disables automatic reconnection. Defaults to {@code true}.
     */
    public JsonClientBuilder reconnect(boolean reconnect) {
        this.reconnect = reconnect;
        return this;
    }

    /**
     * Sets an SSLContext for TLS connections. If null, plain TCP is used.
     */
    public JsonClientBuilder sslContext(SSLContext sslContext) {
        this.sslContext = sslContext;
        return this;
    }

    /**
     * Builds and returns a new {@link JsonClient} instance.
     * The client is not connected until {@link JsonClient#connect()} is called.
     */
    public JsonClient build() {
        return new JsonClient(host, port, authToken, timeout, connectTimeout, maxRetries, reconnect, sslContext);
    }
}
