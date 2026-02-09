package com.astraeadb.model;

import java.util.Map;

public record Edge(
    long id,
    long source,
    long target,
    String edgeType,
    Map<String, Object> properties,
    double weight,
    Long validFrom,
    Long validTo
) {}
