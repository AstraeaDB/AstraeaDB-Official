package com.astraeadb.model;

import java.util.List;
import java.util.Map;

public record Node(
    long id,
    List<String> labels,
    Map<String, Object> properties,
    boolean hasEmbedding
) {}
