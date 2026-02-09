package com.astraeadb.options;

public record RagOptions(
    Long anchor,
    float[] questionEmbedding,
    int hops,
    int maxNodes,
    String format
) {
    public static final RagOptions DEFAULT = new RagOptions(null, null, 3, 50, "structured");

    public static Builder builder() { return new Builder(); }

    public static class Builder {
        private Long anchor;
        private float[] questionEmbedding;
        private int hops = 3;
        private int maxNodes = 50;
        private String format = "structured";

        public Builder anchor(long a) { this.anchor = a; return this; }
        public Builder questionEmbedding(float[] e) { this.questionEmbedding = e; return this; }
        public Builder hops(int h) { this.hops = h; return this; }
        public Builder maxNodes(int n) { this.maxNodes = n; return this; }
        public Builder format(String f) { this.format = f; return this; }
        public RagOptions build() { return new RagOptions(anchor, questionEmbedding, hops, maxNodes, format); }
    }
}
