package com.astraeadb.grpc;

import com.astraeadb.AstraeaClient;
import com.astraeadb.exception.*;
import com.astraeadb.model.*;
import com.astraeadb.options.*;

import com.astraeadb.grpc.proto.AstraeaServiceGrpc;
import com.astraeadb.grpc.proto.BfsRequest;
import com.astraeadb.grpc.proto.BfsResponse;
import com.astraeadb.grpc.proto.CreateEdgeRequest;
import com.astraeadb.grpc.proto.CreateNodeRequest;
import com.astraeadb.grpc.proto.DeleteEdgeRequest;
import com.astraeadb.grpc.proto.DeleteNodeRequest;
import com.astraeadb.grpc.proto.GetEdgeRequest;
import com.astraeadb.grpc.proto.GetEdgeResponse;
import com.astraeadb.grpc.proto.GetNodeRequest;
import com.astraeadb.grpc.proto.GetNodeResponse;
import com.astraeadb.grpc.proto.MutationResponse;
import com.astraeadb.grpc.proto.NeighborsRequest;
import com.astraeadb.grpc.proto.NeighborsResponse;
import com.astraeadb.grpc.proto.PingRequest;
import com.astraeadb.grpc.proto.QueryRequest;
import com.astraeadb.grpc.proto.QueryResponse;
import com.astraeadb.grpc.proto.ShortestPathRequest;
import com.astraeadb.grpc.proto.ShortestPathResponse;
import com.astraeadb.grpc.proto.UpdateEdgeRequest;
import com.astraeadb.grpc.proto.UpdateNodeRequest;
import com.astraeadb.grpc.proto.VectorSearchRequest;
import com.astraeadb.grpc.proto.VectorSearchResponse;
import com.astraeadb.grpc.proto.VectorSearchResult;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;

import com.google.protobuf.Int64Value;

import io.grpc.ManagedChannel;
import io.grpc.ManagedChannelBuilder;
import io.grpc.Status;
import io.grpc.StatusRuntimeException;

import java.io.IOException;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.TimeUnit;

/**
 * gRPC-based implementation of {@link AstraeaClient}.
 *
 * <p>Supports the 14 operations exposed by the AstraeaDB gRPC service:
 * ping, createNode, getNode, updateNode, deleteNode, createEdge, getEdge,
 * updateEdge, deleteEdge, neighbors, bfs, shortestPath, vectorSearch, and query.
 *
 * <p>Temporal queries, semantic/hybrid search, subgraph extraction, GraphRAG,
 * and batch operations are not available over gRPC and will throw
 * {@link UnsupportedOperationException}.
 */
public final class GrpcClient implements AstraeaClient {

    private static final String UNSUPPORTED_MSG =
            "Operation not supported over gRPC; use JsonClient or UnifiedClient";

    private ManagedChannel channel;
    private AstraeaServiceGrpc.AstraeaServiceBlockingStub stub;
    private final Duration timeout;
    private final ObjectMapper mapper;
    private volatile boolean connected;

    // Builder fields (kept for connect/reconnect)
    private final String host;
    private final int port;
    private final String authToken;
    private final boolean usePlaintext;

    /**
     * Constructs a GrpcClient from a builder. The client is not connected
     * until {@link #connect()} is called.
     */
    GrpcClient(GrpcClientBuilder builder) {
        this.host = builder.host();
        this.port = builder.port();
        this.authToken = builder.authToken();
        this.timeout = builder.timeout();
        this.usePlaintext = builder.usePlaintext();
        this.mapper = new ObjectMapper();
        this.connected = false;
    }

    /**
     * Package-private constructor for testing with in-process channels.
     */
    GrpcClient(ManagedChannel channel) {
        this.channel = channel;
        this.stub = AstraeaServiceGrpc.newBlockingStub(channel);
        this.timeout = Duration.ofSeconds(10);
        this.mapper = new ObjectMapper();
        this.connected = true;
        this.host = "in-process";
        this.port = 0;
        this.authToken = null;
        this.usePlaintext = true;
    }

