package com.astraeadb;

import com.astraeadb.exception.*;
import com.astraeadb.flight.FlightAstraeaClient;
import com.astraeadb.flight.FlightClientBuilder;
import com.astraeadb.grpc.GrpcClient;
import com.astraeadb.grpc.GrpcClientBuilder;
import com.astraeadb.json.JsonClient;
import com.astraeadb.json.JsonClientBuilder;
import com.astraeadb.model.*;
import com.astraeadb.options.*;

import java.time.Duration;
import java.util.List;
import java.util.Map;

/**
 * Unified AstraeaDB client that routes operations to the best available transport
 * with graceful fallback.
 *
 * <p>Transport selection strategy:
 * <ul>
 *   <li><b>JSON/TCP</b> (baseline, required) -- supports all 22 operations</li>
 *   <li><b>gRPC</b> (preferred for single-record CRUD and traversal) -- 14 operations</li>
 *   <li><b>Arrow Flight</b> (preferred for queries and bulk inserts) -- query + bulk ops</li>
 * </ul>
 *
 * <p>On {@link #connect()}, JSON is connected first (must succeed). gRPC and Flight
 * are probed on a best-effort basis; if either is unavailable the client silently
 * falls back to JSON for the operations that transport would have served.
 *
 * <p>Construct via the fluent {@link Builder}:
 * <pre>{@code
 * try (UnifiedClient client = UnifiedClient.builder()
 *         .host("db.example.com")
 *         .authToken("secret")
 *         .build()) {
 *     client.connect();
 *     PingResponse pong = client.ping();
 * }
 * }</pre>
 */
public class UnifiedClient implements AstraeaClient {

    private final JsonClient jsonClient;
    private final GrpcClient grpcClient;
    private final FlightAstraeaClient flightClient;
    private boolean grpcAvailable = false;
    private boolean flightAvailable = false;

    // -----------------------------------------------------------------------
    // Builder
    // -----------------------------------------------------------------------

    /** Creates a new {@link Builder} for configuring a {@code UnifiedClient}. */
    public static Builder builder() { return new Builder(); }

    /**
     * Fluent builder for {@link UnifiedClient}.
     */
    public static class Builder {
        private String host = "127.0.0.1";
        private int jsonPort = 7687;
        private int grpcPort = 7688;
        private int flightPort = 7689;
        private String authToken;
        private Duration timeout = Duration.ofSeconds(10);
        private Duration connectTimeout = Duration.ofSeconds(5);

        /** Sets the server host address. Defaults to {@code "127.0.0.1"}. */
        public Builder host(String h) { this.host = h; return this; }

        /** Sets the JSON/TCP port. Defaults to {@code 7687}. */
        public Builder jsonPort(int p) { this.jsonPort = p; return this; }

        /** Sets the gRPC port. Defaults to {@code 7688}. */
        public Builder grpcPort(int p) { this.grpcPort = p; return this; }

        /** Sets the Arrow Flight port. Defaults to {@code 7689}. */
        public Builder flightPort(int p) { this.flightPort = p; return this; }

        /** Sets the authentication token for all transports. */
        public Builder authToken(String t) { this.authToken = t; return this; }

        /** Sets the read / RPC timeout. Defaults to 10 seconds. */
        public Builder timeout(Duration d) { this.timeout = d; return this; }

        /** Sets the connection timeout (JSON transport). Defaults to 5 seconds. */
        public Builder connectTimeout(Duration d) { this.connectTimeout = d; return this; }

        /**
         * Builds a new {@link UnifiedClient}. The client is not connected until
         * {@link UnifiedClient#connect()} is called.
         */
        public UnifiedClient build() {
            JsonClient json = new JsonClientBuilder()
                .host(host).port(jsonPort)
                .authToken(authToken)
                .timeout(timeout).connectTimeout(connectTimeout)
                .build();
            GrpcClient grpc = new GrpcClientBuilder()
                .host(host).port(grpcPort)
                .authToken(authToken)
                .timeout(timeout)
                .build();
            FlightAstraeaClient flight = new FlightClientBuilder()
                .host(host).port(flightPort)
                .authToken(authToken)
                .timeout(timeout)
                .build();
            return new UnifiedClient(json, grpc, flight);
        }
    }

    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /** Package-private constructor for testing with pre-built transport clients. */
    UnifiedClient(JsonClient json, GrpcClient grpc, FlightAstraeaClient flight) {
        this.jsonClient = json;
        this.grpcClient = grpc;
        this.flightClient = flight;
    }

