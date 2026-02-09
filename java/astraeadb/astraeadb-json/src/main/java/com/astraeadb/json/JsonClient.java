package com.astraeadb.json;

import com.astraeadb.AstraeaClient;
import com.astraeadb.exception.AstraeaException;
import com.astraeadb.exception.ErrorClassifier;
import com.astraeadb.exception.NotConnectedException;
import com.astraeadb.model.*;
import com.astraeadb.options.*;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;

import javax.net.ssl.SSLContext;
import javax.net.ssl.SSLSocketFactory;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.time.Duration;
import java.util.*;
import java.util.concurrent.locks.ReentrantLock;

/**
 * JSON/TCP implementation of {@link AstraeaClient}.
 * Communicates with AstraeaDB over NDJSON (newline-delimited JSON) on a TCP socket.
 *
 * <p>Thread-safe: all wire I/O is protected by a {@link ReentrantLock}.
 *
 * <p>Construct via {@link JsonClientBuilder}:
 * <pre>{@code
 * try (AstraeaClient client = new JsonClientBuilder()
 *         .host("127.0.0.1").port(7687)
 *         .build()) {
 *     client.connect();
 *     PingResponse pong = client.ping();
 * }
 * }</pre>
 */
public final class JsonClient implements AstraeaClient {

    private final String host;
    private final int port;
    private final String authToken;
    private final Duration timeout;
    private final Duration connectTimeout;
    private final int maxRetries;
    private final boolean reconnect;
    private final SSLContext sslContext;

    private final ObjectMapper mapper = new ObjectMapper();
    private final ReentrantLock lock = new ReentrantLock();

    private volatile Socket socket;
    private volatile NdjsonCodec codec;
    private volatile boolean connected;

    JsonClient(String host, int port, String authToken, Duration timeout,
               Duration connectTimeout, int maxRetries, boolean reconnect,
               SSLContext sslContext) {
        this.host = host;
        this.port = port;
        this.authToken = authToken;
        this.timeout = timeout;
        this.connectTimeout = connectTimeout;
        this.maxRetries = maxRetries;
        this.reconnect = reconnect;
        this.sslContext = sslContext;
    }

    // ------------------------------------------------------------------
    // Connection lifecycle
    // ------------------------------------------------------------------

    @Override
    public void connect() throws AstraeaException {
        lock.lock();
        try {
            if (connected) return;
            openSocket();
        } catch (IOException e) {
            throw new AstraeaException("Failed to connect to " + host + ":" + port, e);
        } finally {
            lock.unlock();
        }
    }

    @Override
    public void close() throws AstraeaException {
        lock.lock();
        try {
            if (socket != null && !socket.isClosed()) {
                socket.close();
            }
            connected = false;
            codec = null;
            socket = null;
        } catch (IOException e) {
            throw new AstraeaException("Error closing connection", e);
        } finally {
            lock.unlock();
        }
    }

    // ------------------------------------------------------------------
    // Health
    // ------------------------------------------------------------------

