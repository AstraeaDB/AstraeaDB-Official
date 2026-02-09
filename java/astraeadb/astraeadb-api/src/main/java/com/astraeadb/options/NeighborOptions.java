package com.astraeadb.options;

public record NeighborOptions(String direction, String edgeType) {
    public static final NeighborOptions DEFAULT = new NeighborOptions("outgoing", null);

    public static Builder builder() { return new Builder(); }

    public static class Builder {
        private String direction = "outgoing";
        private String edgeType;

        public Builder direction(String d) { this.direction = d; return this; }
        public Builder edgeType(String t) { this.edgeType = t; return this; }
        public NeighborOptions build() { return new NeighborOptions(direction, edgeType); }
    }
}
