package com.astraeadb.grpc;

import com.astraeadb.exception.*;
import com.astraeadb.model.*;
import com.astraeadb.options.EdgeOptions;
import com.astraeadb.options.NeighborOptions;

import io.grpc.ManagedChannel;
import io.grpc.Server;
import io.grpc.inprocess.InProcessChannelBuilder;
import io.grpc.inprocess.InProcessServerBuilder;

import org.junit.jupiter.api.*;

import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.*;

/**
 * Tests for {@link GrpcClient} using an in-process gRPC server and mock service.
 * No network I/O is involved.
 */
class GrpcClientTest {

    private static Server server;
    private static ManagedChannel channel;
    private static GrpcClient client;

    @BeforeAll
    static void setup() throws Exception {
        String serverName = InProcessServerBuilder.generateName();
        server = InProcessServerBuilder.forName(serverName)
                .directExecutor()
                .addService(new MockAstraeaService())
                .build()
                .start();
        channel = InProcessChannelBuilder.forName(serverName)
                .directExecutor()
                .build();
        client = new GrpcClient(channel);
    }

    @AfterAll
    static void teardown() throws Exception {
        client.close();
        channel.shutdownNow();
        server.shutdownNow();
    }

    // -----------------------------------------------------------------------
    // Health
    // -----------------------------------------------------------------------

    @Test
    void ping_returnsPong() throws Exception {
        PingResponse resp = client.ping();
        assertThat(resp.pong()).isTrue();
        assertThat(resp.version()).isEqualTo("1.0.0-test");
    }

    // -----------------------------------------------------------------------
    // Node CRUD
    // -----------------------------------------------------------------------

    @Test
    void createNode_returnsNodeId() throws Exception {
        long id = client.createNode(List.of("Person"), Map.of("name", "Bob"));
        assertThat(id).isEqualTo(42L);
    }

    @Test
    void createNode_withEmbedding_returnsNodeId() throws Exception {
        long id = client.createNode(List.of("Concept"), Map.of("title", "AI"),
                new float[]{0.1f, 0.2f, 0.3f});
        assertThat(id).isEqualTo(42L);
    }

    @Test
    void getNode_returnsNode() throws Exception {
        Node node = client.getNode(1);
        assertThat(node.id()).isEqualTo(1L);
        assertThat(node.labels()).containsExactly("Person");
        assertThat(node.properties()).containsEntry("name", "Alice");
        assertThat(node.hasEmbedding()).isFalse();
    }

    @Test
    void getNode_notFound_throwsException() {
        assertThatThrownBy(() -> client.getNode(999))
                .isInstanceOf(NodeNotFoundException.class)
                .hasMessageContaining("node not found");
    }

    @Test
    void updateNode_succeeds() throws Exception {
        // Should not throw
        client.updateNode(1, Map.of("name", "Alice Updated"));
    }

    @Test
    void updateNode_notFound_throwsException() {
        assertThatThrownBy(() -> client.updateNode(999, Map.of("name", "X")))
                .isInstanceOf(NodeNotFoundException.class);
    }

    @Test
    void deleteNode_succeeds() throws Exception {
        // Should not throw
        client.deleteNode(1);
    }

    @Test
    void deleteNode_notFound_throwsException() {
        assertThatThrownBy(() -> client.deleteNode(999))
                .isInstanceOf(NodeNotFoundException.class);
    }

    // -----------------------------------------------------------------------
    // Edge CRUD
    // -----------------------------------------------------------------------

    @Test
    void createEdge_returnsEdgeId() throws Exception {
        long id = client.createEdge(1, 2, "KNOWS");
        assertThat(id).isEqualTo(100L);
    }

    @Test
    void createEdge_withOptions_returnsEdgeId() throws Exception {
        EdgeOptions opts = EdgeOptions.builder()
                .weight(2.5)
                .validFrom(1000L)
                .validTo(2000L)
                .properties(Map.of("note", "test"))
                .build();
        long id = client.createEdge(1, 2, "KNOWS", opts);
        assertThat(id).isEqualTo(100L);
    }

    @Test
    void getEdge_returnsEdge() throws Exception {
        Edge edge = client.getEdge(10);
        assertThat(edge.id()).isEqualTo(10L);
        assertThat(edge.source()).isEqualTo(1L);
        assertThat(edge.target()).isEqualTo(2L);
        assertThat(edge.edgeType()).isEqualTo("KNOWS");
        assertThat(edge.properties()).containsEntry("since", 2020);
        assertThat(edge.weight()).isEqualTo(1.5);
        assertThat(edge.validFrom()).isEqualTo(1000L);
        assertThat(edge.validTo()).isEqualTo(2000L);
    }

    @Test
    void getEdge_notFound_throwsException() {
        assertThatThrownBy(() -> client.getEdge(999))
                .isInstanceOf(EdgeNotFoundException.class)
                .hasMessageContaining("edge not found");
    }

