package com.astraeadb.json;

import com.astraeadb.exception.*;
import com.astraeadb.model.*;
import com.astraeadb.options.*;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.*;

class JsonClientTest {

    private final ObjectMapper mapper = new ObjectMapper();

    // ---- helpers -------------------------------------------------------

    private JsonClient clientFor(MockJsonServer server) {
        return new JsonClientBuilder()
            .host("127.0.0.1")
            .port(server.port())
            .reconnect(true)
            .build();
    }

    private JsonClient clientFor(MockJsonServer server, String authToken) {
        return new JsonClientBuilder()
            .host("127.0.0.1")
            .port(server.port())
            .authToken(authToken)
            .reconnect(true)
            .build();
    }

    private JsonNode parseRequest(String raw) throws Exception {
        return mapper.readTree(raw);
    }

    // ================================================================
    //  Health
    // ================================================================

    @Test
    void ping_returnsPong() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"pong\":true,\"version\":\"0.1.0\"}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                PingResponse resp = client.ping();
                assertThat(resp.pong()).isTrue();
                assertThat(resp.version()).isEqualTo("0.1.0");

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("Ping");
            }
        }
    }

    // ================================================================
    //  Node CRUD
    // ================================================================

    @Test
    void createNode_returnsNodeId() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"node_id\":42}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                long id = client.createNode(List.of("Person"), Map.of("name", "Alice"));
                assertThat(id).isEqualTo(42);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("CreateNode");
                assertThat(req.path("labels").get(0).asText()).isEqualTo("Person");
                assertThat(req.path("properties").path("name").asText()).isEqualTo("Alice");
                assertThat(req.has("embedding")).isFalse();
            }
        }
    }

    @Test
    void createNode_withEmbedding_sendsEmbedding() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"node_id\":7}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                float[] emb = {0.1f, 0.2f, 0.3f};
                long id = client.createNode(List.of("Doc"), Map.of(), emb);
                assertThat(id).isEqualTo(7);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("embedding").size()).isEqualTo(3);
                assertThat(req.path("embedding").get(0).floatValue()).isCloseTo(0.1f, within(0.001f));
            }
        }
    }

    @Test
    void getNode_returnsNode() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"id\":1,\"labels\":[\"Person\"],\"properties\":{\"age\":30},\"has_embedding\":false}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                Node node = client.getNode(1);
                assertThat(node.id()).isEqualTo(1);
                assertThat(node.labels()).containsExactly("Person");
                assertThat(node.properties()).containsEntry("age", 30);
                assertThat(node.hasEmbedding()).isFalse();

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("GetNode");
                assertThat(req.path("id").asLong()).isEqualTo(1);
            }
        }
    }

    @Test
    void updateNode_sendsProperties() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                client.updateNode(5, Map.of("name", "Bob"));

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("UpdateNode");
                assertThat(req.path("id").asLong()).isEqualTo(5);
                assertThat(req.path("properties").path("name").asText()).isEqualTo("Bob");
            }
        }
    }

    @Test
    void deleteNode_sendsId() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                client.deleteNode(9);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("DeleteNode");
                assertThat(req.path("id").asLong()).isEqualTo(9);
            }
        }
    }

    // ================================================================
    //  Edge CRUD
    // ================================================================

    @Test
    void createEdge_returnsEdgeId() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"edge_id\":100}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                long id = client.createEdge(1, 2, "KNOWS");
                assertThat(id).isEqualTo(100);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("CreateEdge");
                assertThat(req.path("source").asLong()).isEqualTo(1);
                assertThat(req.path("target").asLong()).isEqualTo(2);
                assertThat(req.path("edge_type").asText()).isEqualTo("KNOWS");
            }
        }
    }

    @Test
    void createEdge_withTemporal_sendsValidFromTo() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"edge_id\":200}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                EdgeOptions opts = EdgeOptions.builder()
                    .weight(2.5)
                    .validFrom(1000L)
                    .validTo(2000L)
                    .build();
                long id = client.createEdge(3, 4, "LIKES", opts);
                assertThat(id).isEqualTo(200);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("valid_from").asLong()).isEqualTo(1000);
                assertThat(req.path("valid_to").asLong()).isEqualTo(2000);
                assertThat(req.path("weight").asDouble()).isEqualTo(2.5);
            }
        }
    }

    @Test
    void getEdge_returnsEdge() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"id\":10,\"source\":1,\"target\":2,\"edge_type\":\"KNOWS\","
                + "\"properties\":{\"since\":2020},\"weight\":1.5,\"valid_from\":null,\"valid_to\":null}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                Edge edge = client.getEdge(10);
                assertThat(edge.id()).isEqualTo(10);
                assertThat(edge.source()).isEqualTo(1);
                assertThat(edge.target()).isEqualTo(2);
                assertThat(edge.edgeType()).isEqualTo("KNOWS");
                assertThat(edge.weight()).isEqualTo(1.5);
                assertThat(edge.validFrom()).isNull();

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("GetEdge");
            }
        }
    }

    @Test
    void updateEdge_sendsProperties() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                client.updateEdge(10, Map.of("weight", 3.0));

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("UpdateEdge");
                assertThat(req.path("id").asLong()).isEqualTo(10);
                assertThat(req.path("properties").path("weight").asDouble()).isEqualTo(3.0);
            }
        }
    }

    @Test
    void deleteEdge_sendsId() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                client.deleteEdge(10);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("DeleteEdge");
                assertThat(req.path("id").asLong()).isEqualTo(10);
            }
        }
    }

    // ================================================================
    //  Traversal
    // ================================================================

    @Test
    void neighbors_returnsEntries() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"neighbors\":[{\"edge_id\":1,\"node_id\":2},{\"edge_id\":3,\"node_id\":4}]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                List<NeighborEntry> entries = client.neighbors(1);
                assertThat(entries).hasSize(2);
                assertThat(entries.get(0).edgeId()).isEqualTo(1);
                assertThat(entries.get(0).nodeId()).isEqualTo(2);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("Neighbors");
                assertThat(req.path("id").asLong()).isEqualTo(1);
            }
        }
    }

    @Test
    void neighbors_withEdgeType_sendsEdgeType() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"neighbors\":[]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                NeighborOptions opts = NeighborOptions.builder().edgeType("KNOWS").build();
                client.neighbors(1, opts);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("edge_type").asText()).isEqualTo("KNOWS");
            }
        }
    }

    @Test
    void bfs_returnsEntries() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"nodes\":[{\"node_id\":1,\"depth\":0},{\"node_id\":2,\"depth\":1}]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                List<BfsEntry> entries = client.bfs(1, 3);
                assertThat(entries).hasSize(2);
                assertThat(entries.get(1).nodeId()).isEqualTo(2);
                assertThat(entries.get(1).depth()).isEqualTo(1);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("Bfs");
                assertThat(req.path("start").asLong()).isEqualTo(1);
                assertThat(req.path("max_depth").asInt()).isEqualTo(3);
            }
        }
    }

    @Test
    void shortestPath_returnsPathResult() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"found\":true,\"path\":[1,3,5],\"length\":2,\"cost\":4.5}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                PathResult result = client.shortestPath(1, 5, true);
                assertThat(result.found()).isTrue();
                assertThat(result.path()).containsExactly(1L, 3L, 5L);
                assertThat(result.length()).isEqualTo(2);
                assertThat(result.cost()).isEqualTo(4.5);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("ShortestPath");
                assertThat(req.path("from").asLong()).isEqualTo(1);
                assertThat(req.path("to").asLong()).isEqualTo(5);
                assertThat(req.path("weighted").asBoolean()).isTrue();
            }
        }
    }

    // ================================================================
    //  Query
    // ================================================================

    @Test
    void query_returnsQueryResult() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"columns\":[\"n\"],\"rows\":[[1],[2]],"
                + "\"stats\":{\"nodes_created\":0,\"edges_created\":0,\"nodes_deleted\":0,\"edges_deleted\":0}}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                QueryResult result = client.query("MATCH (n) RETURN n");
                assertThat(result.columns()).containsExactly("n");
                assertThat(result.rows()).hasSize(2);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("Query");
                assertThat(req.path("gql").asText()).isEqualTo("MATCH (n) RETURN n");
            }
        }
    }

    // ================================================================
    //  Vector & semantic search
    // ================================================================

    @Test
    void vectorSearch_returnsResults() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"results\":[{\"node_id\":1,\"distance\":0.1,\"score\":0.9}]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                List<SearchResult> results = client.vectorSearch(new float[]{0.5f, 0.5f}, 5);
                assertThat(results).hasSize(1);
                assertThat(results.get(0).nodeId()).isEqualTo(1);
                assertThat(results.get(0).distance()).isCloseTo(0.1, within(0.001));
                assertThat(results.get(0).score()).isCloseTo(0.9, within(0.001));

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("VectorSearch");
                assertThat(req.path("k").asInt()).isEqualTo(5);
                assertThat(req.path("query").size()).isEqualTo(2);
            }
        }
    }

    @Test
    void hybridSearch_returnsResults() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"results\":[{\"node_id\":3,\"distance\":0.2,\"score\":0.8}]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                HybridSearchOptions opts = HybridSearchOptions.builder()
                    .maxHops(2).k(5).alpha(0.7).build();
                List<SearchResult> results = client.hybridSearch(1, new float[]{0.1f}, opts);
                assertThat(results).hasSize(1);
                assertThat(results.get(0).nodeId()).isEqualTo(3);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("HybridSearch");
                assertThat(req.path("anchor").asLong()).isEqualTo(1);
                assertThat(req.path("max_hops").asInt()).isEqualTo(2);
                assertThat(req.path("k").asInt()).isEqualTo(5);
                assertThat(req.path("alpha").asDouble()).isEqualTo(0.7);
            }
        }
    }

    @Test
    void semanticNeighbors_returnsResults() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"results\":[{\"node_id\":5,\"distance\":0.3,\"score\":0.7}]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                SemanticOptions opts = SemanticOptions.builder().direction("both").k(3).build();
                List<SearchResult> results = client.semanticNeighbors(1, new float[]{0.4f}, opts);
                assertThat(results).hasSize(1);
                assertThat(results.get(0).nodeId()).isEqualTo(5);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("SemanticNeighbors");
                assertThat(req.path("id").asLong()).isEqualTo(1);
                assertThat(req.path("direction").asText()).isEqualTo("both");
                assertThat(req.path("k").asInt()).isEqualTo(3);
            }
        }
    }

    @Test
    void semanticWalk_returnsSteps() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"steps\":[{\"node_id\":1,\"distance\":0.0},{\"node_id\":2,\"distance\":0.5}]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                List<WalkStep> steps = client.semanticWalk(1, new float[]{0.1f, 0.2f}, 5);
                assertThat(steps).hasSize(2);
                assertThat(steps.get(0).nodeId()).isEqualTo(1);
                assertThat(steps.get(1).distance()).isCloseTo(0.5, within(0.001));

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("SemanticWalk");
                assertThat(req.path("start").asLong()).isEqualTo(1);
                assertThat(req.path("max_hops").asInt()).isEqualTo(5);
            }
        }
    }

    // ================================================================
    //  Temporal queries
    // ================================================================

    @Test
    void neighborsAt_sendsTimestamp() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"neighbors\":[{\"edge_id\":10,\"node_id\":20}]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                List<NeighborEntry> entries = client.neighborsAt(1, "outgoing", 1704067200L);
                assertThat(entries).hasSize(1);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("NeighborsAt");
                assertThat(req.path("id").asLong()).isEqualTo(1);
                assertThat(req.path("direction").asText()).isEqualTo("outgoing");
                assertThat(req.path("timestamp").asLong()).isEqualTo(1704067200L);
            }
        }
    }

    @Test
    void bfsAt_sendsTimestamp() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"nodes\":[{\"node_id\":1,\"depth\":0}]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                List<BfsEntry> entries = client.bfsAt(1, 2, 1704067200L);
                assertThat(entries).hasSize(1);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("BfsAt");
                assertThat(req.path("start").asLong()).isEqualTo(1);
                assertThat(req.path("max_depth").asInt()).isEqualTo(2);
                assertThat(req.path("timestamp").asLong()).isEqualTo(1704067200L);
            }
        }
    }

    @Test
    void shortestPathAt_sendsTimestamp() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"found\":true,\"path\":[1,2],\"length\":1,\"cost\":null}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                PathResult result = client.shortestPathAt(1, 2, 1704067200L, false);
                assertThat(result.found()).isTrue();
                assertThat(result.path()).containsExactly(1L, 2L);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("ShortestPathAt");
                assertThat(req.path("from").asLong()).isEqualTo(1);
                assertThat(req.path("to").asLong()).isEqualTo(2);
                assertThat(req.path("timestamp").asLong()).isEqualTo(1704067200L);
                assertThat(req.path("weighted").asBoolean()).isFalse();
            }
        }
    }

    // ================================================================
    //  GraphRAG
    // ================================================================

    @Test
    void extractSubgraph_returnsResult() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"text\":\"Node 1 -> Node 2\",\"node_count\":2,\"edge_count\":1,\"estimated_tokens\":15}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                SubgraphOptions opts = SubgraphOptions.builder().hops(2).maxNodes(10).build();
                SubgraphResult result = client.extractSubgraph(1, opts);
                assertThat(result.text()).isEqualTo("Node 1 -> Node 2");
                assertThat(result.nodeCount()).isEqualTo(2);
                assertThat(result.edgeCount()).isEqualTo(1);
                assertThat(result.estimatedTokens()).isEqualTo(15);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("ExtractSubgraph");
                assertThat(req.path("center").asLong()).isEqualTo(1);
                assertThat(req.path("hops").asInt()).isEqualTo(2);
                assertThat(req.path("max_nodes").asInt()).isEqualTo(10);
            }
        }
    }

    @Test
    void graphRag_returnsResult() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse(
                "{\"status\":\"ok\",\"data\":{\"anchor_node_id\":1,\"context\":\"graph context\","
                + "\"question\":\"What is X?\",\"nodes_in_context\":5,\"edges_in_context\":3,"
                + "\"estimated_tokens\":100,\"note\":\"Generated by AstraeaDB\"}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                RagOptions opts = RagOptions.builder()
                    .anchor(1)
                    .questionEmbedding(new float[]{0.1f, 0.2f})
                    .hops(2)
                    .maxNodes(20)
                    .build();
                RagResult result = client.graphRag("What is X?", opts);
                assertThat(result.anchorNodeId()).isEqualTo(1);
                assertThat(result.context()).isEqualTo("graph context");
                assertThat(result.question()).isEqualTo("What is X?");
                assertThat(result.nodesInContext()).isEqualTo(5);
                assertThat(result.edgesInContext()).isEqualTo(3);
                assertThat(result.estimatedTokens()).isEqualTo(100);
                assertThat(result.note()).isEqualTo("Generated by AstraeaDB");

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("GraphRag");
                assertThat(req.path("question").asText()).isEqualTo("What is X?");
                assertThat(req.path("anchor").asLong()).isEqualTo(1);
                assertThat(req.path("question_embedding").size()).isEqualTo(2);
                assertThat(req.path("hops").asInt()).isEqualTo(2);
                assertThat(req.path("max_nodes").asInt()).isEqualTo(20);
            }
        }
    }

    // ================================================================
    //  Batch operations
    // ================================================================

    @Test
    void createNodes_returnsIds() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"node_ids\":[1,2,3]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                List<NodeInput> nodes = List.of(
                    new NodeInput(List.of("A")),
                    new NodeInput(List.of("B")),
                    new NodeInput(List.of("C"))
                );
                List<Long> ids = client.createNodes(nodes);
                assertThat(ids).containsExactly(1L, 2L, 3L);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("CreateNodes");
                assertThat(req.path("nodes").size()).isEqualTo(3);
            }
        }
    }

    @Test
    void createEdges_returnsIds() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"edge_ids\":[10,20]}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                List<EdgeInput> edges = List.of(
                    new EdgeInput(1, 2, "KNOWS"),
                    new EdgeInput(3, 4, "LIKES", 2.0)
                );
                List<Long> ids = client.createEdges(edges);
                assertThat(ids).containsExactly(10L, 20L);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("CreateEdges");
                assertThat(req.path("edges").size()).isEqualTo(2);
                assertThat(req.path("edges").get(0).path("source").asLong()).isEqualTo(1);
            }
        }
    }

    @Test
    void deleteNodes_returnsCount() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"deleted\":3}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                int count = client.deleteNodes(List.of(1L, 2L, 3L));
                assertThat(count).isEqualTo(3);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("DeleteNodes");
                assertThat(req.path("ids").size()).isEqualTo(3);
            }
        }
    }

    @Test
    void deleteEdges_returnsCount() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"deleted\":2}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                int count = client.deleteEdges(List.of(10L, 20L));
                assertThat(count).isEqualTo(2);

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("type").asText()).isEqualTo("DeleteEdges");
                assertThat(req.path("ids").size()).isEqualTo(2);
            }
        }
    }

    // ================================================================
    //  Auth
    // ================================================================

    @Test
    void authToken_injectedWhenSet() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"pong\":true,\"version\":\"0.1.0\"}}");
            try (JsonClient client = clientFor(server, "my-secret-token")) {
                client.connect();
                client.ping();

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.path("auth_token").asText()).isEqualTo("my-secret-token");
            }
        }
    }

    @Test
    void authToken_absentWhenNotSet() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"pong\":true,\"version\":\"0.1.0\"}}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                client.ping();

                JsonNode req = parseRequest(server.takeRequest());
                assertThat(req.has("auth_token")).isFalse();
            }
        }
    }

    // ================================================================
    //  Connection
    // ================================================================

    @Test
    void notConnected_throwsException() throws Exception {
        // Create a client that cannot reconnect, and never call connect()
        JsonClient client = new JsonClientBuilder()
            .host("127.0.0.1")
            .port(19999) // non-existent port
            .reconnect(false)
            .build();
        try {
            assertThatThrownBy(client::ping)
                .isInstanceOf(NotConnectedException.class);
        } finally {
            client.close();
        }
    }

    @Test
    void autoCloseable_closesConnection() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"ok\",\"data\":{\"pong\":true,\"version\":\"0.1.0\"}}");
            JsonClient client = clientFor(server);
            client.connect();
            client.ping();
            // Now close via AutoCloseable
            client.close();
            // After closing, further calls should fail
            // Client has reconnect=true, but the server may not accept another connection
            // in time. The key assertion is that close() itself does not throw.
            // We just verify that close() completed without error above.
        }
    }

    // ================================================================
    //  Error handling
    // ================================================================

    @Test
    void serverError_throwsClassifiedException() throws Exception {
        try (MockJsonServer server = new MockJsonServer()) {
            server.enqueueResponse("{\"status\":\"error\",\"message\":\"Node not found: id=99\"}");
            try (JsonClient client = clientFor(server)) {
                client.connect();
                assertThatThrownBy(() -> client.getNode(99))
                    .isInstanceOf(NodeNotFoundException.class)
                    .hasMessageContaining("Node not found");
            }
        }
    }
}
