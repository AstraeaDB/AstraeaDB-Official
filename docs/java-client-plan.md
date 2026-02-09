# AstraeaDB Java Client — Implementation Plan

## Overview

Build a full-featured Java client library for AstraeaDB with feature parity to the existing Python, R, and Go clients. The client will support all three transport protocols (JSON/TCP, gRPC, Arrow Flight), provide idiomatic Java APIs with Builder patterns, `AutoCloseable` lifecycle, and `CompletableFuture` async support, and include comprehensive tests.

**Group ID:** `com.astraeadb`
**Artifact ID:** `astraeadb-java`
**Java Version:** 17+
**License:** MIT

---

## 1. Project Structure (Gradle Multi-Module)

```
java/astraeadb/
├── build.gradle.kts                    # Root build (dependency versions, plugins)
├── settings.gradle.kts                 # Module declarations
├── gradle.properties                   # Version catalog
├── gradlew / gradlew.bat
│
├── astraeadb-api/                      # Core API module (zero external deps)
│   ├── build.gradle.kts
│   └── src/
│       ├── main/java/com/astraeadb/
│       │   ├── AstraeaClient.java      # Public interface (all 22 operations)
│       │   ├── AstraeaClientBuilder.java  # Builder for unified client
│       │   ├── model/                  # Domain types
│       │   │   ├── Node.java
│       │   │   ├── Edge.java
│       │   │   ├── NeighborEntry.java
│       │   │   ├── BfsEntry.java
│       │   │   ├── PathResult.java
│       │   │   ├── SearchResult.java
│       │   │   ├── WalkStep.java
│       │   │   ├── QueryResult.java
│       │   │   ├── SubgraphResult.java
│       │   │   ├── RagResult.java
│       │   │   ├── PingResponse.java
│       │   │   ├── NodeInput.java
│       │   │   └── EdgeInput.java
│       │   ├── options/                # Per-operation option classes
│       │   │   ├── EdgeOptions.java
│       │   │   ├── NeighborOptions.java
│       │   │   ├── HybridSearchOptions.java
│       │   │   ├── SemanticOptions.java
│       │   │   ├── SubgraphOptions.java
│       │   │   └── RagOptions.java
│       │   └── exception/              # Exception hierarchy
│       │       ├── AstraeaException.java
│       │       ├── NodeNotFoundException.java
│       │       ├── EdgeNotFoundException.java
│       │       ├── VectorIndexNotConfiguredException.java
│       │       ├── AccessDeniedException.java
│       │       ├── InvalidCredentialsException.java
│       │       ├── AuthRequiredException.java
│       │       └── NotConnectedException.java
│       └── test/java/com/astraeadb/
│           └── model/
│               └── ModelSerializationTest.java
│
├── astraeadb-json/                     # JSON/TCP transport (depends on: api, jackson)
│   ├── build.gradle.kts
│   └── src/
│       ├── main/java/com/astraeadb/json/
│       │   ├── JsonClient.java         # Full 22-operation JSON/TCP client
│       │   ├── JsonClientBuilder.java  # Builder with host, port, TLS, auth, timeouts
│       │   ├── NdjsonCodec.java        # NDJSON read/write over Socket
│       │   └── ExponentialBackoff.java # Reconnection backoff with jitter
│       └── test/java/com/astraeadb/json/
│           ├── JsonClientTest.java     # 25+ tests with mock server
│           ├── NdjsonCodecTest.java    # Wire protocol tests
│           └── BackoffTest.java        # Backoff algorithm tests
│
├── astraeadb-grpc/                     # gRPC transport (depends on: api, grpc-java)
│   ├── build.gradle.kts
│   └── src/
│       ├── main/
│       │   ├── java/com/astraeadb/grpc/
│       │   │   ├── GrpcClient.java     # 14-operation gRPC client
│       │   │   └── GrpcClientBuilder.java
│       │   └── proto/
│       │       └── astraea.proto       # Copied from server repo
│       └── test/java/com/astraeadb/grpc/
│           └── GrpcClientTest.java     # InProcessServer tests
│
├── astraeadb-flight/                   # Arrow Flight transport (depends on: api, arrow-flight)
│   ├── build.gradle.kts
│   └── src/
│       ├── main/java/com/astraeadb/flight/
│       │   ├── FlightClient.java       # Query + bulk import via Arrow Flight
│       │   └── FlightClientBuilder.java
│       └── test/java/com/astraeadb/flight/
│           └── FlightClientTest.java   # In-process Flight server tests
│
├── astraeadb-unified/                  # Unified client (depends on: api, json, grpc, flight)
│   ├── build.gradle.kts
│   └── src/
│       ├── main/java/com/astraeadb/
│       │   └── UnifiedClient.java      # Auto-selects transport per operation
│       └── test/java/com/astraeadb/
│           └── UnifiedClientTest.java  # Transport routing + fallback tests
│
└── examples/                           # Example programs
    ├── build.gradle.kts
    └── src/main/java/com/astraeadb/examples/
        ├── BasicExample.java           # CRUD + traversal demo
        ├── VectorSearchExample.java    # Vector + hybrid search demo
        ├── GraphRagExample.java        # GraphRAG pipeline demo
        └── CybersecurityExample.java   # Threat investigation demo
```

### Dependencies (Version Catalog)

```kotlin
// gradle.properties
javaVersion = 17
jacksonVersion = 2.18.3
grpcVersion = 1.72.0
protocVersion = 28.3
arrowVersion = 18.1.0
junitVersion = 5.11.4
```

```kotlin
// root build.gradle.kts
plugins {
    java
    id("com.google.protobuf") version "0.9.4" apply false
}

subprojects {
    apply(plugin = "java-library")
    java { toolchain { languageVersion.set(JavaLanguageVersion.of(17)) } }
    repositories { mavenCentral() }
    dependencies {
        testImplementation("org.junit.jupiter:junit-jupiter:${property("junitVersion")}")
        testImplementation("org.assertj:assertj-core:3.27.3")
    }
    tasks.test { useJUnitPlatform() }
}
```

| Module | External Dependencies |
|---|---|
| `astraeadb-api` | None (pure Java 17) |
| `astraeadb-json` | `com.fasterxml.jackson.core:jackson-databind` |
| `astraeadb-grpc` | `io.grpc:grpc-netty-shaded`, `io.grpc:grpc-protobuf`, `io.grpc:grpc-stub`, `com.google.protobuf:protobuf-java` |
| `astraeadb-flight` | `org.apache.arrow:flight-core`, `org.apache.arrow:arrow-vector`, `org.apache.arrow:arrow-memory-netty` |
| `astraeadb-unified` | All above (transitive) |