    @Test
    void updateEdge_succeeds() throws Exception {
        client.updateEdge(10, Map.of("note", "updated"));
    }

    @Test
    void deleteEdge_succeeds() throws Exception {
        client.deleteEdge(10);
    }

    // -----------------------------------------------------------------------
    // Traversal
    // -----------------------------------------------------------------------

    @Test
    void neighbors_returnsEntries() throws Exception {
        List<NeighborEntry> entries = client.neighbors(1);
        assertThat(entries).hasSize(2);
        assertThat(entries.get(0).edgeId()).isEqualTo(10L);
        assertThat(entries.get(0).nodeId()).isEqualTo(20L);
        assertThat(entries.get(1).edgeId()).isEqualTo(11L);
        assertThat(entries.get(1).nodeId()).isEqualTo(21L);
    }

    @Test
    void neighbors_withOptions_returnsEntries() throws Exception {
        NeighborOptions opts = NeighborOptions.builder()
                .direction("incoming")
                .edgeType("KNOWS")
                .build();
        List<NeighborEntry> entries = client.neighbors(1, opts);
        assertThat(entries).hasSize(2);
    }

    @Test
    void bfs_returnsEntries() throws Exception {
        List<BfsEntry> entries = client.bfs(1, 3);
        assertThat(entries).hasSize(3);
        assertThat(entries.get(0).nodeId()).isEqualTo(1L);
        assertThat(entries.get(0).depth()).isEqualTo(0);
        assertThat(entries.get(1).nodeId()).isEqualTo(20L);
        assertThat(entries.get(1).depth()).isEqualTo(1);
        assertThat(entries.get(2).nodeId()).isEqualTo(30L);
        assertThat(entries.get(2).depth()).isEqualTo(2);
    }

    @Test
    void shortestPath_returnsResult() throws Exception {
        PathResult result = client.shortestPath(1, 10, true);
        assertThat(result.found()).isTrue();
        assertThat(result.path()).containsExactly(1L, 5L, 10L);
        assertThat(result.length()).isEqualTo(2);
        assertThat(result.cost()).isEqualTo(3.5);
    }

    // -----------------------------------------------------------------------
    // Vector search
    // -----------------------------------------------------------------------

    @Test
    void vectorSearch_returnsResults() throws Exception {
        List<SearchResult> results = client.vectorSearch(new float[]{0.1f, 0.2f, 0.3f}, 3);
        assertThat(results).hasSize(3);
        assertThat(results.get(0).nodeId()).isEqualTo(100L);
        assertThat(results.get(0).score()).isGreaterThan(0.0);
    }

    @Test
    void vectorSearch_defaultK_returnsResults() throws Exception {
        List<SearchResult> results = client.vectorSearch(new float[]{0.5f, 0.6f});
        assertThat(results).isNotEmpty();
    }

    // -----------------------------------------------------------------------
    // GQL query
    // -----------------------------------------------------------------------

    @Test
    void query_returnsResult() throws Exception {
        QueryResult result = client.query("MATCH (n) RETURN n LIMIT 1");
        assertThat(result.columns()).containsExactly("n");
        assertThat(result.rows()).hasSize(1);
        assertThat(result.stats()).isNotNull();
    }

    // -----------------------------------------------------------------------
    // Unsupported operations
    // -----------------------------------------------------------------------

    @Test
    void unsupportedOperation_neighborsAt_throwsException() {
        assertThatThrownBy(() -> client.neighborsAt(1, "outgoing", 1000))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_bfsAt_throwsException() {
        assertThatThrownBy(() -> client.bfsAt(1, 3, 1000))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_shortestPathAt_throwsException() {
        assertThatThrownBy(() -> client.shortestPathAt(1, 2, 1000, false))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_hybridSearch_throwsException() {
        assertThatThrownBy(() -> client.hybridSearch(1, new float[]{0.1f}))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_semanticNeighbors_throwsException() {
        assertThatThrownBy(() -> client.semanticNeighbors(1, new float[]{0.1f}))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_semanticWalk_throwsException() {
        assertThatThrownBy(() -> client.semanticWalk(1, new float[]{0.1f}, 3))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_extractSubgraph_throwsException() {
        assertThatThrownBy(() -> client.extractSubgraph(1))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_graphRag_throwsException() {
        assertThatThrownBy(() -> client.graphRag("What is AI?"))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_createNodes_throwsException() {
        assertThatThrownBy(() -> client.createNodes(List.of()))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_createEdges_throwsException() {
        assertThatThrownBy(() -> client.createEdges(List.of()))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_deleteNodes_throwsException() {
        assertThatThrownBy(() -> client.deleteNodes(List.of()))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }

    @Test
    void unsupportedOperation_deleteEdges_throwsException() {
        assertThatThrownBy(() -> client.deleteEdges(List.of()))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over gRPC");
    }
}
