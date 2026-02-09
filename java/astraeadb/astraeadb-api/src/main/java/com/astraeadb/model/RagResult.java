package com.astraeadb.model;

public record RagResult(
    long anchorNodeId,
    String context,
    String question,
    int nodesInContext,
    int edgesInContext,
    int estimatedTokens,
    String note
) {}