---

## 2. Public API Design

### 2.1 Builder Pattern (Java Idiom)

```java
// AstraeaClientBuilder.java — replaces Go's functional options
public class AstraeaClientBuilder {
    private String host = "127.0.0.1";
    private int jsonPort = 7687;
    private int grpcPort = 7688;
    private int flightPort = 7689;
    private String authToken;
    private Duration timeout = Duration.ofSeconds(10);
    private Duration connectTimeout = Duration.ofSeconds(5);
    private SSLContext sslContext;
    private int maxRetries = 3;
    private boolean reconnect = true;

    public AstraeaClientBuilder host(String host) { ... }
    public AstraeaClientBuilder jsonPort(int port) { ... }
    public AstraeaClientBuilder grpcPort(int port) { ... }
    public AstraeaClientBuilder flightPort(int port) { ... }
    public AstraeaClientBuilder authToken(String token) { ... }
    public AstraeaClientBuilder timeout(Duration timeout) { ... }
    public AstraeaClientBuilder connectTimeout(Duration timeout) { ... }
    public AstraeaClientBuilder ssl(SSLContext ctx) { ... }
    public AstraeaClientBuilder mtls(Path certPath, Path keyPath, Path caPath) { ... }
    public AstraeaClientBuilder maxRetries(int n) { ... }
    public AstraeaClientBuilder reconnect(boolean enabled) { ... }

    public JsonClient buildJson() { ... }
    public GrpcClient buildGrpc() { ... }
    public FlightClient buildFlight() { ... }
    public UnifiedClient build() { ... }  // auto-selects transport
}
```

### 2.2 Client Interface

All four client implementations implement this interface, mirroring the Python/R/Go client API:

```java
// AstraeaClient.java
public interface AstraeaClient extends AutoCloseable {

    // Connection lifecycle
    void connect() throws AstraeaException;

    @Override
    void close() throws AstraeaException;

    // Health
    PingResponse ping() throws AstraeaException;

    // Node CRUD
    long createNode(List<String> labels, Map<String, Object> properties,
                    float[] embedding) throws AstraeaException;
    Node getNode(long id) throws AstraeaException;
    void updateNode(long id, Map<String, Object> properties) throws AstraeaException;
    void deleteNode(long id) throws AstraeaException;

    // Edge CRUD
    long createEdge(long source, long target, String edgeType,
                    EdgeOptions options) throws AstraeaException;
    Edge getEdge(long id) throws AstraeaException;
    void updateEdge(long id, Map<String, Object> properties) throws AstraeaException;
    void deleteEdge(long id) throws AstraeaException;

    // Traversal
    List<NeighborEntry> neighbors(long id,
                                  NeighborOptions options) throws AstraeaException;
    List<BfsEntry> bfs(long start, int maxDepth) throws AstraeaException;
    PathResult shortestPath(long from, long to,
                            boolean weighted) throws AstraeaException;

    // Temporal queries
    List<NeighborEntry> neighborsAt(long id, String direction,
                                    long timestamp) throws AstraeaException;
    List<BfsEntry> bfsAt(long start, int maxDepth,
                         long timestamp) throws AstraeaException;
    PathResult shortestPathAt(long from, long to, long timestamp,
                              boolean weighted) throws AstraeaException;

    // Vector & semantic search
    List<SearchResult> vectorSearch(float[] query, int k) throws AstraeaException;
    List<SearchResult> hybridSearch(long anchor, float[] query,
                                    HybridSearchOptions options) throws AstraeaException;
    List<SearchResult> semanticNeighbors(long id, float[] concept,
                                         SemanticOptions options) throws AstraeaException;
    List<WalkStep> semanticWalk(long start, float[] concept,
                                int maxHops) throws AstraeaException;

    // GQL query
    QueryResult query(String gql) throws AstraeaException;

    // GraphRAG
    SubgraphResult extractSubgraph(long center,
                                    SubgraphOptions options) throws AstraeaException;
    RagResult graphRag(String question,
                       RagOptions options) throws AstraeaException;

    // Batch operations
    List<Long> createNodes(List<NodeInput> nodes) throws AstraeaException;
    List<Long> createEdges(List<EdgeInput> edges) throws AstraeaException;
    int deleteNodes(List<Long> ids) throws AstraeaException;
    int deleteEdges(List<Long> ids) throws AstraeaException;
}
```

### 2.3 Convenience Overloads

```java
// Common shorthands with default parameters
long createNode(List<String> labels, Map<String, Object> properties);
long createNode(List<String> labels);
long createEdge(long source, long target, String edgeType);
long createEdge(long source, long target, String edgeType, double weight);
List<NeighborEntry> neighbors(long id);
List<SearchResult> hybridSearch(long anchor, float[] query);
SubgraphResult extractSubgraph(long center);
RagResult graphRag(String question);
```

### 2.4 Constructor Examples

```java
// JSON/TCP only (minimal dependencies)
try (var client = new JsonClientBuilder()
        .host("127.0.0.1").port(7687)
        .authToken("my-key")
        .build()) {
    client.connect();
    long id = client.createNode(List.of("Person"), Map.of("name", "Alice"));
}

// gRPC only
try (var client = new GrpcClientBuilder()
        .host("127.0.0.1").port(7688)
        .build()) {
    client.connect();
    Node node = client.getNode(42);
}

// Unified client (auto-selects best transport)
try (var client = AstraeaClient.builder()
        .host("127.0.0.1")
        .authToken("my-key")
        .build()) {
    client.connect();
    var results = client.vectorSearch(new float[]{0.1f, 0.2f, 0.3f}, 5);
}
```

---

## 3. Type Definitions

### 3.1 Core Domain Types (Java Records)

