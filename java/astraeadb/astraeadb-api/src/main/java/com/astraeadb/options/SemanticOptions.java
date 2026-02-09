package com.astraeadb.options;

public record SemanticOptions(String direction, int k) {
    public static final SemanticOptions DEFAULT = new SemanticOptions("outgoing", 10);

    public static Builder builder() { return new Builder(); }

    public static class Builder {
        private String direction = "outgoing";
        private int k = 10;

        public Builder direction(String d) { this.direction = d; return this; }
        public Builder k(int k) { this.k = k; return this; }
        public SemanticOptions build() { return new SemanticOptions(direction, k); }
    }
}
