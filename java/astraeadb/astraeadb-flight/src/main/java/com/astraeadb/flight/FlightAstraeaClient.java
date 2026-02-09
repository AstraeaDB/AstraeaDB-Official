package com.astraeadb.flight;

import com.astraeadb.AstraeaClient;
import com.astraeadb.exception.AstraeaException;
import com.astraeadb.exception.NotConnectedException;
import com.astraeadb.model.*;
import com.astraeadb.options.*;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.apache.arrow.flight.*;
import org.apache.arrow.memory.BufferAllocator;
import org.apache.arrow.memory.RootAllocator;
import org.apache.arrow.vector.*;
import org.apache.arrow.vector.types.FloatingPointPrecision;
import org.apache.arrow.vector.types.pojo.ArrowType;
import org.apache.arrow.vector.types.pojo.Field;
import org.apache.arrow.vector.types.pojo.Schema;

import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * Arrow Flight-based client for AstraeaDB.
 *
 * <p>This transport is optimised for bulk data movement and columnar analytics.
 * It supports three operations natively over the Flight protocol:
 * <ul>
 *   <li>{@link #query(String)} -- executes a GQL query via {@code DoGet}</li>
 *   <li>{@link #createNodes(List)} -- bulk node insertion via {@code DoPut}</li>
 *   <li>{@link #createEdges(List)} -- bulk edge insertion via {@code DoPut}</li>
 * </ul>
 *
 * All other operations defined on {@link AstraeaClient} throw
 * {@link UnsupportedOperationException} because they are single-record
 * operations better served by the JSON/TCP or gRPC transports.
 */
public class FlightAstraeaClient implements AstraeaClient {

    private static final String UNSUPPORTED_MSG =
            "Operation not supported over Arrow Flight; use JsonClient or UnifiedClient";

    private final String host;
    private final int port;
    private final String authToken;
    private final Duration timeout;
    private final boolean useTls;

    private final ObjectMapper mapper = new ObjectMapper();

    private BufferAllocator allocator;
    private FlightClient flightClient;
    private volatile boolean connected;

    FlightAstraeaClient(FlightClientBuilder builder) {
        this.host = builder.host();
        this.port = builder.port();
        this.authToken = builder.authToken();
        this.timeout = builder.timeout();
        this.useTls = builder.useTls();
    }

    // ------------------------------------------------------------------ lifecycle

    @Override
    public void connect() throws AstraeaException {
        try {
            Location location = useTls
                    ? Location.forGrpcTls(host, port)
                    : Location.forGrpcInsecure(host, port);
            allocator = new RootAllocator();
            flightClient = FlightClient.builder(allocator, location).build();
            connected = true;
        } catch (Exception e) {
            throw new AstraeaException("Failed to connect to Flight server: " + e.getMessage(), e);
        }
    }

    @Override
    public void close() throws AstraeaException {
        connected = false;
        try {
            if (flightClient != null) {
                flightClient.close();
            }
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new AstraeaException("Interrupted while closing Flight client", e);
        } catch (Exception e) {
            throw new AstraeaException("Error closing Flight client: " + e.getMessage(), e);
        } finally {
            if (allocator != null) {
                allocator.close();
            }
        }
    }

    // ------------------------------------------------------------------ health

    @Override
    public PingResponse ping() {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // ------------------------------------------------------------------ node CRUD (unsupported)

    @Override
    public long createNode(List<String> labels, Map<String, Object> properties, float[] embedding) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public Node getNode(long id) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public void updateNode(long id, Map<String, Object> properties) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public void deleteNode(long id) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // ------------------------------------------------------------------ edge CRUD (unsupported)

    @Override
    public long createEdge(long source, long target, String edgeType, EdgeOptions options) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public Edge getEdge(long id) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public void updateEdge(long id, Map<String, Object> properties) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public void deleteEdge(long id) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // ------------------------------------------------------------------ traversal (unsupported)

    @Override
    public List<NeighborEntry> neighbors(long id, NeighborOptions options) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<BfsEntry> bfs(long start, int maxDepth) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public PathResult shortestPath(long from, long to, boolean weighted) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // ------------------------------------------------------------------ temporal (unsupported)

    @Override
    public List<NeighborEntry> neighborsAt(long id, String direction, long timestamp) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<NeighborEntry> neighborsAt(long id, String direction, long timestamp, String edgeType) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<BfsEntry> bfsAt(long start, int maxDepth, long timestamp) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public PathResult shortestPathAt(long from, long to, long timestamp, boolean weighted) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // ------------------------------------------------------------------ vector & semantic (unsupported)

    @Override
    public List<SearchResult> vectorSearch(float[] query, int k) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<SearchResult> hybridSearch(long anchor, float[] query, HybridSearchOptions options) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<SearchResult> semanticNeighbors(long id, float[] concept, SemanticOptions options) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public List<WalkStep> semanticWalk(long start, float[] concept, int maxHops) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // ------------------------------------------------------------------ GQL query (supported)

    /**
     * Executes a GQL query via Arrow Flight {@code DoGet}.
     *
     * <p>The query string is sent as a {@link Ticket} payload. The server is
     * expected to return a Flight stream whose schema represents the result
     * columns and whose batches contain the result rows.
     *
     * @param gql the GQL query string
     * @return a {@link QueryResult} with columns and rows materialised from the Flight stream
     * @throws AstraeaException if the query fails or a transport error occurs
     */
    @Override
    public QueryResult query(String gql) throws AstraeaException {
        ensureConnected();
        try {
            Ticket ticket = new Ticket(gql.getBytes(StandardCharsets.UTF_8));
            FlightStream stream = flightClient.getStream(ticket);

            List<String> columns = new ArrayList<>();
            List<List<Object>> rows = new ArrayList<>();

            VectorSchemaRoot root = stream.getRoot();
            for (Field field : root.getSchema().getFields()) {
                columns.add(field.getName());
            }

            while (stream.next()) {
                for (int i = 0; i < root.getRowCount(); i++) {
                    List<Object> row = new ArrayList<>();
                    for (FieldVector vec : root.getFieldVectors()) {
                        row.add(vec.isNull(i) ? null : vec.getObject(i));
                    }
                    rows.add(row);
                }
            }
            stream.close();

            return new QueryResult(columns, rows, QueryResult.QueryStats.EMPTY);
        } catch (Exception e) {
            throw new AstraeaException("Flight query failed: " + e.getMessage(), e);
        }
    }

    // ------------------------------------------------------------------ GraphRAG (unsupported)

    @Override
    public SubgraphResult extractSubgraph(long center, SubgraphOptions options) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public RagResult graphRag(String question, RagOptions options) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // ------------------------------------------------------------------ batch operations (supported)

    /**
     * Bulk-inserts nodes via Arrow Flight {@code DoPut}.
     *
     * <p>The node data is encoded into a columnar Arrow batch with four columns:
     * <ul>
     *   <li>{@code id} -- placeholder UInt64, filled with zeros (the server assigns real IDs)</li>
     *   <li>{@code labels} -- JSON-encoded list of label strings</li>
     *   <li>{@code properties} -- JSON-encoded property map</li>
     *   <li>{@code has_embedding} -- boolean flag indicating whether an embedding is present</li>
     * </ul>
     *
     * @param nodes the list of {@link NodeInput} records to insert
     * @return a list of placeholder IDs (server assigns the actual IDs)
     * @throws AstraeaException if the insertion fails or a transport error occurs
     */
    @Override
    public List<Long> createNodes(List<NodeInput> nodes) throws AstraeaException {
        ensureConnected();
        try {
            Schema schema = new Schema(List.of(
                    Field.nullable("id", new ArrowType.Int(64, false)),
                    Field.nullable("labels", ArrowType.Utf8.INSTANCE),
                    Field.nullable("properties", ArrowType.Utf8.INSTANCE),
                    Field.nullable("has_embedding", ArrowType.Bool.INSTANCE)
            ));

            try (VectorSchemaRoot root = VectorSchemaRoot.create(schema, allocator)) {
                root.allocateNew();

                UInt8Vector idVec = (UInt8Vector) root.getVector("id");
                VarCharVector labelsVec = (VarCharVector) root.getVector("labels");
                VarCharVector propsVec = (VarCharVector) root.getVector("properties");
                BitVector embedVec = (BitVector) root.getVector("has_embedding");

                for (int i = 0; i < nodes.size(); i++) {
                    NodeInput n = nodes.get(i);
                    idVec.setSafe(i, 0);
                    labelsVec.setSafe(i, mapper.writeValueAsString(n.labels()).getBytes(StandardCharsets.UTF_8));
                    propsVec.setSafe(i, mapper.writeValueAsString(n.properties()).getBytes(StandardCharsets.UTF_8));
                    embedVec.setSafe(i, n.embedding() != null ? 1 : 0);
                }
                root.setRowCount(nodes.size());

                FlightDescriptor descriptor = FlightDescriptor.command(
                        "bulk_insert_nodes".getBytes(StandardCharsets.UTF_8));
                FlightClient.ClientStreamListener listener =
                        flightClient.startPut(descriptor, root, new AsyncPutListener());
                listener.putNext();
                listener.completed();
                listener.getResult();
            }

            // Return placeholder IDs; the server assigns actual identifiers.
            List<Long> ids = new ArrayList<>();
            for (int i = 0; i < nodes.size(); i++) {
                ids.add((long) i);
            }
            return ids;
        } catch (Exception e) {
            throw new AstraeaException("Flight bulk node insert failed: " + e.getMessage(), e);
        }
    }

    /**
     * Bulk-inserts edges via Arrow Flight {@code DoPut}.
     *
     * <p>The edge data is encoded into a columnar Arrow batch with eight columns:
     * <ul>
     *   <li>{@code id} -- placeholder UInt64, filled with zeros (the server assigns real IDs)</li>
     *   <li>{@code source} -- UInt64 source node ID</li>
     *   <li>{@code target} -- UInt64 target node ID</li>
     *   <li>{@code edge_type} -- the relationship type string</li>
     *   <li>{@code properties} -- JSON-encoded property map</li>
     *   <li>{@code weight} -- Float64 edge weight</li>
     *   <li>{@code valid_from} -- nullable Int64 temporal start</li>
     *   <li>{@code valid_to} -- nullable Int64 temporal end</li>
     * </ul>
     *
     * @param edges the list of {@link EdgeInput} records to insert
     * @return a list of placeholder IDs (server assigns the actual IDs)
     * @throws AstraeaException if the insertion fails or a transport error occurs
     */
    @Override
    public List<Long> createEdges(List<EdgeInput> edges) throws AstraeaException {
        ensureConnected();
        try {
            Schema schema = new Schema(List.of(
                    Field.nullable("id", new ArrowType.Int(64, false)),
                    Field.nullable("source", new ArrowType.Int(64, false)),
                    Field.nullable("target", new ArrowType.Int(64, false)),
                    Field.nullable("edge_type", ArrowType.Utf8.INSTANCE),
                    Field.nullable("properties", ArrowType.Utf8.INSTANCE),
                    Field.nullable("weight", new ArrowType.FloatingPoint(FloatingPointPrecision.DOUBLE)),
                    Field.nullable("valid_from", new ArrowType.Int(64, true)),
                    Field.nullable("valid_to", new ArrowType.Int(64, true))
            ));

            try (VectorSchemaRoot root = VectorSchemaRoot.create(schema, allocator)) {
                root.allocateNew();

                UInt8Vector idVec = (UInt8Vector) root.getVector("id");
                UInt8Vector sourceVec = (UInt8Vector) root.getVector("source");
                UInt8Vector targetVec = (UInt8Vector) root.getVector("target");
                VarCharVector typeVec = (VarCharVector) root.getVector("edge_type");
                VarCharVector propsVec = (VarCharVector) root.getVector("properties");
                Float8Vector weightVec = (Float8Vector) root.getVector("weight");
                BigIntVector validFromVec = (BigIntVector) root.getVector("valid_from");
                BigIntVector validToVec = (BigIntVector) root.getVector("valid_to");

                for (int i = 0; i < edges.size(); i++) {
                    EdgeInput e = edges.get(i);
                    idVec.setSafe(i, 0);
                    sourceVec.setSafe(i, (byte) e.source());
                    targetVec.setSafe(i, (byte) e.target());
                    typeVec.setSafe(i, e.edgeType().getBytes(StandardCharsets.UTF_8));
                    propsVec.setSafe(i, mapper.writeValueAsString(e.properties()).getBytes(StandardCharsets.UTF_8));
                    weightVec.setSafe(i, e.weight());

                    if (e.validFrom() != null) {
                        validFromVec.setSafe(i, e.validFrom());
                    } else {
                        validFromVec.setNull(i);
                    }
                    if (e.validTo() != null) {
                        validToVec.setSafe(i, e.validTo());
                    } else {
                        validToVec.setNull(i);
                    }
                }
                root.setRowCount(edges.size());

                FlightDescriptor descriptor = FlightDescriptor.command(
                        "bulk_insert_edges".getBytes(StandardCharsets.UTF_8));
                FlightClient.ClientStreamListener listener =
                        flightClient.startPut(descriptor, root, new AsyncPutListener());
                listener.putNext();
                listener.completed();
                listener.getResult();
            }

            // Return placeholder IDs; the server assigns actual identifiers.
            List<Long> ids = new ArrayList<>();
            for (int i = 0; i < edges.size(); i++) {
                ids.add((long) i);
            }
            return ids;
        } catch (Exception e) {
            throw new AstraeaException("Flight bulk edge insert failed: " + e.getMessage(), e);
        }
    }

    @Override
    public int deleteNodes(List<Long> ids) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    @Override
    public int deleteEdges(List<Long> ids) {
        throw new UnsupportedOperationException(UNSUPPORTED_MSG);
    }

    // ------------------------------------------------------------------ internal helpers

    private void ensureConnected() throws NotConnectedException {
        if (!connected) {
            throw new NotConnectedException();
        }
    }
}