    // -----------------------------------------------------------------------
    // Connection lifecycle
    // -----------------------------------------------------------------------

    /**
     * Connects all available transports.
     *
     * <p>JSON/TCP is the baseline and <em>must</em> succeed. gRPC and Arrow Flight
     * are probed on a best-effort basis; failure is silently absorbed.
     *
     * @throws AstraeaException if the JSON transport fails to connect
     */
    @Override
    public void connect() throws AstraeaException {
        // Always connect JSON (baseline, required)
        jsonClient.connect();

        // Probe gRPC (best-effort)
        try {
            grpcClient.connect();
            grpcAvailable = true;
        } catch (Exception e) {
            grpcAvailable = false;
        }

        // Probe Arrow Flight (best-effort)
        try {
            flightClient.connect();
            flightAvailable = true;
        } catch (Exception e) {
            flightAvailable = false;
        }
    }

    /**
     * Closes all transport connections. If multiple transports fail to close,
     * only the first exception from JSON is propagated (gRPC / Flight errors
     * are swallowed).
     */
    @Override
    public void close() throws AstraeaException {
        AstraeaException firstError = null;
        try { jsonClient.close(); } catch (AstraeaException e) { firstError = e; }
        try { grpcClient.close(); } catch (Exception ignored) {}
        try { flightClient.close(); } catch (Exception ignored) {}
        if (firstError != null) throw firstError;
    }

    // -----------------------------------------------------------------------
    // Health check -- gRPC preferred, JSON fallback
    // -----------------------------------------------------------------------

    @Override
    public PingResponse ping() throws AstraeaException {
        if (grpcAvailable) return grpcClient.ping();
        return jsonClient.ping();
    }

    // -----------------------------------------------------------------------
    // Node CRUD -- gRPC preferred, JSON fallback
    // -----------------------------------------------------------------------

    @Override
    public long createNode(List<String> labels, Map<String, Object> properties, float[] embedding)
            throws AstraeaException {
        if (grpcAvailable) return grpcClient.createNode(labels, properties, embedding);
        return jsonClient.createNode(labels, properties, embedding);
    }

    @Override
    public Node getNode(long id) throws AstraeaException {
        if (grpcAvailable) return grpcClient.getNode(id);
        return jsonClient.getNode(id);
    }

    @Override
    public void updateNode(long id, Map<String, Object> properties) throws AstraeaException {
        if (grpcAvailable) grpcClient.updateNode(id, properties);
        else jsonClient.updateNode(id, properties);
    }

    @Override
    public void deleteNode(long id) throws AstraeaException {
        if (grpcAvailable) grpcClient.deleteNode(id);
        else jsonClient.deleteNode(id);
    }

    // -----------------------------------------------------------------------
    // Edge CRUD -- gRPC preferred, JSON fallback
    // -----------------------------------------------------------------------

    @Override
    public long createEdge(long source, long target, String edgeType, EdgeOptions options)
            throws AstraeaException {
        if (grpcAvailable) return grpcClient.createEdge(source, target, edgeType, options);
        return jsonClient.createEdge(source, target, edgeType, options);
    }

    @Override
    public Edge getEdge(long id) throws AstraeaException {
        if (grpcAvailable) return grpcClient.getEdge(id);
        return jsonClient.getEdge(id);
    }

    @Override
    public void updateEdge(long id, Map<String, Object> properties) throws AstraeaException {
        if (grpcAvailable) grpcClient.updateEdge(id, properties);
        else jsonClient.updateEdge(id, properties);
    }

    @Override
    public void deleteEdge(long id) throws AstraeaException {
        if (grpcAvailable) grpcClient.deleteEdge(id);
        else jsonClient.deleteEdge(id);
    }

    // -----------------------------------------------------------------------
    // Traversal -- gRPC preferred, JSON fallback
    // -----------------------------------------------------------------------

    @Override
    public List<NeighborEntry> neighbors(long id, NeighborOptions options) throws AstraeaException {
        if (grpcAvailable) return grpcClient.neighbors(id, options);
        return jsonClient.neighbors(id, options);
    }

    @Override
    public List<BfsEntry> bfs(long start, int maxDepth) throws AstraeaException {
        if (grpcAvailable) return grpcClient.bfs(start, maxDepth);
        return jsonClient.bfs(start, maxDepth);
    }