```java
// model/Node.java
public record Node(
    long id,
    List<String> labels,
    Map<String, Object> properties,
    boolean hasEmbedding
) {}

// model/Edge.java
public record Edge(
    long id,
    long source,
    long target,
    String edgeType,
    Map<String, Object> properties,
    double weight,
    Long validFrom,   // nullable — null means "not temporal"
    Long validTo      // nullable
) {}

// model/NeighborEntry.java
public record NeighborEntry(long edgeId, long nodeId) {}

// model/BfsEntry.java
public record BfsEntry(long nodeId, int depth) {}

// model/PathResult.java
public record PathResult(
    boolean found,
    List<Long> path,
    int length,
    Double cost       // nullable — absent for unweighted
) {}

// model/SearchResult.java
public record SearchResult(long nodeId, double distance, double score) {}

// model/WalkStep.java
public record WalkStep(long nodeId, double distance) {}

// model/QueryResult.java
public record QueryResult(
    List<String> columns,
    List<List<Object>> rows,
    QueryStats stats
) {
    public record QueryStats(
        long nodesCreated, long edgesCreated,
        long nodesDeleted, long edgesDeleted
    ) {}
}

// model/SubgraphResult.java
public record SubgraphResult(
    String text, int nodeCount, int edgeCount, int estimatedTokens
) {}

// model/RagResult.java
public record RagResult(
    long anchorNodeId,
    String context,
    String question,
    int nodesInContext,
    int edgesInContext,
    int estimatedTokens,
    String note
) {}

// model/PingResponse.java
public record PingResponse(boolean pong, String version) {}
```

### 3.2 Batch Input Types

```java
// model/NodeInput.java
public record NodeInput(
    List<String> labels,
    Map<String, Object> properties,
    float[] embedding               // nullable
) {
    public NodeInput(List<String> labels, Map<String, Object> properties) {
        this(labels, properties, null);
    }
}

// model/EdgeInput.java
public record EdgeInput(
    long source,
    long target,
    String edgeType,
    Map<String, Object> properties,
    double weight,
    Long validFrom,                 // nullable
    Long validTo                    // nullable
) {
    public EdgeInput(long source, long target, String edgeType) {
        this(source, target, edgeType, Map.of(), 1.0, null, null);
    }
}
```

### 3.3 Per-Operation Options (Builder Pattern)

```java
// options/EdgeOptions.java
public record EdgeOptions(
    Map<String, Object> properties,
    double weight,
    Long validFrom,
    Long validTo
) {
    public static Builder builder() { return new Builder(); }

    public static class Builder {
        private Map<String, Object> properties = Map.of();
        private double weight = 1.0;
        private Long validFrom, validTo;

        public Builder properties(Map<String, Object> p) { properties = p; return this; }
        public Builder weight(double w) { weight = w; return this; }
        public Builder validFrom(long t) { validFrom = t; return this; }
        public Builder validTo(long t) { validTo = t; return this; }
        public EdgeOptions build() { return new EdgeOptions(properties, weight, validFrom, validTo); }
    }
}

// options/NeighborOptions.java
public record NeighborOptions(String direction, String edgeType) {
    public static final NeighborOptions DEFAULT =
        new NeighborOptions("outgoing", null);

    public static Builder builder() { return new Builder(); }

    public static class Builder {
        private String direction = "outgoing";
        private String edgeType;

        public Builder direction(String d) { direction = d; return this; }
        public Builder edgeType(String t) { edgeType = t; return this; }
        public NeighborOptions build() { return new NeighborOptions(direction, edgeType); }
    }
}

// options/HybridSearchOptions.java
public record HybridSearchOptions(int maxHops, int k, double alpha) {
    public static final HybridSearchOptions DEFAULT =
        new HybridSearchOptions(3, 10, 0.5);

    public static Builder builder() { return new Builder(); }

    public static class Builder {
        private int maxHops = 3, k = 10;
        private double alpha = 0.5;

        public Builder maxHops(int h) { maxHops = h; return this; }
        public Builder k(int k) { this.k = k; return this; }
        public Builder alpha(double a) { alpha = a; return this; }
        public HybridSearchOptions build() { return new HybridSearchOptions(maxHops, k, alpha); }
    }
}

// options/SemanticOptions.java
public record SemanticOptions(String direction, int k) {
    public static final SemanticOptions DEFAULT =
        new SemanticOptions("outgoing", 10);
}

// options/SubgraphOptions.java
public record SubgraphOptions(int hops, int maxNodes, String format) {
    public static final SubgraphOptions DEFAULT =
        new SubgraphOptions(3, 50, "structured");
}

// options/RagOptions.java
public record RagOptions(
    Long anchor,             // nullable
    float[] questionEmbedding, // nullable
    int hops,
    int maxNodes,
    String format
) {
    public static final RagOptions DEFAULT =
        new RagOptions(null, null, 3, 50, "structured");
}
```

### 3.4 Wire Protocol Types (Internal)

```java
// json/internal — Jackson-annotated request/response envelopes

// JSON-TCP request (constructed as ObjectNode for flexibility)
// { "type": "CreateNode", "labels": [...], "properties": {...}, "auth_token": "..." }

// JSON-TCP response
record JsonResponse(
    @JsonProperty("status") String status,
    @JsonProperty("data") JsonNode data,
    @JsonProperty("message") String message
) {}
```

### 3.5 Request-Specific Field Maps

Each of the 22 request types maps to specific JSON fields (same as Go/Python):

| Request Type | Key Fields |
|---|---|
| `Ping` | *(none)* |
| `CreateNode` | `labels: List<String>`, `properties: Map`, `embedding: float[]` |
| `GetNode` | `id: long` |
| `UpdateNode` | `id: long`, `properties: Map` |
| `DeleteNode` | `id: long` |
| `CreateEdge` | `source`, `target: long`, `edge_type: String`, `properties: Map`, `weight: double`, `valid_from: Long?`, `valid_to: Long?` |
| `GetEdge` | `id: long` |
| `UpdateEdge` | `id: long`, `properties: Map` |
| `DeleteEdge` | `id: long` |
| `Neighbors` | `id: long`, `direction: String`, `edge_type: String?` |
| `Bfs` | `start: long`, `max_depth: int` |
| `ShortestPath` | `from`, `to: long`, `weighted: boolean` |
| `VectorSearch` | `query: float[]`, `k: int` |
| `HybridSearch` | `anchor: long`, `query: float[]`, `max_hops: int`, `k: int`, `alpha: double` |
| `SemanticNeighbors` | `id: long`, `concept: float[]`, `direction: String`, `k: int` |
| `SemanticWalk` | `start: long`, `concept: float[]`, `max_hops: int` |
| `Query` | `gql: String` |
| `ExtractSubgraph` | `center: long`, `hops: int`, `max_nodes: int`, `format: String` |
| `GraphRag` | `question: String`, `anchor: Long?`, `question_embedding: float[]?`, `hops: int`, `max_nodes: int`, `format: String` |
| `NeighborsAt` | `id: long`, `direction: String`, `timestamp: long`, `edge_type: String?` |
| `BfsAt` | `start: long`, `max_depth: int`, `timestamp: long` |
| `ShortestPathAt` | `from`, `to: long`, `timestamp: long`, `weighted: boolean` |

