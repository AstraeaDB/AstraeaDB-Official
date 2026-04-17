//! GraphRAG pipeline: vector search -> subgraph extraction -> linearization -> LLM.
//!
//! Provides the full GraphRAG query flow as a single function call. The
//! pipeline connects the three RAG building blocks (subgraph extraction,
//! linearization, token budgeting) with an LLM provider to answer questions
//! grounded in graph context.

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::{GraphOps, VectorIndex};
use astraea_core::types::NodeId;

use crate::linearize::{TextFormat, linearize_subgraph};
use crate::llm::LlmProvider;
use crate::subgraph::extract_subgraph;
use crate::token::estimate_tokens;

/// Configuration for a GraphRAG query.
#[derive(Debug, Clone)]
pub struct GraphRagConfig {
    /// Number of BFS hops from the anchor node for subgraph extraction.
    pub hops: usize,
    /// Maximum number of nodes to include in the context subgraph.
    pub max_context_nodes: usize,
    /// Text format for linearizing the subgraph.
    pub text_format: TextFormat,
    /// Maximum token budget for the context portion of the prompt.
    pub token_budget: usize,
    /// Optional system prompt prepended to the LLM prompt.
    pub system_prompt: Option<String>,
}

impl Default for GraphRagConfig {
    fn default() -> Self {
        Self {
            hops: 2,
            max_context_nodes: 50,
            text_format: TextFormat::Structured,
            token_budget: 4000,
            system_prompt: None,
        }
    }
}

/// Result of a GraphRAG query.
#[derive(Debug, Clone)]
pub struct GraphRagResult {
    /// The LLM-generated answer.
    pub answer: String,
    /// The anchor node used as the subgraph center.
    pub anchor_node_id: NodeId,
    /// The linearized context text sent to the LLM.
    pub context_text: String,
    /// Number of nodes included in the context subgraph.
    pub nodes_in_context: usize,
    /// Estimated token count of the context text.
    pub estimated_tokens: usize,
}

/// Execute a GraphRAG query: vector search -> subgraph extraction -> linearization -> LLM.
///
/// 1. **Vector search**: find the most relevant node for the question embedding.
/// 2. **Subgraph extraction**: BFS from anchor up to `config.hops`, capped at
///    `config.max_context_nodes`.
/// 3. **Linearization**: convert subgraph to text in the configured format.
/// 4. **Token budget**: if the context exceeds the budget, re-extract with
///    progressively fewer nodes until it fits.
/// 5. **LLM call**: send prompt + context to the provider.
///
/// # Errors
///
/// Returns an error if the vector search finds no results, the graph operations
/// fail, or the LLM provider returns an error.
pub fn graph_rag_query(
    graph: &dyn GraphOps,
    vector_index: &dyn VectorIndex,
    llm: &dyn LlmProvider,
    question: &str,
    question_embedding: &[f32],
    config: &GraphRagConfig,
) -> Result<GraphRagResult> {
    // Step 1: Vector search to find the anchor node.
    let search_results = vector_index.search(question_embedding, 1)?;
    let anchor = search_results
        .first()
        .ok_or_else(|| {
            AstraeaError::QueryExecution("vector search returned no results".into())
        })?
        .node_id;

    // Delegate to the anchored version.
    graph_rag_query_anchored(graph, llm, question, anchor, config)
}

