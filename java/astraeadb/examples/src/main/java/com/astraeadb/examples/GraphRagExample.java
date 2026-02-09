package com.astraeadb.examples;

import com.astraeadb.json.JsonClient;
import com.astraeadb.json.JsonClientBuilder;
import com.astraeadb.model.*;
import com.astraeadb.options.*;

import java.util.List;
import java.util.Map;

/**
 * GraphRAG pipeline demo: extract subgraph context, then query with GraphRAG.
 *
 * <p>Builds a small knowledge graph of AI topics, extracts a structured
 * subgraph around the root "Artificial Intelligence" node, and then
 * uses the GraphRAG endpoint to retrieve context for a natural-language
 * question.
 */
public class GraphRagExample {
    public static void main(String[] args) throws Exception {
        try (JsonClient client = new JsonClientBuilder()
                .host("127.0.0.1").port(7687).build()) {

            client.connect();
            System.out.println("Connected to AstraeaDB");

            // Build a small knowledge graph
            long ai = client.createNode(List.of("Topic"),
                Map.of("name", "Artificial Intelligence"),
                new float[]{0.9f, 0.1f, 0.0f});
            long ml = client.createNode(List.of("Topic"),
                Map.of("name", "Machine Learning"),
                new float[]{0.85f, 0.15f, 0.0f});
            long dl = client.createNode(List.of("Topic"),
                Map.of("name", "Deep Learning"),
                new float[]{0.8f, 0.2f, 0.1f});

            client.createEdge(ai, ml, "INCLUDES");
            client.createEdge(ml, dl, "INCLUDES");

            // Extract subgraph as structured context
            SubgraphResult subgraph = client.extractSubgraph(ai,
                SubgraphOptions.builder().hops(2).maxNodes(50).format("structured").build());
            System.out.println("Extracted subgraph:");
            System.out.println("  Nodes: " + subgraph.nodeCount());
            System.out.println("  Edges: " + subgraph.edgeCount());
            System.out.println("  Estimated tokens: " + subgraph.estimatedTokens());
            System.out.println("  Text: " + subgraph.text().substring(0, Math.min(200, subgraph.text().length())));

            // GraphRAG: retrieve context for a question
            RagResult rag = client.graphRag("What is the relationship between AI and Deep Learning?",
                RagOptions.builder()
                    .anchor(ai)
                    .hops(3)
                    .maxNodes(100)
                    .format("prose")
                    .build());
            System.out.println("\nGraphRAG result:");
            System.out.println("  Anchor node: " + rag.anchorNodeId());
            System.out.println("  Nodes in context: " + rag.nodesInContext());
            System.out.println("  Edges in context: " + rag.edgesInContext());
            System.out.println("  Estimated tokens: " + rag.estimatedTokens());
            System.out.println("  Context: " + rag.context().substring(0, Math.min(300, rag.context().length())));

            // Cleanup
            client.deleteNode(ai);
            client.deleteNode(ml);
            client.deleteNode(dl);
            System.out.println("\nDone");
        }
    }
}