    // -----------------------------------------------------------------------
    // Connection lifecycle
    // -----------------------------------------------------------------------

    @Override
    public void connect() throws AstraeaException {
        if (connected) return;
        try {
            var builder = ManagedChannelBuilder
                    .forAddress(host, port)
                    .maxInboundMessageSize(64 * 1024 * 1024); // 64 MiB
            if (usePlaintext) {
                builder.usePlaintext();
            }
            this.channel = builder.build();
            this.stub = AstraeaServiceGrpc.newBlockingStub(channel);
            this.connected = true;
        } catch (Exception e) {
            throw new AstraeaException("Failed to open gRPC channel: " + e.getMessage(), e);
        }
    }

    @Override
    public void close() throws AstraeaException {
        if (channel != null && !channel.isShutdown()) {
            try {
                channel.shutdown().awaitTermination(5, TimeUnit.SECONDS);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new AstraeaException("Interrupted while closing gRPC channel", e);
            }
        }
        connected = false;
    }

    private void ensureConnected() throws NotConnectedException {
        if (!connected) throw new NotConnectedException();
    }

    // -----------------------------------------------------------------------
    // Health check
    // -----------------------------------------------------------------------

    @Override
    public PingResponse ping() throws AstraeaException {
        ensureConnected();
        try {
            com.astraeadb.grpc.proto.PingResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .ping(PingRequest.newBuilder().build());
            return new PingResponse(resp.getPong(), resp.getVersion());
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        }
    }

    // -----------------------------------------------------------------------
    // Node CRUD
    // -----------------------------------------------------------------------

    @Override
    public long createNode(List<String> labels, Map<String, Object> properties, float[] embedding)
            throws AstraeaException {
        ensureConnected();
        try {
            var reqBuilder = CreateNodeRequest.newBuilder()
                    .addAllLabels(labels)
                    .setPropertiesJson(mapper.writeValueAsString(
                            properties != null ? properties : Map.of()));
            if (embedding != null) {
                for (float v : embedding) {
                    reqBuilder.addEmbedding(v);
                }
            }
            MutationResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .createNode(reqBuilder.build());
            if (!resp.getSuccess()) {
                throw ErrorClassifier.classify(resp.getError());
            }
            JsonNode result = mapper.readTree(resp.getResultJson());
            return result.path("node_id").asLong();
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        } catch (IOException e) {
            throw new AstraeaException("JSON processing error", e);
        }
    }

    @Override
    public Node getNode(long id) throws AstraeaException {
        ensureConnected();
        try {
            GetNodeResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .getNode(GetNodeRequest.newBuilder().setId(id).build());
            if (!resp.getError().isEmpty()) {
                throw ErrorClassifier.classify(resp.getError());
            }
            if (!resp.getFound()) {
                throw new NodeNotFoundException("node not found with id " + id);
            }
            Map<String, Object> props = resp.getPropertiesJson().isEmpty()
                    ? Map.of()
                    : mapper.readValue(resp.getPropertiesJson(), new TypeReference<>() {});
            return new Node(resp.getId(), List.copyOf(resp.getLabelsList()), props, resp.getHasEmbedding());
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        } catch (IOException e) {
            throw new AstraeaException("JSON processing error", e);
        }
    }

    @Override
    public void updateNode(long id, Map<String, Object> properties) throws AstraeaException {
        ensureConnected();
        try {
            MutationResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .updateNode(UpdateNodeRequest.newBuilder()
                            .setId(id)
                            .setPropertiesJson(mapper.writeValueAsString(
                                    properties != null ? properties : Map.of()))
                            .build());
            if (!resp.getSuccess()) {
                throw ErrorClassifier.classify(resp.getError());
            }
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        } catch (JsonProcessingException e) {
            throw new AstraeaException("JSON processing error", e);
        }
    }

