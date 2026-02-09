package com.astraeadb.model;

import java.util.Map;

public record EdgeInput(
    long source,
    long target,
    String edgeType,
    Map<String, Object> properties,
    double weight,
    Long validFrom,
    Long validTo
) {
    public EdgeInput(long source, long target, String edgeType) {
        this(source, target, edgeType, Map.of(), 1.0, null, null);
    }
    public EdgeInput(long source, long target, String edgeType, double weight) {
        this(source, target, edgeType, Map.of(), weight, null, null);
    }
}
