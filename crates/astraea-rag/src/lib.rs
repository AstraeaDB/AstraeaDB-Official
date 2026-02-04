//! AstraeaDB RAG (Retrieval-Augmented Generation) crate.
//!
//! Provides subgraph extraction and linearization for feeding graph context
//! into LLM prompts. This is the foundation of the GraphRAG engine:
//!
//! 1. **Subgraph extraction** -- BFS-based or semantic (vector-guided) extraction
//!    of a local neighborhood around a node.
//! 2. **Linearization** -- Converting a subgraph into a textual representation
//!    suitable for inclusion in an LLM context window.
//! 3. **Token budget** -- Estimating token counts and capping extraction to fit
//!    within a given token budget.
//! 4. **LLM providers** -- Pluggable LLM provider abstraction (OpenAI, Anthropic,
//!    Ollama) with no baked-in HTTP dependencies.
//! 5. **GraphRAG pipeline** -- End-to-end pipeline: vector search -> subgraph
//!    extraction -> linearization -> LLM completion.

pub mod linearize;
pub mod llm;
pub mod pipeline;
pub mod subgraph;
pub mod token;

pub use linearize::{TextFormat, linearize_subgraph};
pub use llm::{
    AnthropicProvider, LlmConfig, LlmProvider, MockProvider, OllamaProvider, OpenAiProvider,
    ProviderType,
};
pub use pipeline::{GraphRagConfig, GraphRagResult, graph_rag_query, graph_rag_query_anchored};
pub use subgraph::{Subgraph, extract_subgraph, extract_subgraph_semantic};
pub use token::{estimate_tokens, extract_with_budget};