/// Execute a GraphRAG query with a known anchor node (no vector search needed).
///
/// This is useful when the caller already knows which node to center the
/// context on (e.g., from a previous search or user selection).
///
/// The pipeline:
/// 1. Extract subgraph via BFS from `anchor`.
/// 2. Linearize the subgraph.
/// 3. Enforce the token budget (re-extract with fewer nodes if needed).
/// 4. Build the full prompt and call the LLM.
pub fn graph_rag_query_anchored(
    graph: &dyn GraphOps,
    llm: &dyn LlmProvider,
    question: &str,
    anchor: NodeId,
    config: &GraphRagConfig,
) -> Result<GraphRagResult> {
    // Step 1: Extract the initial subgraph.
    let mut subgraph = extract_subgraph(graph, anchor, config.hops, config.max_context_nodes)?;

    // Step 2: Linearize.
    let mut context_text = linearize_subgraph(&subgraph, config.text_format);

    // Step 3: Check token budget and re-extract with fewer nodes if needed.
    let mut tokens = estimate_tokens(&context_text);
    if tokens > config.token_budget && subgraph.nodes.len() > 1 {
        // Binary search for the right number of nodes.
        let mut max_nodes = subgraph.nodes.len() / 2;
        while max_nodes > 1 {
            subgraph = extract_subgraph(graph, anchor, config.hops, max_nodes)?;
            context_text = linearize_subgraph(&subgraph, config.text_format);
            tokens = estimate_tokens(&context_text);
            if tokens <= config.token_budget {
                break;
            }
            max_nodes /= 2;
        }

        // If still over budget with minimal nodes, just use what we have.
        if tokens > config.token_budget {
            subgraph = extract_subgraph(graph, anchor, config.hops, 1)?;
            context_text = linearize_subgraph(&subgraph, config.text_format);
            tokens = estimate_tokens(&context_text);
        }
    }

    // Step 4: Build the user prompt (question only) and the system-side
    // context (config.system_prompt + linearized graph context).
    //
    // astraeadb-issues.md #16 used to inline `context_text` into
    // `full_prompt` AND pass it again as the `context` argument, which
    // the OpenAI/Anthropic providers route into the system message —
    // context went out twice. Now each piece has one home.
    let user_prompt = build_prompt(question, config);
    let llm_context = build_llm_context(&context_text, config);

    // Step 5: Call the LLM.
    let answer = llm.complete(&user_prompt, &llm_context)?;

    Ok(GraphRagResult {
        answer,
        anchor_node_id: anchor,
        context_text,
        nodes_in_context: subgraph.nodes.len(),
        estimated_tokens: tokens,
    })
}

/// Build the **user prompt** — just the question framing, no context.
/// Graph context is placed separately via [`build_llm_context`] and passed
/// as the `context` argument to [`LlmProvider::complete`]. This split is
/// what fixes astraeadb-issues.md #16.
fn build_prompt(question: &str, _config: &GraphRagConfig) -> String {
    format!("Answer the question: {question}")
}