    @Override
    public void deleteNode(long id) throws AstraeaException {
        ensureConnected();
        try {
            MutationResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .deleteNode(DeleteNodeRequest.newBuilder().setId(id).build());
            if (!resp.getSuccess()) {
                throw ErrorClassifier.classify(resp.getError());
            }
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        }
    }

    // -----------------------------------------------------------------------
    // Edge CRUD
    // -----------------------------------------------------------------------

    @Override
    public long createEdge(long source, long target, String edgeType, EdgeOptions options)
            throws AstraeaException {
        ensureConnected();
        try {
            var reqBuilder = CreateEdgeRequest.newBuilder()
                    .setSource(source)
                    .setTarget(target)
                    .setEdgeType(edgeType)
                    .setPropertiesJson(mapper.writeValueAsString(
                            options.properties() != null ? options.properties() : Map.of()))
                    .setWeight(options.weight());
            if (options.validFrom() != null) {
                reqBuilder.setValidFrom(Int64Value.of(options.validFrom()));
            }
            if (options.validTo() != null) {
                reqBuilder.setValidTo(Int64Value.of(options.validTo()));
            }
            MutationResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .createEdge(reqBuilder.build());
            if (!resp.getSuccess()) {
                throw ErrorClassifier.classify(resp.getError());
            }
            JsonNode result = mapper.readTree(resp.getResultJson());
            return result.path("edge_id").asLong();
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        } catch (IOException e) {
            throw new AstraeaException("JSON processing error", e);
        }
    }

    @Override
    public Edge getEdge(long id) throws AstraeaException {
        ensureConnected();
        try {
            GetEdgeResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .getEdge(GetEdgeRequest.newBuilder().setId(id).build());
            if (!resp.getError().isEmpty()) {
                throw ErrorClassifier.classify(resp.getError());
            }
            if (!resp.getFound()) {
                throw new EdgeNotFoundException("edge not found with id " + id);
            }
            Map<String, Object> props = resp.getPropertiesJson().isEmpty()
                    ? Map.of()
                    : mapper.readValue(resp.getPropertiesJson(), new TypeReference<>() {});
            Long validFrom = resp.hasValidFrom() ? resp.getValidFrom().getValue() : null;
            Long validTo = resp.hasValidTo() ? resp.getValidTo().getValue() : null;
            return new Edge(resp.getId(), resp.getSource(), resp.getTarget(),
                    resp.getEdgeType(), props, resp.getWeight(), validFrom, validTo);
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        } catch (IOException e) {
            throw new AstraeaException("JSON processing error", e);
        }
    }

    @Override
    public void updateEdge(long id, Map<String, Object> properties) throws AstraeaException {
        ensureConnected();
        try {
            MutationResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .updateEdge(UpdateEdgeRequest.newBuilder()
                            .setId(id)
                            .setPropertiesJson(mapper.writeValueAsString(
                                    properties != null ? properties : Map.of()))
                            .build());
            if (!resp.getSuccess()) {
                throw ErrorClassifier.classify(resp.getError());
            }
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        } catch (JsonProcessingException e) {
            throw new AstraeaException("JSON processing error", e);
        }
    }

    @Override
    public void deleteEdge(long id) throws AstraeaException {
        ensureConnected();
        try {
            MutationResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .deleteEdge(DeleteEdgeRequest.newBuilder().setId(id).build());
            if (!resp.getSuccess()) {
                throw ErrorClassifier.classify(resp.getError());
            }
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        }
    }

    // -----------------------------------------------------------------------
    // Traversal
    // -----------------------------------------------------------------------

