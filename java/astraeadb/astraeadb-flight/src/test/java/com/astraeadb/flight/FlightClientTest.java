package com.astraeadb.flight;

import com.astraeadb.exception.NotConnectedException;
import com.astraeadb.model.*;
import com.astraeadb.options.*;
import org.junit.jupiter.api.Test;

import java.time.Duration;
import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.*;

class FlightClientTest {

    // ------------------------------------------------------------------ builder tests

    @Test
    void builderDefaults() {
        var builder = new FlightClientBuilder();
        assertThat(builder.host()).isEqualTo("127.0.0.1");
        assertThat(builder.port()).isEqualTo(7689);
        assertThat(builder.authToken()).isNull();
        assertThat(builder.timeout()).isEqualTo(Duration.ofSeconds(10));
        assertThat(builder.useTls()).isFalse();
    }

    @Test
    void builderCustomValues() {
        var builder = new FlightClientBuilder()
                .host("myhost")
                .port(9999)
                .authToken("tok")
                .timeout(Duration.ofSeconds(30))
                .useTls(true);
        assertThat(builder.host()).isEqualTo("myhost");
        assertThat(builder.port()).isEqualTo(9999);
        assertThat(builder.authToken()).isEqualTo("tok");
        assertThat(builder.timeout()).isEqualTo(Duration.ofSeconds(30));
        assertThat(builder.useTls()).isTrue();
    }

    @Test
    void buildReturnsFlightAstraeaClient() {
        var client = new FlightClientBuilder().build();
        assertThat(client).isInstanceOf(FlightAstraeaClient.class);
    }

    // ------------------------------------------------------------------ not-connected guard

    @Test
    void notConnected_query_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.query("MATCH (n) RETURN n"))
                .isInstanceOf(NotConnectedException.class);
    }

    @Test
    void notConnected_createNodes_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.createNodes(List.of(new NodeInput(List.of("Person")))))
                .isInstanceOf(NotConnectedException.class);
    }

    @Test
    void notConnected_createEdges_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.createEdges(List.of(new EdgeInput(1, 2, "KNOWS"))))
                .isInstanceOf(NotConnectedException.class);
    }

    // ------------------------------------------------------------------ unsupported single-record ops

    @Test
    void unsupported_createNode_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.createNode(List.of("Person"), Map.of("name", "Alice"), null))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_getNode_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.getNode(1L))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_updateNode_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.updateNode(1L, Map.of("age", 30)))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_deleteNode_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.deleteNode(1L))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_createEdge_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.createEdge(1L, 2L, "KNOWS"))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_getEdge_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.getEdge(1L))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_updateEdge_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.updateEdge(1L, Map.of("weight", 2.0)))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_deleteEdge_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.deleteEdge(1L))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    // ------------------------------------------------------------------ unsupported traversal

    @Test
    void unsupported_neighbors_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.neighbors(1L, NeighborOptions.DEFAULT))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_bfs_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.bfs(1L, 3))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_shortestPath_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.shortestPath(1L, 2L, false))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    // ------------------------------------------------------------------ unsupported temporal

    @Test
    void unsupported_neighborsAt_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.neighborsAt(1L, "outgoing", 1000L))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_neighborsAtWithEdgeType_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.neighborsAt(1L, "outgoing", 1000L, "KNOWS"))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_bfsAt_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.bfsAt(1L, 3, 1000L))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_shortestPathAt_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.shortestPathAt(1L, 2L, 1000L, false))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    // ------------------------------------------------------------------ unsupported vector & semantic

    @Test
    void unsupported_vectorSearch_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.vectorSearch(new float[]{0.1f}, 10))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_hybridSearch_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.hybridSearch(1L, new float[]{0.1f}, HybridSearchOptions.DEFAULT))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_semanticNeighbors_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.semanticNeighbors(1L, new float[]{0.1f}, SemanticOptions.DEFAULT))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_semanticWalk_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.semanticWalk(1L, new float[]{0.1f}, 3))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    // ------------------------------------------------------------------ unsupported GraphRAG

    @Test
    void unsupported_extractSubgraph_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.extractSubgraph(1L, SubgraphOptions.DEFAULT))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_graphRag_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.graphRag("What is risk?", RagOptions.DEFAULT))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    // ------------------------------------------------------------------ unsupported ping

    @Test
    void unsupported_ping_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.ping())
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    // ------------------------------------------------------------------ unsupported batch delete

    @Test
    void unsupported_deleteNodes_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.deleteNodes(List.of(1L, 2L)))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }

    @Test
    void unsupported_deleteEdges_throwsException() {
        var client = new FlightClientBuilder().build();
        assertThatThrownBy(() -> client.deleteEdges(List.of(1L, 2L)))
                .isInstanceOf(UnsupportedOperationException.class)
                .hasMessageContaining("not supported over Arrow Flight");
    }
}