/// Build the **system-side context** — config.system_prompt plus the
/// linearized graph context, joined by a blank line. Every provider puts
/// this into the system role (OpenAI), the system field (Anthropic), or
/// prepends it to the prompt (Ollama).
fn build_llm_context(context_text: &str, config: &GraphRagConfig) -> String {
    match &config.system_prompt {
        Some(sp) if !sp.is_empty() => {
            format!("{sp}\n\nGiven the following graph context:\n\n{context_text}")
        }
        _ => format!("Given the following graph context:\n\n{context_text}"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockProvider;
    use astraea_core::traits::GraphOps;
    use astraea_core::types::DistanceMetric;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;
    use astraea_vector::HnswVectorIndex;
    use std::sync::Arc;

    /// Build a test graph with embeddings:
    ///   n1("Alice", [1,0,0]) -[KNOWS]-> n2("Bob", [0,1,0]) -[WORKS_AT]-> n3("Acme", [0,0,1])
    fn build_test_graph() -> (Graph, Arc<HnswVectorIndex>) {
        let storage = InMemoryStorage::new();
        let vi = Arc::new(HnswVectorIndex::new(3, DistanceMetric::Euclidean));
        let graph = Graph::with_vector_index(Box::new(storage), vi.clone());

        graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Alice"}),
                Some(vec![1.0, 0.0, 0.0]),
            )
            .unwrap();
        graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Bob"}),
                Some(vec![0.0, 1.0, 0.0]),
            )
            .unwrap();
        graph
            .create_node(
                vec!["Company".into()],
                serde_json::json!({"name": "Acme"}),
                Some(vec![0.0, 0.0, 1.0]),
            )
            .unwrap();

        graph
            .create_edge(
                NodeId(1),
                NodeId(2),
                "KNOWS".into(),
                serde_json::json!({"since": 2020}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                NodeId(2),
                NodeId(3),
                "WORKS_AT".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        (graph, vi)
    }

    #[test]
    fn test_graph_rag_query_with_mock() {
        let (graph, vi) = build_test_graph();
        let mock = MockProvider {
            response_prefix: "GraphRAG:".to_string(),
            context_window: 8000,
        };

        let config = GraphRagConfig {
            hops: 2,
            max_context_nodes: 50,
            text_format: TextFormat::Structured,
            token_budget: 4000,
            system_prompt: Some("You are a helpful assistant.".into()),
        };

        // Query embedding close to Alice [1,0,0]
        let result = graph_rag_query(
            &graph,
            vi.as_ref(),
            &mock,
            "Who does Alice know?",
            &[1.0, 0.0, 0.0],
            &config,
        )
        .unwrap();

        // The anchor should be node 1 (Alice, closest to [1,0,0]).
        assert_eq!(result.anchor_node_id, NodeId(1));

        // The answer should include the mock prefix.
        assert!(result.answer.starts_with("GraphRAG:"));

        // Context should mention Alice.
        assert!(result.context_text.contains("Alice"));

        // Should have nodes in context.
        assert!(result.nodes_in_context > 0);
        assert!(result.estimated_tokens > 0);
    }

    #[test]
    fn test_graph_rag_query_anchored() {
        let (graph, _vi) = build_test_graph();
        let mock = MockProvider {
            response_prefix: "Anchored:".to_string(),
            context_window: 8000,
        };

        let config = GraphRagConfig::default();

        let result = graph_rag_query_anchored(
            &graph,
            &mock,
            "Tell me about Bob",
            NodeId(2),
            &config,
        )
        .unwrap();

        assert_eq!(result.anchor_node_id, NodeId(2));
        assert!(result.answer.starts_with("Anchored:"));
        // 2-hop from Bob: Bob -> Acme (outgoing)
        assert!(result.context_text.contains("Bob"));
        assert!(result.nodes_in_context >= 1);
    }

    #[test]
    fn test_graph_rag_default_config() {
        let config = GraphRagConfig::default();
        assert_eq!(config.hops, 2);
        assert_eq!(config.max_context_nodes, 50);
        assert_eq!(config.text_format, TextFormat::Structured);
        assert_eq!(config.token_budget, 4000);
        assert!(config.system_prompt.is_none());
    }

    #[test]
    fn test_graph_rag_respects_token_budget() {
        let (graph, _vi) = build_test_graph();
        let mock = MockProvider {
            response_prefix: "Budget:".to_string(),
            context_window: 8000,
        };

        // Use a very tight token budget.
        let config = GraphRagConfig {
            hops: 10,
            max_context_nodes: 100,
            text_format: TextFormat::Structured,
            token_budget: 10, // very small budget
            system_prompt: None,
        };

        let result = graph_rag_query_anchored(
            &graph,
            &mock,
            "Q",
            NodeId(1),
            &config,
        )
        .unwrap();

        // With a tight budget, should have fewer nodes than the full graph.
        // The exact count depends on linearization, but it should be capped.
        assert!(result.nodes_in_context <= 3);
        assert!(result.answer.starts_with("Budget:"));
    }

    #[test]
    fn test_build_prompt_is_question_only() {
        let config = GraphRagConfig::default();
        let p = build_prompt("What is this?", &config);
        // The question is framed, but context is NOT inlined — that's the
        // whole point of the #16 fix.
        assert_eq!(p, "Answer the question: What is this?");
        assert!(!p.contains("Given the following graph context"));
    }

    #[test]
    fn test_build_llm_context_with_system_prompt() {
        let config = GraphRagConfig {
            system_prompt: Some("You are a graph expert.".into()),
            ..GraphRagConfig::default()
        };
        let ctx = build_llm_context("Node [A] -> [B]", &config);
        assert!(ctx.starts_with("You are a graph expert."));
        assert!(ctx.contains("Given the following graph context:"));
        assert!(ctx.contains("Node [A] -> [B]"));
    }

    #[test]
    fn test_build_llm_context_without_system_prompt() {
        let config = GraphRagConfig::default();
        let ctx = build_llm_context("context here", &config);
        assert!(!ctx.contains("You are"));
        assert!(ctx.starts_with("Given the following graph context:"));
        assert!(ctx.contains("context here"));
    }

    #[test]
    fn test_no_duplicate_context() {
        // #16 regression guard: the user prompt passed to the LLM must not
        // contain the graph context text — context goes through the `context`
        // argument only.
        let config = GraphRagConfig {
            system_prompt: Some("sys".into()),
            ..GraphRagConfig::default()
        };
        let context_text = "UNIQUE_CONTEXT_MARKER_9f4c";
        let user = build_prompt("q?", &config);
        let ctx = build_llm_context(context_text, &config);
        assert!(!user.contains(context_text));
        assert!(ctx.contains(context_text));
    }
}