    @Override
    public List<NeighborEntry> neighbors(long id, NeighborOptions options) throws AstraeaException {
        ensureConnected();
        try {
            var reqBuilder = NeighborsRequest.newBuilder()
                    .setId(id)
                    .setDirection(options.direction() != null ? options.direction() : "outgoing");
            if (options.edgeType() != null) {
                reqBuilder.setEdgeType(options.edgeType());
            }
            NeighborsResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .neighbors(reqBuilder.build());
            if (!resp.getError().isEmpty()) {
                throw ErrorClassifier.classify(resp.getError());
            }
            List<NeighborEntry> entries = new ArrayList<>(resp.getNeighborsCount());
            for (com.astraeadb.grpc.proto.NeighborEntry e : resp.getNeighborsList()) {
                entries.add(new NeighborEntry(e.getEdgeId(), e.getNodeId()));
            }
            return entries;
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        }
    }

    @Override
    public List<BfsEntry> bfs(long start, int maxDepth) throws AstraeaException {
        ensureConnected();
        try {
            BfsResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .bfs(BfsRequest.newBuilder()
                            .setStart(start)
                            .setMaxDepth(maxDepth)
                            .build());
            if (!resp.getError().isEmpty()) {
                throw ErrorClassifier.classify(resp.getError());
            }
            List<BfsEntry> entries = new ArrayList<>(resp.getNodesCount());
            for (com.astraeadb.grpc.proto.BfsEntry e : resp.getNodesList()) {
                entries.add(new BfsEntry(e.getNodeId(), e.getDepth()));
            }
            return entries;
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        }
    }

    @Override
    public PathResult shortestPath(long from, long to, boolean weighted) throws AstraeaException {
        ensureConnected();
        try {
            ShortestPathResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .shortestPath(ShortestPathRequest.newBuilder()
                            .setFrom(from)
                            .setTo(to)
                            .setWeighted(weighted)
                            .build());
            if (!resp.getError().isEmpty()) {
                throw ErrorClassifier.classify(resp.getError());
            }
            List<Long> path = new ArrayList<>(resp.getPathCount());
            for (long nodeId : resp.getPathList()) {
                path.add(nodeId);
            }
            Double cost = resp.hasCost() ? resp.getCost().getValue() : null;
            return new PathResult(resp.getFound(), path, resp.getLength(), cost);
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        }
    }

    // -----------------------------------------------------------------------
    // Vector search
    // -----------------------------------------------------------------------

    @Override
    public List<SearchResult> vectorSearch(float[] query, int k) throws AstraeaException {
        ensureConnected();
        try {
            var reqBuilder = VectorSearchRequest.newBuilder().setK(k);
            if (query != null) {
                for (float v : query) {
                    reqBuilder.addQuery(v);
                }
            }
            VectorSearchResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .vectorSearch(reqBuilder.build());
            if (!resp.getError().isEmpty()) {
                throw ErrorClassifier.classify(resp.getError());
            }
            List<SearchResult> results = new ArrayList<>(resp.getResultsCount());
            for (VectorSearchResult r : resp.getResultsList()) {
                results.add(new SearchResult(r.getNodeId(), r.getScore(), r.getScore()));
            }
            return results;
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        }
    }

    // -----------------------------------------------------------------------
    // GQL query
    // -----------------------------------------------------------------------

    @Override
    public QueryResult query(String gql) throws AstraeaException {
        ensureConnected();
        try {
            QueryResponse resp = stub
                    .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
                    .query(QueryRequest.newBuilder().setGql(gql).build());
            if (!resp.getSuccess()) {
                throw ErrorClassifier.classify(resp.getError());
            }
            // Parse result_json into QueryResult
            if (resp.getResultJson().isEmpty()) {
                return new QueryResult(List.of(), List.of(), QueryResult.QueryStats.EMPTY);
            }
            JsonNode root = mapper.readTree(resp.getResultJson());
            List<String> columns = new ArrayList<>();
            if (root.has("columns")) {
                for (JsonNode col : root.get("columns")) {
                    columns.add(col.asText());
                }
            }
            List<List<Object>> rows = new ArrayList<>();
            if (root.has("rows")) {
                for (JsonNode row : root.get("rows")) {
                    List<Object> rowList = new ArrayList<>();
                    for (JsonNode cell : row) {
                        rowList.add(mapper.treeToValue(cell, Object.class));
                    }
                    rows.add(rowList);
                }
            }
            QueryResult.QueryStats stats = QueryResult.QueryStats.EMPTY;
            if (root.has("stats")) {
                JsonNode s = root.get("stats");
                stats = new QueryResult.QueryStats(
                        s.path("nodes_created").asLong(0),
                        s.path("edges_created").asLong(0),
                        s.path("nodes_deleted").asLong(0),
                        s.path("edges_deleted").asLong(0));
            }
            return new QueryResult(columns, rows, stats);
        } catch (StatusRuntimeException e) {
            throw translateGrpcError(e);
        } catch (AstraeaException e) {
            throw e;
        } catch (IOException e) {
            throw new AstraeaException("JSON processing error", e);
        }
    }

