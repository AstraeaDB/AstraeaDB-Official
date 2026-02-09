package com.astraeadb.options;

import java.util.Map;

public record EdgeOptions(
    Map<String, Object> properties,
    double weight,
    Long validFrom,
    Long validTo
) {
    public static final EdgeOptions DEFAULT = new EdgeOptions(Map.of(), 1.0, null, null);

    public static Builder builder() { return new Builder(); }

    public static class Builder {
        private Map<String, Object> properties = Map.of();
        private double weight = 1.0;
        private Long validFrom;
        private Long validTo;

        public Builder properties(Map<String, Object> p) { this.properties = p; return this; }
        public Builder weight(double w) { this.weight = w; return this; }
        public Builder validFrom(long t) { this.validFrom = t; return this; }
        public Builder validTo(long t) { this.validTo = t; return this; }
        public EdgeOptions build() { return new EdgeOptions(properties, weight, validFrom, validTo); }
    }
}
