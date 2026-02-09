package com.astraeadb.examples;

import com.astraeadb.json.JsonClient;
import com.astraeadb.json.JsonClientBuilder;
import com.astraeadb.model.*;
import com.astraeadb.options.*;

import java.util.List;
import java.util.Map;

/**
 * Vector search and hybrid search demo for AstraeaDB.
 *
 * <p>Creates three concept nodes with embeddings, links related concepts,
 * then demonstrates k-NN vector search, hybrid search (combining graph
 * proximity with vector similarity), and semantic neighbor discovery.
 */
public class VectorSearchExample {
    public static void main(String[] args) throws Exception {
        try (JsonClient client = new JsonClientBuilder()
                .host("127.0.0.1").port(7687).build()) {

            client.connect();
            System.out.println("Connected to AstraeaDB");

            // Create nodes with embeddings
            long n1 = client.createNode(List.of("Concept"), Map.of("title", "Machine Learning"),
                new float[]{0.9f, 0.1f, 0.2f});
            long n2 = client.createNode(List.of("Concept"), Map.of("title", "Deep Learning"),
                new float[]{0.85f, 0.15f, 0.25f});
            long n3 = client.createNode(List.of("Concept"), Map.of("title", "Cooking"),
                new float[]{0.1f, 0.9f, 0.1f});

            // Create edges
            client.createEdge(n1, n2, "RELATED_TO", 0.95);

            // Vector search (k-NN)
            float[] queryVec = {0.88f, 0.12f, 0.22f};
            List<SearchResult> results = client.vectorSearch(queryVec, 5);
            System.out.println("Vector search results:");
            for (SearchResult r : results) {
                System.out.println("  Node " + r.nodeId() + " distance=" + r.distance());
            }

            // Hybrid search (graph + vector)
            List<SearchResult> hybrid = client.hybridSearch(n1, queryVec,
                HybridSearchOptions.builder().maxHops(2).k(10).alpha(0.5).build());
            System.out.println("Hybrid search results: " + hybrid.size());

            // Semantic neighbors
            float[] concept = {0.9f, 0.1f, 0.2f};
            List<SearchResult> semantic = client.semanticNeighbors(n1, concept);
            System.out.println("Semantic neighbors: " + semantic.size());

            // Cleanup
            client.deleteNode(n1);
            client.deleteNode(n2);
            client.deleteNode(n3);
            System.out.println("Done");
        }
    }
}
