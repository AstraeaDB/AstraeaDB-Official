package com.astraeadb.model;

import java.util.List;
import java.util.Map;

public record NodeInput(
    List<String> labels,
    Map<String, Object> properties,
    float[] embedding
) {
    public NodeInput(List<String> labels, Map<String, Object> properties) {
        this(labels, properties, null);
    }
    public NodeInput(List<String> labels) {
        this(labels, Map.of(), null);
    }
}