    @Override
    public PingResponse ping() throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "Ping");
        JsonNode data = send(req);
        return new PingResponse(
            data.path("pong").asBoolean(),
            data.path("version").asText()
        );
    }

    // ------------------------------------------------------------------
    // Node CRUD
    // ------------------------------------------------------------------

    @Override
    public long createNode(List<String> labels, Map<String, Object> properties, float[] embedding)
            throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "CreateNode");
        req.set("labels", toArrayNode(labels));
        req.set("properties", mapper.valueToTree(properties != null ? properties : Map.of()));
        if (embedding != null) {
            req.set("embedding", floatArrayToNode(embedding));
        }
        JsonNode data = send(req);
        return data.path("node_id").asLong();
    }

    @Override
    public Node getNode(long id) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "GetNode");
        req.put("id", id);
        JsonNode data = send(req);
        return parseNode(data);
    }

    @Override
    public void updateNode(long id, Map<String, Object> properties) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "UpdateNode");
        req.put("id", id);
        req.set("properties", mapper.valueToTree(properties != null ? properties : Map.of()));
        send(req);
    }

    @Override
    public void deleteNode(long id) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "DeleteNode");
        req.put("id", id);
        send(req);
    }

    // ------------------------------------------------------------------
    // Edge CRUD
    // ------------------------------------------------------------------

    @Override
    public long createEdge(long source, long target, String edgeType, EdgeOptions options)
            throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "CreateEdge");
        req.put("source", source);
        req.put("target", target);
        req.put("edge_type", edgeType);
        req.set("properties", mapper.valueToTree(
            options.properties() != null ? options.properties() : Map.of()));
        req.put("weight", options.weight());
        if (options.validFrom() != null) req.put("valid_from", options.validFrom());
        if (options.validTo() != null) req.put("valid_to", options.validTo());
        JsonNode data = send(req);
        return data.path("edge_id").asLong();
    }

    @Override
    public Edge getEdge(long id) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "GetEdge");
        req.put("id", id);
        JsonNode data = send(req);
        return parseEdge(data);
    }

    @Override
    public void updateEdge(long id, Map<String, Object> properties) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "UpdateEdge");
        req.put("id", id);
        req.set("properties", mapper.valueToTree(properties != null ? properties : Map.of()));
        send(req);
    }

    @Override
    public void deleteEdge(long id) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "DeleteEdge");
        req.put("id", id);
        send(req);
    }

    // ------------------------------------------------------------------
    // Traversal
    // ------------------------------------------------------------------

    @Override
    public List<NeighborEntry> neighbors(long id, NeighborOptions options) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "Neighbors");
        req.put("id", id);
        req.put("direction", options.direction());
        if (options.edgeType() != null) {
            req.put("edge_type", options.edgeType());
        }
        JsonNode data = send(req);
        return parseNeighborList(data.path("neighbors"));
    }

    @Override
    public List<BfsEntry> bfs(long start, int maxDepth) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "Bfs");
        req.put("start", start);
        req.put("max_depth", maxDepth);
        JsonNode data = send(req);
        return parseBfsList(data.path("nodes"));
    }

    @Override
    public PathResult shortestPath(long from, long to, boolean weighted) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "ShortestPath");
        req.put("from", from);
        req.put("to", to);
        req.put("weighted", weighted);
        JsonNode data = send(req);
        return parsePathResult(data);
    }

    // ------------------------------------------------------------------
    // Temporal queries
    // ------------------------------------------------------------------

    @Override
    public List<NeighborEntry> neighborsAt(long id, String direction, long timestamp)
            throws AstraeaException {
        return neighborsAt(id, direction, timestamp, null);
    }

    @Override
    public List<NeighborEntry> neighborsAt(long id, String direction, long timestamp, String edgeType)
            throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "NeighborsAt");
        req.put("id", id);
        req.put("direction", direction);
        req.put("timestamp", timestamp);
        if (edgeType != null) {
            req.put("edge_type", edgeType);
        }
        JsonNode data = send(req);
        return parseNeighborList(data.path("neighbors"));
    }

    @Override
    public List<BfsEntry> bfsAt(long start, int maxDepth, long timestamp) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "BfsAt");
        req.put("start", start);
        req.put("max_depth", maxDepth);
        req.put("timestamp", timestamp);
        JsonNode data = send(req);
        return parseBfsList(data.path("nodes"));
    }

    @Override
    public PathResult shortestPathAt(long from, long to, long timestamp, boolean weighted)
            throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "ShortestPathAt");
        req.put("from", from);
        req.put("to", to);
        req.put("timestamp", timestamp);
        req.put("weighted", weighted);
        JsonNode data = send(req);
        return parsePathResult(data);
    }

    // ------------------------------------------------------------------
    // Vector & semantic search
    // ------------------------------------------------------------------

    @Override
    public List<SearchResult> vectorSearch(float[] query, int k) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "VectorSearch");
        req.set("query", floatArrayToNode(query));
        req.put("k", k);
        JsonNode data = send(req);
        return parseSearchResults(data.path("results"));
    }

    @Override
    public List<SearchResult> hybridSearch(long anchor, float[] query, HybridSearchOptions options)
            throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "HybridSearch");
        req.put("anchor", anchor);
        req.set("query", floatArrayToNode(query));
        req.put("max_hops", options.maxHops());
        req.put("k", options.k());
        req.put("alpha", options.alpha());
        JsonNode data = send(req);
        return parseSearchResults(data.path("results"));
    }

    @Override
    public List<SearchResult> semanticNeighbors(long id, float[] concept, SemanticOptions options)
            throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "SemanticNeighbors");
        req.put("id", id);
        req.set("concept", floatArrayToNode(concept));
        req.put("direction", options.direction());
        req.put("k", options.k());
        JsonNode data = send(req);
        return parseSearchResults(data.path("results"));
    }

    @Override
    public List<WalkStep> semanticWalk(long start, float[] concept, int maxHops)
            throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "SemanticWalk");
        req.put("start", start);
        req.set("concept", floatArrayToNode(concept));
        req.put("max_hops", maxHops);
        JsonNode data = send(req);
        List<WalkStep> steps = new ArrayList<>();
        for (JsonNode step : data.path("steps")) {
            steps.add(new WalkStep(
                step.path("node_id").asLong(),
                step.path("distance").asDouble()
            ));
        }
        return steps;
    }

    // ------------------------------------------------------------------
    // GQL query
    // ------------------------------------------------------------------

    @Override
    public QueryResult query(String gql) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "Query");
        req.put("gql", gql);
        JsonNode data = send(req);
        return parseQueryResult(data);
    }

    // ------------------------------------------------------------------
    // GraphRAG
    // ------------------------------------------------------------------

    @Override
    public SubgraphResult extractSubgraph(long center, SubgraphOptions options) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "ExtractSubgraph");
        req.put("center", center);
        req.put("hops", options.hops());
        req.put("max_nodes", options.maxNodes());
        req.put("format", options.format());
        JsonNode data = send(req);
        return new SubgraphResult(
            data.path("text").asText(),
            data.path("node_count").asInt(),
            data.path("edge_count").asInt(),
            data.path("estimated_tokens").asInt()
        );
    }

    @Override
    public RagResult graphRag(String question, RagOptions options) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "GraphRag");
        req.put("question", question);
        if (options.anchor() != null) req.put("anchor", options.anchor());
        if (options.questionEmbedding() != null) {
            req.set("question_embedding", floatArrayToNode(options.questionEmbedding()));
        }
        req.put("hops", options.hops());
        req.put("max_nodes", options.maxNodes());
        req.put("format", options.format());
        JsonNode data = send(req);
        return new RagResult(
            data.path("anchor_node_id").asLong(),
            data.path("context").asText(),
            data.path("question").asText(),
            data.path("nodes_in_context").asInt(),
            data.path("edges_in_context").asInt(),
            data.path("estimated_tokens").asInt(),
            data.path("note").asText(null)
        );
    }

    // ------------------------------------------------------------------
    // Batch operations
    // ------------------------------------------------------------------

    @Override
    public List<Long> createNodes(List<NodeInput> nodes) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "CreateNodes");
        ArrayNode arr = req.putArray("nodes");
        for (NodeInput ni : nodes) {
            ObjectNode n = mapper.createObjectNode();
            n.set("labels", toArrayNode(ni.labels()));
            n.set("properties", mapper.valueToTree(ni.properties() != null ? ni.properties() : Map.of()));
            if (ni.embedding() != null) {
                n.set("embedding", floatArrayToNode(ni.embedding()));
            }
            arr.add(n);
        }
        JsonNode data = send(req);
        return parseLongList(data.path("node_ids"));
    }

    @Override
    public List<Long> createEdges(List<EdgeInput> edges) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "CreateEdges");
        ArrayNode arr = req.putArray("edges");
        for (EdgeInput ei : edges) {
            ObjectNode e = mapper.createObjectNode();
            e.put("source", ei.source());
            e.put("target", ei.target());
            e.put("edge_type", ei.edgeType());
            e.set("properties", mapper.valueToTree(ei.properties() != null ? ei.properties() : Map.of()));
            e.put("weight", ei.weight());
            if (ei.validFrom() != null) e.put("valid_from", ei.validFrom());
            if (ei.validTo() != null) e.put("valid_to", ei.validTo());
            arr.add(e);
        }
        JsonNode data = send(req);
        return parseLongList(data.path("edge_ids"));
    }

    @Override
    public int deleteNodes(List<Long> ids) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "DeleteNodes");
        ArrayNode arr = req.putArray("ids");
        for (long id : ids) arr.add(id);
        JsonNode data = send(req);
        return data.path("deleted").asInt();
    }

    @Override
    public int deleteEdges(List<Long> ids) throws AstraeaException {
        ObjectNode req = mapper.createObjectNode();
        req.put("type", "DeleteEdges");
        ArrayNode arr = req.putArray("ids");
        for (long id : ids) arr.add(id);
        JsonNode data = send(req);
        return data.path("deleted").asInt();
    }

    // ------------------------------------------------------------------
    // Wire I/O (lock-protected)
    // ------------------------------------------------------------------

    private JsonNode send(ObjectNode request) throws AstraeaException {
        lock.lock();
        try {
            ensureConnected();
            if (authToken != null) {
                request.put("auth_token", authToken);
            }
            codec.send(request);
            JsonNode response = codec.receive();
            String status = response.path("status").asText();
            if ("error".equals(status)) {
                String message = response.path("message").asText("Unknown server error");
                throw ErrorClassifier.classify(message);
            }
            return response.path("data");
        } catch (AstraeaException ae) {
            throw ae;
        } catch (IOException e) {
            connected = false;
            throw new AstraeaException("I/O error during request", e);
        } finally {
            lock.unlock();
        }
    }

    private void ensureConnected() throws AstraeaException {
        if (!connected || socket == null || socket.isClosed()) {
            if (reconnect) {
                try {
                    openSocket();
                } catch (IOException e) {
                    throw new NotConnectedException();
                }
            } else {
                throw new NotConnectedException();
            }
        }
    }

    private void openSocket() throws IOException {
        Socket s;
        if (sslContext != null) {
            SSLSocketFactory factory = sslContext.getSocketFactory();
            s = factory.createSocket();
        } else {
            s = new Socket();
        }
        s.connect(new InetSocketAddress(host, port), (int) connectTimeout.toMillis());
        s.setSoTimeout((int) timeout.toMillis());
        s.setTcpNoDelay(true);
        s.setKeepAlive(true);
        this.socket = s;
        this.codec = new NdjsonCodec(s, mapper);
        this.connected = true;
    }

    // ------------------------------------------------------------------
    // JSON → Model parsing helpers
    // ------------------------------------------------------------------

    private Node parseNode(JsonNode data) {
        List<String> labels = new ArrayList<>();
        for (JsonNode l : data.path("labels")) {
            labels.add(l.asText());
        }
        @SuppressWarnings("unchecked")
        Map<String, Object> props = data.has("properties")
            ? mapper.convertValue(data.path("properties"), Map.class)
            : Map.of();
        return new Node(
            data.path("id").asLong(),
            labels,
            props,
            data.path("has_embedding").asBoolean()
        );
    }

    private Edge parseEdge(JsonNode data) {
        @SuppressWarnings("unchecked")
        Map<String, Object> props = data.has("properties")
            ? mapper.convertValue(data.path("properties"), Map.class)
            : Map.of();
        return new Edge(
            data.path("id").asLong(),
            data.path("source").asLong(),
            data.path("target").asLong(),
            data.path("edge_type").asText(),
            props,
            data.path("weight").asDouble(1.0),
            data.has("valid_from") && !data.path("valid_from").isNull()
                ? data.path("valid_from").asLong() : null,
            data.has("valid_to") && !data.path("valid_to").isNull()
                ? data.path("valid_to").asLong() : null
        );
    }

    private List<NeighborEntry> parseNeighborList(JsonNode arr) {
        List<NeighborEntry> list = new ArrayList<>();
        for (JsonNode n : arr) {
            list.add(new NeighborEntry(
                n.path("edge_id").asLong(),
                n.path("node_id").asLong()
            ));
        }
        return list;
    }

    private List<BfsEntry> parseBfsList(JsonNode arr) {
        List<BfsEntry> list = new ArrayList<>();
        for (JsonNode n : arr) {
            list.add(new BfsEntry(
                n.path("node_id").asLong(),
                n.path("depth").asInt()
            ));
        }
        return list;
    }

    private PathResult parsePathResult(JsonNode data) {
        List<Long> path = new ArrayList<>();
        for (JsonNode n : data.path("path")) {
            path.add(n.asLong());
        }
        Double cost = data.has("cost") && !data.path("cost").isNull()
            ? data.path("cost").asDouble() : null;
        return new PathResult(
            data.path("found").asBoolean(),
            path,
            data.path("length").asInt(),
            cost
        );
    }

    private List<SearchResult> parseSearchResults(JsonNode arr) {
        List<SearchResult> list = new ArrayList<>();
        for (JsonNode n : arr) {
            list.add(new SearchResult(
                n.path("node_id").asLong(),
                n.path("distance").asDouble(),
                n.path("score").asDouble()
            ));
        }
        return list;
    }

    private QueryResult parseQueryResult(JsonNode data) {
        List<String> columns = new ArrayList<>();
        for (JsonNode c : data.path("columns")) {
            columns.add(c.asText());
        }
        List<List<Object>> rows = new ArrayList<>();
        for (JsonNode row : data.path("rows")) {
            List<Object> r = new ArrayList<>();
            for (JsonNode cell : row) {
                r.add(nodeToObject(cell));
            }
            rows.add(r);
        }
        JsonNode stats = data.path("stats");
        QueryResult.QueryStats qs = stats.isMissingNode()
            ? QueryResult.QueryStats.EMPTY
            : new QueryResult.QueryStats(
                stats.path("nodes_created").asLong(0),
                stats.path("edges_created").asLong(0),
                stats.path("nodes_deleted").asLong(0),
                stats.path("edges_deleted").asLong(0)
            );
        return new QueryResult(columns, rows, qs);
    }

    // ------------------------------------------------------------------
    // Utility helpers
    // ------------------------------------------------------------------

    private ArrayNode toArrayNode(List<String> items) {
        ArrayNode arr = mapper.createArrayNode();
        if (items != null) {
            for (String s : items) arr.add(s);
        }
        return arr;
    }

    private ArrayNode floatArrayToNode(float[] values) {
        ArrayNode arr = mapper.createArrayNode();
        for (float v : values) arr.add(v);
        return arr;
    }

    private List<Long> parseLongList(JsonNode arr) {
        List<Long> list = new ArrayList<>();
        for (JsonNode n : arr) list.add(n.asLong());
        return list;
    }

    private Object nodeToObject(JsonNode node) {
        if (node.isNull()) return null;
        if (node.isTextual()) return node.asText();
        if (node.isInt()) return node.asInt();
        if (node.isLong()) return node.asLong();
        if (node.isDouble() || node.isFloat()) return node.asDouble();
        if (node.isBoolean()) return node.asBoolean();
        return node.toString();
    }
}
