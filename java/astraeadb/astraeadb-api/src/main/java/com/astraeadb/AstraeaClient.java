package com.astraeadb;

import com.astraeadb.exception.AstraeaException;
import com.astraeadb.model.*;
import com.astraeadb.options.*;

import java.util.List;
import java.util.Map;

/**
 * Primary interface for all AstraeaDB client implementations.
 * Supports all 22 server operations across JSON/TCP, gRPC, and Arrow Flight transports.
 */
public interface AstraeaClient extends AutoCloseable {

    void connect() throws AstraeaException;

    @Override
    void close() throws AstraeaException;

    // Health
    PingResponse ping() throws AstraeaException;

    // Node CRUD
    long createNode(List<String> labels, Map<String, Object> properties, float[] embedding) throws AstraeaException;
    default long createNode(List<String> labels, Map<String, Object> properties) throws AstraeaException {
        return createNode(labels, properties, null);
    }
    default long createNode(List<String> labels) throws AstraeaException {
        return createNode(labels, Map.of(), null);
    }
    Node getNode(long id) throws AstraeaException;
    void updateNode(long id, Map<String, Object> properties) throws AstraeaException;
    void deleteNode(long id) throws AstraeaException;

    // Edge CRUD
    long createEdge(long source, long target, String edgeType, EdgeOptions options) throws AstraeaException;
    default long createEdge(long source, long target, String edgeType) throws AstraeaException {
        return createEdge(source, target, edgeType, EdgeOptions.DEFAULT);
    }
    default long createEdge(long source, long target, String edgeType, double weight) throws AstraeaException {
        return createEdge(source, target, edgeType, EdgeOptions.builder().weight(weight).build());
    }
    Edge getEdge(long id) throws AstraeaException;
    void updateEdge(long id, Map<String, Object> properties) throws AstraeaException;
    void deleteEdge(long id) throws AstraeaException;

    // Traversal
    List<NeighborEntry> neighbors(long id, NeighborOptions options) throws AstraeaException;
    default List<NeighborEntry> neighbors(long id) throws AstraeaException {
        return neighbors(id, NeighborOptions.DEFAULT);
    }
    List<BfsEntry> bfs(long start, int maxDepth) throws AstraeaException;
    default List<BfsEntry> bfs(long start) throws AstraeaException {
        return bfs(start, 3);
    }
    PathResult shortestPath(long from, long to, boolean weighted) throws AstraeaException;
    default PathResult shortestPath(long from, long to) throws AstraeaException {
        return shortestPath(from, to, false);
    }

    // Temporal queries
    List<NeighborEntry> neighborsAt(long id, String direction, long timestamp) throws AstraeaException;
    List<NeighborEntry> neighborsAt(long id, String direction, long timestamp, String edgeType) throws AstraeaException;
    List<BfsEntry> bfsAt(long start, int maxDepth, long timestamp) throws AstraeaException;
    PathResult shortestPathAt(long from, long to, long timestamp, boolean weighted) throws AstraeaException;

    // Vector & semantic search
    List<SearchResult> vectorSearch(float[] query, int k) throws AstraeaException;
    default List<SearchResult> vectorSearch(float[] query) throws AstraeaException {
        return vectorSearch(query, 10);
    }
    List<SearchResult> hybridSearch(long anchor, float[] query, HybridSearchOptions options) throws AstraeaException;
    default List<SearchResult> hybridSearch(long anchor, float[] query) throws AstraeaException {
        return hybridSearch(anchor, query, HybridSearchOptions.DEFAULT);
    }
    List<SearchResult> semanticNeighbors(long id, float[] concept, SemanticOptions options) throws AstraeaException;
    default List<SearchResult> semanticNeighbors(long id, float[] concept) throws AstraeaException {
        return semanticNeighbors(id, concept, SemanticOptions.DEFAULT);
    }
    List<WalkStep> semanticWalk(long start, float[] concept, int maxHops) throws AstraeaException;
    default List<WalkStep> semanticWalk(long start, float[] concept) throws AstraeaException {
        return semanticWalk(start, concept, 3);
    }

    // GQL query
    QueryResult query(String gql) throws AstraeaException;

    // GraphRAG
    SubgraphResult extractSubgraph(long center, SubgraphOptions options) throws AstraeaException;
    default SubgraphResult extractSubgraph(long center) throws AstraeaException {
        return extractSubgraph(center, SubgraphOptions.DEFAULT);
    }
    RagResult graphRag(String question, RagOptions options) throws AstraeaException;
    default RagResult graphRag(String question) throws AstraeaException {
        return graphRag(question, RagOptions.DEFAULT);
    }

    // Batch operations
    List<Long> createNodes(List<NodeInput> nodes) throws AstraeaException;
    List<Long> createEdges(List<EdgeInput> edges) throws AstraeaException;
    int deleteNodes(List<Long> ids) throws AstraeaException;
    int deleteEdges(List<Long> ids) throws AstraeaException;
}