---

## 4. Transport Implementations

### 4.1 JSON/TCP Client (JsonClient.java)

**Protocol:** Newline-delimited JSON over TCP (port 7687)

**Key implementation details:**
- `java.net.Socket` with `SocketChannel` for non-blocking awareness
- `BufferedReader.readLine()` + Jackson `ObjectMapper.readTree()` for response reading
- `OutputStream.write(bytes)` + `flush()` for request writing (NDJSON: one JSON object per line)
- `ReentrantLock` to protect the shared `Socket`
- `Socket.setSoTimeout()` for read deadline (bridged from builder timeout)
- `Socket.setTcpNoDelay(true)` for low-latency request-response
- `Socket.setKeepAlive(true)` for connection health
- Exponential backoff with ±25% jitter for reconnection
- 1 MB max line buffer (matching server limit)

**Core send/receive loop:**
```java
public class JsonClient implements AstraeaClient {
    private final ReentrantLock lock = new ReentrantLock();
    private Socket socket;
    private BufferedReader reader;
    private OutputStream writer;
    private final ObjectMapper mapper = new ObjectMapper();

    private JsonNode send(ObjectNode request) throws AstraeaException {
        lock.lock();
        try {
            if (socket == null || socket.isClosed()) {
                throw new NotConnectedException();
            }

            // Inject auth token
            if (authToken != null) {
                request.put("auth_token", authToken);
            }

            // Write NDJSON request
            byte[] bytes = mapper.writeValueAsBytes(request);
            writer.write(bytes);
            writer.write('\n');
            writer.flush();

            // Read NDJSON response
            String line = reader.readLine();
            if (line == null) {
                closeQuietly();
                throw new AstraeaException("Connection closed by server");
            }

            JsonNode resp = mapper.readTree(line);
            if ("error".equals(resp.path("status").asText())) {
                throw classifyError(resp.path("message").asText());
            }
            return resp.path("data");
        } catch (IOException e) {
            closeQuietly();
            throw new AstraeaException("I/O error: " + e.getMessage(), e);
        } finally {
            lock.unlock();
        }
    }
}
```

**TLS support:**
```java
// Plain TCP
socket = new Socket(host, port);

// TLS
SSLSocketFactory factory = sslContext.getSocketFactory();
socket = factory.createSocket(host, port);

// mTLS
KeyManagerFactory kmf = KeyManagerFactory.getInstance("PKCS12");
kmf.init(keyStore, password);
TrustManagerFactory tmf = TrustManagerFactory.getInstance("X509");
tmf.init(trustStore);
SSLContext ctx = SSLContext.getInstance("TLSv1.3");
ctx.init(kmf.getKeyManagers(), tmf.getTrustManagers(), null);
socket = ctx.getSocketFactory().createSocket(host, port);
```

### 4.2 gRPC Client (GrpcClient.java)

**Protocol:** Protobuf over gRPC (port 7688)

**Key implementation details:**
- `ManagedChannel` via `ManagedChannelBuilder.forAddress()` or `NettyChannelBuilder` (for TLS)
- Generated `AstraeaServiceGrpc.AstraeaServiceBlockingStub` for synchronous calls
- `AstraeaServiceGrpc.AstraeaServiceFutureStub` for `CompletableFuture` async variant
- `CallOptions.withDeadlineAfter()` for per-RPC deadlines
- `StatusRuntimeException` for gRPC error inspection
- Lazy connection (connects on first RPC, not at construction)

**Proto code generation (Gradle):**
```kotlin
// astraeadb-grpc/build.gradle.kts
plugins {
    id("com.google.protobuf")
}

protobuf {
    protoc { artifact = "com.google.protobuf:protoc:${property("protocVersion")}" }
    plugins {
        create("grpc") {
            artifact = "io.grpc:protoc-gen-grpc-java:${property("grpcVersion")}"
        }
    }
    generateProtoTasks {
        all().forEach { task ->
            task.plugins { create("grpc") }
        }
    }
}
```

**Implementation pattern:**
```java
public class GrpcClient implements AstraeaClient {
    private ManagedChannel channel;
    private AstraeaServiceBlockingStub stub;

    @Override
    public long createNode(List<String> labels, Map<String, Object> properties,
                           float[] embedding) throws AstraeaException {
        var reqBuilder = CreateNodeRequest.newBuilder()
            .addAllLabels(labels)
            .setPropertiesJson(mapper.writeValueAsString(properties));
        if (embedding != null) {
            for (float v : embedding) reqBuilder.addEmbedding(v);
        }

        MutationResponse resp = stub
            .withDeadlineAfter(timeout.toMillis(), TimeUnit.MILLISECONDS)
            .createNode(reqBuilder.build());

        if (!resp.getSuccess()) {
            throw classifyError(resp.getError());
        }
        JsonNode result = mapper.readTree(resp.getResultJson());
        return result.path("node_id").asLong();
    }
}
```

**gRPC-specific methods mapping (14 RPCs):**

| Java Method | gRPC RPC | Request | Response |
|---|---|---|---|
| `createNode` | `CreateNode` | `CreateNodeRequest` | `MutationResponse` |
| `getNode` | `GetNode` | `GetNodeRequest` | `GetNodeResponse` |
| `updateNode` | `UpdateNode` | `UpdateNodeRequest` | `MutationResponse` |
| `deleteNode` | `DeleteNode` | `DeleteNodeRequest` | `MutationResponse` |
| `createEdge` | `CreateEdge` | `CreateEdgeRequest` | `MutationResponse` |
| `getEdge` | `GetEdge` | `GetEdgeRequest` | `GetEdgeResponse` |
| `updateEdge` | `UpdateEdge` | `UpdateEdgeRequest` | `MutationResponse` |
| `deleteEdge` | `DeleteEdge` | `DeleteEdgeRequest` | `MutationResponse` |
| `neighbors` | `Neighbors` | `NeighborsRequest` | `NeighborsResponse` |
| `bfs` | `Bfs` | `BfsRequest` | `BfsResponse` |
| `shortestPath` | `ShortestPath` | `ShortestPathRequest` | `ShortestPathResponse` |
| `vectorSearch` | `VectorSearch` | `VectorSearchRequest` | `VectorSearchResponse` |
| `query` | `Query` | `QueryRequest` | `QueryResponse` |
| `ping` | `Ping` | `PingRequest` | `PingResponse` |

