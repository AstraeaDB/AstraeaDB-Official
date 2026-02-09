package com.astraeadb.model;

import java.util.List;

public record QueryResult(
    List<String> columns,
    List<List<Object>> rows,
    QueryStats stats
) {
    public record QueryStats(
        long nodesCreated,
        long edgesCreated,
        long nodesDeleted,
        long edgesDeleted
    ) {
        public static final QueryStats EMPTY = new QueryStats(0, 0, 0, 0);
    }
}
