package com.astraeadb.model;

import java.util.List;

public record PathResult(
    boolean found,
    List<Long> path,
    int length,
    Double cost
) {}