**Note:** gRPC proto covers 14 of the 22 request types. The remaining 8 (temporal queries, semantic search, GraphRAG) are only available via JSON/TCP. The UnifiedClient falls back to JSON for these.

### 4.3 Arrow Flight Client (FlightClient.java)

**Protocol:** Apache Arrow Flight over gRPC (port 7689)

**Key implementation details:**
- `FlightClient.builder()` from `org.apache.arrow.flight`
- `getStream(Ticket)` with GQL query as ticket bytes for query execution
- `startPut(FlightDescriptor, VectorSchemaRoot)` for bulk node/edge import
- `VectorSchemaRoot` for zero-copy columnar data
- `BufferAllocator` lifecycle management (root allocator, close on client close)
- Schema detection: "labels" column → nodes, "edge_type" column → edges

**DoGet (Query execution):**
```java
public QueryResult query(String gql) throws AstraeaException {
    var ticket = new Ticket(gql.getBytes(StandardCharsets.UTF_8));
    try (var stream = flightClient.getStream(ticket)) {
        VectorSchemaRoot root = stream.getRoot();
        List<String> columns = root.getSchema().getFields().stream()
            .map(Field::getName).toList();
        List<List<Object>> rows = new ArrayList<>();
        while (stream.next()) {
            for (int i = 0; i < root.getRowCount(); i++) {
                List<Object> row = new ArrayList<>();
                for (FieldVector vec : root.getFieldVectors()) {
                    row.add(vec.getObject(i));
                }
                rows.add(row);
            }
        }
        return new QueryResult(columns, rows, QueryResult.QueryStats.EMPTY);
    }
}
```

**DoPut (Bulk import):**
```java
public List<Long> createNodes(List<NodeInput> nodes) throws AstraeaException {
    Schema schema = new Schema(List.of(
        Field.nullable("id", new ArrowType.Int(64, false)),
        Field.nullable("labels", ArrowType.Utf8.INSTANCE),
        Field.nullable("properties", ArrowType.Utf8.INSTANCE),
        Field.nullable("has_embedding", ArrowType.Bool.INSTANCE)
    ));

    try (VectorSchemaRoot root = VectorSchemaRoot.create(schema, allocator)) {
        root.allocateNew();
        for (int i = 0; i < nodes.size(); i++) {
            NodeInput n = nodes.get(i);
            ((UInt8Vector) root.getVector("id")).set(i, 0);
            ((VarCharVector) root.getVector("labels"))
                .setSafe(i, mapper.writeValueAsBytes(n.labels()));
            ((VarCharVector) root.getVector("properties"))
                .setSafe(i, mapper.writeValueAsBytes(n.properties()));
            ((BitVector) root.getVector("has_embedding"))
                .set(i, n.embedding() != null ? 1 : 0);
        }
        root.setRowCount(nodes.size());

        var descriptor = FlightDescriptor.command("bulk_insert_nodes".getBytes());
        try (var listener = flightClient.startPut(descriptor, root, new AsyncPutListener())) {
            listener.putNext();
            listener.completed();
            listener.getResult(); // blocks until ack
        }
    }
    // ... parse response for IDs ...
}
```

**Arrow schemas (must match server):**

| Schema | Columns | Arrow Types |
|---|---|---|
| Node | `id`, `labels`, `properties`, `has_embedding` | UInt64, Utf8, Utf8, Bool |
| Edge | `id`, `source`, `target`, `edge_type`, `properties`, `weight`, `valid_from`, `valid_to` | UInt64, UInt64, UInt64, Utf8, Utf8, Float64, Int64?, Int64? |
| Query Result | *(dynamic from RETURN clause)* | All nullable Utf8 |

### 4.4 Unified Client (UnifiedClient.java)

Routes operations to the optimal transport:

| Operation Category | Primary Transport | Fallback |
|---|---|---|
| CRUD (Node/Edge) | gRPC | JSON |
| Traversal (Neighbors, BFS, ShortestPath) | gRPC | JSON |
| Query (GQL) | Arrow Flight | JSON |
| Temporal (NeighborsAt, BFSAt, ShortestPathAt) | JSON | — |
| Vector (VectorSearch, HybridSearch) | gRPC (VectorSearch only) | JSON |
| Semantic (SemanticNeighbors, SemanticWalk) | JSON | — |
| GraphRAG (ExtractSubgraph, GraphRag) | JSON | — |
| Bulk Insert | Arrow Flight | JSON (loop) |

**Graceful degradation:** Arrow Flight and gRPC are optional. If unavailable (connection fails or dependencies not on classpath), the client silently falls back to JSON/TCP for all operations.

**Transport probe on connect:**
```java
@Override
public void connect() throws AstraeaException {
    // Always connect JSON (baseline)
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
```

---

## 5. Exception Hierarchy

### 5.1 Exception Types (exception/)

```java
// Base exception
public class AstraeaException extends Exception {
    private final String serverMessage;

    public AstraeaException(String message) {
        super(message);
        this.serverMessage = message;
    }

    public AstraeaException(String message, Throwable cause) {
        super(message, cause);
        this.serverMessage = message;
    }

    public String getServerMessage() { return serverMessage; }
}

// Specific exceptions (all extend AstraeaException)
public class NodeNotFoundException extends AstraeaException { ... }
public class EdgeNotFoundException extends AstraeaException { ... }
public class VectorIndexNotConfiguredException extends AstraeaException { ... }
public class AccessDeniedException extends AstraeaException { ... }
public class InvalidCredentialsException extends AstraeaException { ... }
public class AuthRequiredException extends AstraeaException { ... }
public class NotConnectedException extends AstraeaException { ... }
```

### 5.2 Error Classification (shared utility)

