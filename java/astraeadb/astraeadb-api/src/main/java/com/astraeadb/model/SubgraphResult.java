package com.astraeadb.model;

public record SubgraphResult(
    String text,
    int nodeCount,
    int edgeCount,
    int estimatedTokens
) {}
