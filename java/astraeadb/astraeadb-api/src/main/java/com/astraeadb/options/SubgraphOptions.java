package com.astraeadb.options;

public record SubgraphOptions(int hops, int maxNodes, String format) {
    public static final SubgraphOptions DEFAULT = new SubgraphOptions(3, 50, "structured");

    public static Builder builder() { return new Builder(); }

    public static class Builder {
        private int hops = 3;
        private int maxNodes = 50;
        private String format = "structured";

        public Builder hops(int h) { this.hops = h; return this; }
        public Builder maxNodes(int n) { this.maxNodes = n; return this; }
        public Builder format(String f) { this.format = f; return this; }
        public SubgraphOptions build() { return new SubgraphOptions(hops, maxNodes, format); }
    }
}