```java
// Maps server error messages to specific exception types
static AstraeaException classifyError(String message) {
    if (message.contains("not found")) {
        if (message.contains("node")) return new NodeNotFoundException(message);
        if (message.contains("edge")) return new EdgeNotFoundException(message);
    }
    if (message.contains("vector index not configured"))
        return new VectorIndexNotConfiguredException(message);
    if (message.contains("access denied"))
        return new AccessDeniedException(message);
    if (message.contains("invalid credentials"))
        return new InvalidCredentialsException(message);
    if (message.contains("authentication required"))
        return new AuthRequiredException(message);
    return new AstraeaException(message);
}
```

### 5.3 gRPC Error Translation

```java
static AstraeaException translateGrpcError(StatusRuntimeException e) {
    Status status = e.getStatus();
    return switch (status.getCode()) {
        case UNAVAILABLE -> new AstraeaException("Server unavailable: " + status.getDescription(), e);
        case DEADLINE_EXCEEDED -> new AstraeaException("Request timed out", e);
        case INVALID_ARGUMENT -> new AstraeaException(status.getDescription(), e);
        case UNAUTHENTICATED -> new InvalidCredentialsException(status.getDescription());
        case PERMISSION_DENIED -> new AccessDeniedException(status.getDescription());
        default -> classifyError(status.getDescription() != null ? status.getDescription() : e.getMessage());
    };
}
```

---

## 6. Authentication & TLS

### 6.1 API Key Auth (JSON/TCP)

Injected as `auth_token` field in every JSON request:
```json
{"type":"CreateNode","labels":["Person"],"properties":{},"auth_token":"my-key"}
```

### 6.2 TLS Configuration

```java
// Server TLS only (verify server cert)
var client = AstraeaClient.builder()
    .host("127.0.0.1")
    .ssl(SSLContext.getDefault())
    .build();

// Mutual TLS (client presents cert)
var client = AstraeaClient.builder()
    .host("127.0.0.1")
    .mtls(Path.of("client-cert.pem"), Path.of("client-key.pem"), Path.of("ca-cert.pem"))
    .build();
```

**CN-to-role mapping (server-side):**
- CN ending in `-admin` → Admin role
- CN ending in `-writer` → Writer role
- All others → Reader role

### 6.3 gRPC Auth

- Bearer token via `CallCredentials` or metadata interceptor
- `authorization: Bearer <token>` in metadata header

```java
// Bearer token interceptor
CallCredentials creds = new CallCredentials() {
    @Override
    public void applyRequestMetadata(RequestInfo info, Executor executor, MetadataApplier applier) {
        Metadata headers = new Metadata();
        headers.put(Metadata.Key.of("authorization", Metadata.ASCII_STRING_MARSHALLER),
                    "Bearer " + authToken);
        applier.apply(headers);
    }
};
stub = stub.withCallCredentials(creds);
```

---

## 7. Testing Strategy

### 7.1 Test Structure (targeting parity with Go: 30+ tests, Python: 41 tests)

| Test Category | Count | Description |
|---|---|---|
| **Model Serialization** | 5 | Jackson round-trip for Node, Edge, PathResult, QueryResult, RagResult |
| **JSON Client — Node CRUD** | 5 | Create, create with embedding, get, update, delete |
| **JSON Client — Edge CRUD** | 5 | Create, create with temporal, get, update, delete |
| **JSON Client — Traversal** | 4 | Neighbors, neighbors with edge_type, BFS, shortest path |
| **JSON Client — Query** | 1 | GQL execution |
| **JSON Client — Vector Search** | 1 | k-NN search |
| **JSON Client — Hybrid/Semantic** | 3 | Hybrid search, semantic neighbors, semantic walk |
| **JSON Client — Temporal** | 5 | NeighborsAt, NeighborsAt+edgeType, BFSAt, ShortestPathAt, ShortestPathAt weighted |
| **JSON Client — GraphRAG** | 4 | Extract structured, extract prose, RAG with anchor, RAG with embedding |
| **JSON Client — Batch** | 4 | Create nodes batch, create edges batch, delete nodes, delete edges |
| **JSON Client — Auth** | 2 | Token sent when set, token absent when not set |
| **JSON Client — Connection** | 2 | Not-connected error, AutoCloseable lifecycle |
| **NDJSON Wire Protocol** | 3 | Marshal round-trip, framing, partial read |
| **Backoff Algorithm** | 4 | Default values, exponential increase, max cap, reset |
| **gRPC Client** | 7 | Ping, create+get node, create+get edge, delete, neighbors, query, not-found error |
| **Arrow Flight Client** | 3 | DoGet query, DoPut nodes, DoPut edges |
| **Unified Client** | 3 | Transport routing, gRPC fallback to JSON, Arrow fallback to JSON |
| **Error Classification** | 2 | classifyError mapping, gRPC error translation |
| **Options/Builder** | 3 | Default values, all builder methods, SSLContext configuration |
| **Total** | **66** | |

### 7.2 Mock Strategy

**JSON/TCP tests:** Use `java.net.ServerSocket` on localhost with an ephemeral port. A background thread reads requests and writes mock responses.

```java
@Test
void createNode_returnsNodeId() throws Exception {
    try (var mockServer = new MockJsonServer()) {
        mockServer.enqueueResponse("""
            {"status":"ok","data":{"node_id":42}}
            """);

        try (var client = new JsonClientBuilder()
                .host("127.0.0.1").port(mockServer.getPort()).build()) {
            client.connect();
            long id = client.createNode(List.of("Person"), Map.of("name", "Alice"));

            assertEquals(42L, id);

            var req = mockServer.takeRequest();
            assertEquals("CreateNode", req.path("type").asText());
            assertEquals("Alice", req.path("properties").path("name").asText());
        }
    }
}

// MockJsonServer — reusable test fixture
class MockJsonServer implements AutoCloseable {
    private final ServerSocket serverSocket;
    private final BlockingQueue<String> responses = new LinkedBlockingQueue<>();
    private final BlockingQueue<JsonNode> requests = new LinkedBlockingQueue<>();

    MockJsonServer() throws IOException {
        serverSocket = new ServerSocket(0); // ephemeral port
        new Thread(this::acceptLoop).start();
    }

    int getPort() { return serverSocket.getLocalPort(); }
    void enqueueResponse(String json) { responses.add(json.strip()); }
    JsonNode takeRequest() throws InterruptedException { return requests.take(); }

    private void acceptLoop() {
        // Accept connection, read NDJSON lines, enqueue to requests, dequeue responses
    }

    @Override
    public void close() throws IOException { serverSocket.close(); }
}
```