    @Override
    public PathResult shortestPath(long from, long to, boolean weighted) throws AstraeaException {
        if (grpcAvailable) return grpcClient.shortestPath(from, to, weighted);
        return jsonClient.shortestPath(from, to, weighted);
    }

    // -----------------------------------------------------------------------
    // Temporal queries -- JSON only (not supported by gRPC or Flight)
    // -----------------------------------------------------------------------

    @Override
    public List<NeighborEntry> neighborsAt(long id, String direction, long timestamp)
            throws AstraeaException {
        return jsonClient.neighborsAt(id, direction, timestamp);
    }

    @Override
    public List<NeighborEntry> neighborsAt(long id, String direction, long timestamp, String edgeType)
            throws AstraeaException {
        return jsonClient.neighborsAt(id, direction, timestamp, edgeType);
    }

    @Override
    public List<BfsEntry> bfsAt(long start, int maxDepth, long timestamp) throws AstraeaException {
        return jsonClient.bfsAt(start, maxDepth, timestamp);
    }

    @Override
    public PathResult shortestPathAt(long from, long to, long timestamp, boolean weighted)
            throws AstraeaException {
        return jsonClient.shortestPathAt(from, to, timestamp, weighted);
    }

    // -----------------------------------------------------------------------
    // Vector search -- gRPC for vectorSearch, JSON for hybrid/semantic
    // -----------------------------------------------------------------------

    @Override
    public List<SearchResult> vectorSearch(float[] query, int k) throws AstraeaException {
        if (grpcAvailable) return grpcClient.vectorSearch(query, k);
        return jsonClient.vectorSearch(query, k);
    }

    @Override
    public List<SearchResult> hybridSearch(long anchor, float[] query, HybridSearchOptions options)
            throws AstraeaException {
        return jsonClient.hybridSearch(anchor, query, options);
    }

    @Override
    public List<SearchResult> semanticNeighbors(long id, float[] concept, SemanticOptions options)
            throws AstraeaException {
        return jsonClient.semanticNeighbors(id, concept, options);
    }

    @Override
    public List<WalkStep> semanticWalk(long start, float[] concept, int maxHops)
            throws AstraeaException {
        return jsonClient.semanticWalk(start, concept, maxHops);
    }

    // -----------------------------------------------------------------------
    // GQL query -- Arrow Flight preferred, JSON fallback
    // -----------------------------------------------------------------------

    @Override
    public QueryResult query(String gql) throws AstraeaException {
        if (flightAvailable) {
            try {
                return flightClient.query(gql);
            } catch (Exception e) {
                // fallback to JSON on Flight failure
            }
        }
        return jsonClient.query(gql);
    }

    // -----------------------------------------------------------------------
    // GraphRAG -- JSON only
    // -----------------------------------------------------------------------

    @Override
    public SubgraphResult extractSubgraph(long center, SubgraphOptions options)
            throws AstraeaException {
        return jsonClient.extractSubgraph(center, options);
    }

    @Override
    public RagResult graphRag(String question, RagOptions options) throws AstraeaException {
        return jsonClient.graphRag(question, options);
    }

    // -----------------------------------------------------------------------
    // Batch operations -- Arrow Flight preferred, JSON fallback
    // -----------------------------------------------------------------------

    @Override
    public List<Long> createNodes(List<NodeInput> nodes) throws AstraeaException {
        if (flightAvailable) {
            try {
                return flightClient.createNodes(nodes);
            } catch (Exception e) {
                // fallback to JSON on Flight failure
            }
        }
        return jsonClient.createNodes(nodes);
    }

    @Override
    public List<Long> createEdges(List<EdgeInput> edges) throws AstraeaException {
        if (flightAvailable) {
            try {
                return flightClient.createEdges(edges);
            } catch (Exception e) {
                // fallback to JSON on Flight failure
            }
        }
        return jsonClient.createEdges(edges);
    }

    @Override
    public int deleteNodes(List<Long> ids) throws AstraeaException {
        return jsonClient.deleteNodes(ids);
    }

    @Override
    public int deleteEdges(List<Long> ids) throws AstraeaException {
        return jsonClient.deleteEdges(ids);
    }

    // -----------------------------------------------------------------------
    // Transport availability (package-private, for testing)
    // -----------------------------------------------------------------------

    /** Returns {@code true} if the gRPC transport connected successfully. */
    boolean isGrpcAvailable() { return grpcAvailable; }

    /** Returns {@code true} if the Arrow Flight transport connected successfully. */
    boolean isFlightAvailable() { return flightAvailable; }
}
