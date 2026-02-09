package com.astraeadb.options;

import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class OptionsTest {

    @Test
    void edgeOptionsDefaults() {
        var opts = EdgeOptions.DEFAULT;
        assertThat(opts.weight()).isEqualTo(1.0);
        assertThat(opts.properties()).isEmpty();
        assertThat(opts.validFrom()).isNull();
        assertThat(opts.validTo()).isNull();
    }

    @Test
    void edgeOptionsBuilder() {
        var opts = EdgeOptions.builder()
            .weight(0.5)
            .properties(Map.of("key", "val"))
            .validFrom(100L)
            .validTo(200L)
            .build();
        assertThat(opts.weight()).isEqualTo(0.5);
        assertThat(opts.properties()).containsEntry("key", "val");
        assertThat(opts.validFrom()).isEqualTo(100L);
        assertThat(opts.validTo()).isEqualTo(200L);
    }

    @Test
    void neighborOptionsDefaults() {
        var opts = NeighborOptions.DEFAULT;
        assertThat(opts.direction()).isEqualTo("outgoing");
        assertThat(opts.edgeType()).isNull();
    }

    @Test
    void neighborOptionsBuilder() {
        var opts = NeighborOptions.builder()
            .direction("incoming")
            .edgeType("KNOWS")
            .build();
        assertThat(opts.direction()).isEqualTo("incoming");
        assertThat(opts.edgeType()).isEqualTo("KNOWS");
    }

    @Test
    void hybridSearchDefaults() {
        var opts = HybridSearchOptions.DEFAULT;
        assertThat(opts.maxHops()).isEqualTo(3);
        assertThat(opts.k()).isEqualTo(10);
        assertThat(opts.alpha()).isEqualTo(0.5);
    }

    @Test
    void hybridSearchBuilder() {
        var opts = HybridSearchOptions.builder()
            .maxHops(5).k(20).alpha(0.7).build();
        assertThat(opts.maxHops()).isEqualTo(5);
        assertThat(opts.k()).isEqualTo(20);
        assertThat(opts.alpha()).isEqualTo(0.7);
    }

    @Test
    void semanticOptionsDefaults() {
        assertThat(SemanticOptions.DEFAULT.direction()).isEqualTo("outgoing");
        assertThat(SemanticOptions.DEFAULT.k()).isEqualTo(10);
    }

    @Test
    void subgraphOptionsDefaults() {
        var opts = SubgraphOptions.DEFAULT;
        assertThat(opts.hops()).isEqualTo(3);
        assertThat(opts.maxNodes()).isEqualTo(50);
        assertThat(opts.format()).isEqualTo("structured");
    }

    @Test
    void ragOptionsDefaults() {
        var opts = RagOptions.DEFAULT;
        assertThat(opts.anchor()).isNull();
        assertThat(opts.questionEmbedding()).isNull();
        assertThat(opts.hops()).isEqualTo(3);
        assertThat(opts.maxNodes()).isEqualTo(50);
        assertThat(opts.format()).isEqualTo("structured");
    }

    @Test
    void ragOptionsBuilder() {
        var opts = RagOptions.builder()
            .anchor(42L)
            .questionEmbedding(new float[]{0.1f, 0.2f})
            .hops(5)
            .maxNodes(100)
            .format("prose")
            .build();
        assertThat(opts.anchor()).isEqualTo(42L);
        assertThat(opts.questionEmbedding()).containsExactly(0.1f, 0.2f);
        assertThat(opts.hops()).isEqualTo(5);
        assertThat(opts.maxNodes()).isEqualTo(100);
        assertThat(opts.format()).isEqualTo("prose");
    }
}
