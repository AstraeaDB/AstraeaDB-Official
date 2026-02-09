package com.astraeadb.model;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class ModelSerializationTest {

    private final ObjectMapper mapper = new ObjectMapper();

    @Test
    void nodeRoundTrip() throws Exception {
        String json = """
            {"id":42,"labels":["Person"],"properties":{"name":"Alice","age":30},"hasEmbedding":true}
            """;
        JsonNode tree = mapper.readTree(json);
        assertThat(tree.path("id").asLong()).isEqualTo(42L);
        assertThat(tree.path("labels").get(0).asText()).isEqualTo("Person");
        assertThat(tree.path("hasEmbedding").asBoolean()).isTrue();

        Node node = new Node(42, List.of("Person"), Map.of("name", "Alice", "age", 30), true);
        assertThat(node.id()).isEqualTo(42L);
        assertThat(node.labels()).containsExactly("Person");
        assertThat(node.hasEmbedding()).isTrue();
    }

    @Test
    void edgeRoundTrip() throws Exception {
        Edge edge = new Edge(1, 10, 20, "KNOWS", Map.of("since", 2020), 0.9, 1000L, 2000L);
        assertThat(edge.id()).isEqualTo(1);
        assertThat(edge.source()).isEqualTo(10);
        assertThat(edge.target()).isEqualTo(20);
        assertThat(edge.edgeType()).isEqualTo("KNOWS");
        assertThat(edge.weight()).isEqualTo(0.9);
        assertThat(edge.validFrom()).isEqualTo(1000L);
        assertThat(edge.validTo()).isEqualTo(2000L);
    }

    @Test
    void pathResultNullCost() {
        PathResult result = new PathResult(true, List.of(1L, 2L, 3L), 2, null);
        assertThat(result.found()).isTrue();
        assertThat(result.path()).containsExactly(1L, 2L, 3L);
        assertThat(result.length()).isEqualTo(2);
        assertThat(result.cost()).isNull();
    }

    @Test
    void queryResultWithStats() {
        var stats = new QueryResult.QueryStats(1, 2, 0, 0);
        var result = new QueryResult(
            List.of("name", "age"),
            List.of(List.of("Alice", 30), List.of("Bob", 25)),
            stats
        );
        assertThat(result.columns()).containsExactly("name", "age");
        assertThat(result.rows()).hasSize(2);
        assertThat(result.stats().nodesCreated()).isEqualTo(1);
    }

    @Test
    void ragResultFields() {
        var rag = new RagResult(42, "context text", "What is?", 5, 10, 150, "Use LLM");
        assertThat(rag.anchorNodeId()).isEqualTo(42);
        assertThat(rag.context()).isEqualTo("context text");
        assertThat(rag.question()).isEqualTo("What is?");
        assertThat(rag.nodesInContext()).isEqualTo(5);
        assertThat(rag.estimatedTokens()).isEqualTo(150);
    }

    @Test
    void nodeInputConvenienceConstructors() {
        var full = new NodeInput(List.of("Person"), Map.of("name", "Alice"), new float[]{0.1f, 0.2f});
        assertThat(full.embedding()).isNotNull();

        var noEmbed = new NodeInput(List.of("Person"), Map.of("name", "Bob"));
        assertThat(noEmbed.embedding()).isNull();
        assertThat(noEmbed.properties()).containsEntry("name", "Bob");

        var minimal = new NodeInput(List.of("Thing"));
        assertThat(minimal.properties()).isEmpty();
        assertThat(minimal.embedding()).isNull();
    }

    @Test
    void edgeInputConvenienceConstructors() {
        var full = new EdgeInput(1, 2, "KNOWS", Map.of("since", 2020), 0.5, 100L, 200L);
        assertThat(full.weight()).isEqualTo(0.5);
        assertThat(full.validFrom()).isEqualTo(100L);

        var minimal = new EdgeInput(1, 2, "KNOWS");
        assertThat(minimal.weight()).isEqualTo(1.0);
        assertThat(minimal.properties()).isEmpty();
        assertThat(minimal.validFrom()).isNull();

        var weighted = new EdgeInput(1, 2, "KNOWS", 0.8);
        assertThat(weighted.weight()).isEqualTo(0.8);
    }
}