    // -----------------------------------------------------------------------
    // Temporal queries (unsupported over gRPC)
    // -----------------------------------------------------------------------

    @Override
    public List<NeighborEntry> neighborsAt(long id, String direction, long timestamp)
            throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<NeighborEntry> neighborsAt(long id, String direction, long timestamp, String edgeType)
            throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<BfsEntry> bfsAt(long start, int maxDepth, long timestamp) throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public PathResult shortestPathAt(long from, long to, long timestamp, boolean weighted)
            throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // -----------------------------------------------------------------------
    // Semantic / hybrid search (unsupported over gRPC)
    // -----------------------------------------------------------------------

    @Override
    public List<SearchResult> hybridSearch(long anchor, float[] query, HybridSearchOptions options)
            throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<SearchResult> semanticNeighbors(long id, float[] concept, SemanticOptions options)
            throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<WalkStep> semanticWalk(long start, float[] concept, int maxHops)
            throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // -----------------------------------------------------------------------
    // GraphRAG (unsupported over gRPC)
    // -----------------------------------------------------------------------

    @Override
    public SubgraphResult extractSubgraph(long center, SubgraphOptions options)
            throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public RagResult graphRag(String question, RagOptions options) throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // -----------------------------------------------------------------------
    // Batch operations (unsupported over gRPC)
    // -----------------------------------------------------------------------

    @Override
    public List<Long> createNodes(List<NodeInput> nodes) throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<Long> createEdges(List<EdgeInput> edges) throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public int deleteNodes(List<Long> ids) throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public int deleteEdges(List<Long> ids) throws AstraeaException {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // -----------------------------------------------------------------------
    // Error translation
    // -----------------------------------------------------------------------

    /**
     * Translates a gRPC {@link StatusRuntimeException} into an appropriate
     * {@link AstraeaException} subclass.
     */
    private static AstraeaException translateGrpcError(StatusRuntimeException e) {
        Status status = e.getStatus();
        String desc = status.getDescription();
        return switch (status.getCode()) {
            case NOT_FOUND -> {
                if (desc != null && desc.toLowerCase().contains("edge")) {
                    yield new EdgeNotFoundException(desc);
                }
                yield new NodeNotFoundException(desc != null ? desc : "Resource not found");
            }
            case UNAUTHENTICATED -> new InvalidCredentialsException(
                    desc != null ? desc : "Invalid credentials");
            case PERMISSION_DENIED -> new AccessDeniedException(
                    desc != null ? desc : "Access denied");
            case UNAVAILABLE -> new AstraeaException(
                    "Server unavailable: " + (desc != null ? desc : e.getMessage()), e);
            case DEADLINE_EXCEEDED -> new AstraeaException(
                    "Request timed out: " + (desc != null ? desc : e.getMessage()), e);
            case INVALID_ARGUMENT -> new AstraeaException(
                    "Invalid argument: " + (desc != null ? desc : e.getMessage()), e);
            default -> {
                // If the status description contains a classifiable error, use the classifier
                if (desc != null) {
                    yield ErrorClassifier.classify(desc);
                }
                yield new AstraeaException("gRPC error [" + status.getCode() + "]: " + e.getMessage(), e);
            }
        };
    }
}
