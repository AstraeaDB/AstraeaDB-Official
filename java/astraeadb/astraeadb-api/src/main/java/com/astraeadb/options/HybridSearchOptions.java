package com.astraeadb.options;

public record HybridSearchOptions(int maxHops, int k, double alpha) {
    public static final HybridSearchOptions DEFAULT = new HybridSearchOptions(3, 10, 0.5);

    public static Builder builder() { return new Builder(); }

    public static class Builder {
        private int maxHops = 3;
        private int k = 10;
        private double alpha = 0.5;

        public Builder maxHops(int h) { this.maxHops = h; return this; }
        public Builder k(int k) { this.k = k; return this; }
        public Builder alpha(double a) { this.alpha = a; return this; }
        public HybridSearchOptions build() { return new HybridSearchOptions(maxHops, k, alpha); }
    }
}