**gRPC tests:** Use `io.grpc.inprocess.InProcessServerBuilder` for zero-network-overhead testing.

```java
@Test
void ping_returnsPong() throws Exception {
    String serverName = InProcessServerBuilder.generateName();
    var mockService = new MockAstraeaService();

    var server = InProcessServerBuilder.forName(serverName)
        .directExecutor()
        .addService(mockService)
        .build().start();

    var channel = InProcessChannelBuilder.forName(serverName)
        .directExecutor().build();

    try (var client = new GrpcClient(channel)) {
        PingResponse resp = client.ping();
        assertTrue(resp.pong());
        assertEquals("1.0.0", resp.version());
    } finally {
        channel.shutdownNow();
        server.shutdownNow();
    }
}
```

**Arrow Flight tests:** Use `FlightServer.builder()` with an in-process `BufferAllocator` and a mock `FlightProducer`.

### 7.3 Parameterized Tests (JUnit 5 Idiom)

```java
@ParameterizedTest
@ValueSource(strings = {"outgoing", "incoming", "both"})
void neighbors_sendsDirection(String direction) throws Exception {
    try (var mockServer = new MockJsonServer()) {
        mockServer.enqueueResponse("""
            {"status":"ok","data":{"neighbors":[]}}
            """);

        try (var client = new JsonClientBuilder()
                .host("127.0.0.1").port(mockServer.getPort()).build()) {
            client.connect();
            client.neighbors(1L, NeighborOptions.builder()
                .direction(direction).build());

            var req = mockServer.takeRequest();
            assertEquals(direction, req.path("direction").asText());
        }
    }
}
```

---

## 8. Reconnection & Backoff

### 8.1 Exponential Backoff Algorithm

Matching the Go/Python client backoff behavior:

```java
public class ExponentialBackoff {
    private static final Duration INITIAL_DELAY = Duration.ofMillis(100);
    private static final Duration MAX_DELAY = Duration.ofSeconds(30);
    private static final double MULTIPLIER = 2.0;
    private static final double JITTER = 0.25;
    private static final ThreadLocalRandom RNG = ThreadLocalRandom.current();

    private Duration currentDelay = INITIAL_DELAY;

    public Duration nextDelay() {
        Duration delay = currentDelay;
        // Apply ±25% jitter
        double jitterFactor = 1.0 + (RNG.nextDouble() * 2 - 1) * JITTER;
        delay = Duration.ofMillis((long) (delay.toMillis() * jitterFactor));
        // Increase for next call
        currentDelay = Duration.ofMillis(
            Math.min((long) (currentDelay.toMillis() * MULTIPLIER), MAX_DELAY.toMillis()));
        return delay;
    }

    public void reset() { currentDelay = INITIAL_DELAY; }
}
```

### 8.2 Auto-Reconnect in JsonClient

```java
private JsonNode sendWithRetry(ObjectNode request) throws AstraeaException {
    AstraeaException lastError = null;
    ExponentialBackoff backoff = new ExponentialBackoff();

    for (int attempt = 0; attempt <= maxRetries; attempt++) {
        try {
            return send(request);
        } catch (AstraeaException e) {
            lastError = e;
            if (!reconnectEnabled || isApplicationError(e)) throw e;
            try {
                Thread.sleep(backoff.nextDelay().toMillis());
            } catch (InterruptedException ie) {
                Thread.currentThread().interrupt();
                throw new AstraeaException("Interrupted during reconnection", ie);
            }
            try { connect(); } catch (AstraeaException ignored) {}
        }
    }
    throw lastError;
}
```

---

## 9. Default Parameter Values

Matching server defaults for consistency (same as Go/Python/R):

| Parameter | Default | Used By |
|---|---|---|
| `maxDepth` | 3 | BFS, BFSAt |
| `weighted` | false | ShortestPath, ShortestPathAt |
| `k` | 10 | VectorSearch, HybridSearch, SemanticNeighbors |
| `maxHops` | 3 | HybridSearch, SemanticWalk |
| `alpha` | 0.5 | HybridSearch |
| `direction` | "outgoing" | Neighbors, SemanticNeighbors, NeighborsAt |
| `hops` | 3 | ExtractSubgraph, GraphRag |
| `maxNodes` | 50 | ExtractSubgraph, GraphRag |
| `format` | "structured" | ExtractSubgraph, GraphRag |
| `weight` | 1.0 | CreateEdge |
| `properties` | `Map.of()` | CreateNode, CreateEdge |
| TCP port | 7687 | JsonClient |
| gRPC port | 7688 | GrpcClient |
| Flight port | 7689 | FlightClient |
| Timeout | 10s | All operations |
| Connect timeout | 5s | Connection |

---

## 10. Implementation Phases

### Phase 1: API Module & JSON/TCP Foundation (Week 1–2)

**Goal:** Core client with CRUD, traversal, and query support over JSON/TCP.

| Task | Files | Tests |
|---|---|---|
| Gradle multi-module scaffolding | `build.gradle.kts`, `settings.gradle.kts`, module `build.gradle.kts` | — |
| Domain types (Java records) | `model/*.java` | `ModelSerializationTest` (5 tests) |
| Exception hierarchy | `exception/*.java` | `ErrorClassificationTest` (2 tests) |
| Per-operation options (builders) | `options/*.java` | `OptionsTest` (3 tests) |
| NDJSON codec (read/write over Socket) | `NdjsonCodec.java` | `NdjsonCodecTest` (3 tests) |
| Exponential backoff with jitter | `ExponentialBackoff.java` | `BackoffTest` (4 tests) |
| MockJsonServer test fixture | `MockJsonServer.java` | — |
| JsonClient: connect, close, TLS, mTLS | `JsonClient.java`, `JsonClientBuilder.java` | Connection tests (2) |
| JsonClient: Ping | `JsonClient.java` | `testPing` |
| JsonClient: Node CRUD (4 methods) | `JsonClient.java` | 5 tests |
| JsonClient: Edge CRUD (4 methods) | `JsonClient.java` | 5 tests |
| JsonClient: Traversal (3 methods) | `JsonClient.java` | 4 tests |
| JsonClient: Query | `JsonClient.java` | 1 test |
| JsonClient: Auth token injection | `JsonClient.java` | 2 tests |

**Deliverable:** `JsonClient` supporting 13 operations with 37 tests.

### Phase 2: Vector, Semantic & Temporal (Week 3)

**Goal:** Add all remaining JSON/TCP operations for full parity.

| Task | Files | Tests |
|---|---|---|
| VectorSearch | `JsonClient.java` | 1 test |
| HybridSearch, SemanticNeighbors, SemanticWalk | `JsonClient.java` | 3 tests |
| NeighborsAt, BFSAt, ShortestPathAt | `JsonClient.java` | 5 tests |
| ExtractSubgraph, GraphRag | `JsonClient.java` | 4 tests |
| Batch operations (CreateNodes, CreateEdges, DeleteNodes, DeleteEdges) | `JsonClient.java` | 4 tests |

**Deliverable:** Full 22-operation JSON client with 54 tests.

### Phase 3: gRPC Transport (Week 4)

**Goal:** gRPC client with protobuf code generation.

| Task | Files | Tests |
|---|---|---|
| Copy `astraea.proto`, configure Gradle protobuf plugin | `proto/`, `build.gradle.kts` | — |
| GrpcClient: connection, close, TLS | `GrpcClient.java`, `GrpcClientBuilder.java` | — |
| GrpcClient: 14 RPC methods | `GrpcClient.java` | 7 tests (InProcessServer) |
| gRPC error translation | `GrpcClient.java` | Included in above |
| MockAstraeaService test fixture | `MockAstraeaService.java` | — |

**Deliverable:** `GrpcClient` supporting 14 operations with 61 cumulative tests.

### Phase 4: Arrow Flight Transport (Week 5)

**Goal:** Arrow Flight client for high-throughput queries and bulk import.

| Task | Files | Tests |
|---|---|---|
| FlightClient: connection, BufferAllocator lifecycle | `FlightClient.java`, `FlightClientBuilder.java` | — |
| DoGet: GQL query → QueryResult | `FlightClient.java` | 1 test |
| DoPut: Bulk node import | `FlightClient.java` | 1 test |
| DoPut: Bulk edge import | `FlightClient.java` | 1 test |
| MockFlightProducer test fixture | `MockFlightProducer.java` | — |

**Deliverable:** `FlightClient` with query + bulk import, 64 cumulative tests.

### Phase 5: Unified Client & Polish (Week 6)

**Goal:** Unified client, examples, documentation.

| Task | Files | Tests |
|---|---|---|
| UnifiedClient with transport auto-selection | `UnifiedClient.java` | 3 tests |
| Graceful degradation (gRPC → JSON, Arrow → JSON) | `UnifiedClient.java` | Included above |
| Basic CRUD example | `examples/BasicExample.java` | — |
| Vector search example | `examples/VectorSearchExample.java` | — |
| GraphRAG example | `examples/GraphRagExample.java` | — |
| Cybersecurity demo (port from Python/Go) | `examples/CybersecurityExample.java` | — |
| Javadoc on all public types | All public files | — |
| README.md | `README.md` | — |

**Deliverable:** Production-ready Java client with 66+ tests and 4 examples.

---

## 11. Key Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| **Java version** | 17+ | Records, sealed classes, pattern matching, text blocks — all needed for clean API. LTS release with broad adoption. |
| **Build tool** | Gradle (Kotlin DSL) | Better protobuf plugin support than Maven; version catalogs for consistent dependency management. |
| **JSON library** | Jackson Databind | Industry standard, `ObjectMapper` thread-safe, records supported natively since 2.15+. |
| **Domain types** | Java Records | Immutable, auto `equals`/`hashCode`/`toString`, concise, pattern-match ready. |
| **Options pattern** | Inner Builder classes on Records | Java-idiomatic; avoids method overload explosion; matches Go's functional options semantically. |
| **Connection model** | `java.net.Socket` | Simple, blocking I/O sufficient for request-response protocol; avoids NIO complexity. |
| **Thread safety** | `ReentrantLock` on client | Explicit lock more flexible than `synchronized`; supports tryLock for timeout scenarios. |
| **Error handling** | Checked exception hierarchy | Java convention; `catch (NodeNotFoundException e)` is cleaner than sentinel checking; compiler enforces handling. |
| **gRPC connection** | `ManagedChannelBuilder` | Standard grpc-java API; supports `usePlaintext()` for dev, TLS for prod. |
| **gRPC testing** | `InProcessServerBuilder` | Zero-network testing; grpc-java officially recommended pattern. |
| **Arrow library** | `org.apache.arrow:flight-core` | Official Apache Arrow Java, includes Flight client/server. |
| **Async support** | `CompletableFuture` (future phase) | Standard Java async; gRPC `FutureStub` maps naturally. Not in initial scope — sync first. |
| **Module system** | Multi-module Gradle | Users who only need JSON/TCP pull zero external deps; gRPC and Arrow are opt-in modules. |
| **Null handling** | `@Nullable` annotations + `Optional` return | `Optional` for getters that may return absent data; `@Nullable` for parameters. |
| **Logging** | SLF4J API (no implementation) | Users bring their own Logback/Log4j2; library never forces a logging framework. |

---

## 12. Comparison with Other Clients

| Feature | Python | R | Go | Java (Planned) |
|---|---|---|---|---|
| **JSON/TCP** | Yes (22 ops) | Yes (22+ ops) | Yes (22 ops) | Yes (22 ops) |
| **gRPC** | No | No | Yes (14 ops) | Yes (14 ops) |
| **Arrow Flight** | Yes | Yes | Stub | Yes |
| **Unified Client** | Yes | Yes | Yes | Yes |
| **Async** | No | No | No | Future (CompletableFuture) |
| **TLS/mTLS** | Yes | No | Yes | Yes |
| **Auth Token** | Yes | Yes | Yes | Yes |
| **Reconnect** | Yes | No | Yes | Yes |
| **Builder/Options** | kwargs | Named args | Functional options | Builder pattern |
| **Error Types** | Single exception | Conditions | 7 sentinel errors | 7 exception subclasses |
| **DataFrame** | Pandas/Polars | data.frame | — | — |
| **Context Manager** | `with` | — | `defer Close()` | `try-with-resources` |
| **Tests** | 41 | — | 30 | 66 (planned) |
| **Examples** | 2 | Inline | 2 | 4 (planned) |
