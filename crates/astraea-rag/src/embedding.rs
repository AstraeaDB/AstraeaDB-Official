//! Embedding provider abstraction for AstraeaDB RAG.
//!
//! Provides a trait for text-embedding providers and a [`MockEmbedder`] for
//! testing. The concrete [`OllamaProvider`](crate::llm::OllamaProvider)
//! implementation lives in [`crate::llm`] alongside the chat provider.
//!
//! # Design notes
//!
//! - The trait is **synchronous** to match [`LlmProvider`](crate::llm::LlmProvider).
//!   Both traits use an injected HTTP-callback pattern; there is no async
//!   runtime requirement in this crate.
//! - Error type is [`AstraeaError`] (reusing `QueryExecution` and `Config`
//!   variants). No separate `EmbedError` is introduced.
//! - `embedding_dim()` returns the value the provider was configured with at
//!   construction time. It does not make a live API call.

use astraea_core::error::Result;

/// Trait for text-embedding providers.
///
/// Implementations must be `Send + Sync` so they can be shared across threads.
/// Like [`LlmProvider`](crate::llm::LlmProvider), implementations carry no
/// HTTP client; callers inject an HTTP handler via a setter method on the
/// concrete type.
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a batch of texts into fixed-dimensional vectors.
    ///
    /// Returns one vector per input text in the same order. The length of each
    /// inner `Vec<f32>` equals [`embedding_dim`](EmbeddingProvider::embedding_dim).
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

    /// Dimension of the vectors this provider returns.
    ///
    /// Callers use this to decide whether truncation is needed — for example,
    /// `embeddinggemma` returns 768-dimensional vectors while the current HNSW
    /// index may be configured for 128 dimensions.
    fn embedding_dim(&self) -> usize;
}

// ---------------------------------------------------------------------------
// MockEmbedder
// ---------------------------------------------------------------------------

/// A mock embedding provider for testing.
///
/// Returns canned vectors (all zeros, padded or truncated to `dim`). Useful
/// for verifying that the trait compiles and can be stored as a trait object.
pub struct MockEmbedder {
    /// The dimension each returned vector will have.
    pub dim: usize,
    /// Canned values to fill each vector with (defaults to `0.0` if empty).
    pub fill: f32,
}

impl MockEmbedder {
    /// Create a `MockEmbedder` that returns zero vectors of the given dimension.
    pub fn zeros(dim: usize) -> Self {
        Self { dim, fill: 0.0 }
    }
}

impl EmbeddingProvider for MockEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![self.fill; self.dim]).collect())
    }

    fn embedding_dim(&self) -> usize {
        self.dim
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies the trait compiles and is object-safe (stored as `Box<dyn EmbeddingProvider>`).
    #[test]
    fn test_mock_embedder_trait_shape() {
        let embedder: Box<dyn EmbeddingProvider> = Box::new(MockEmbedder::zeros(4));

        let texts = &["hello", "world"];
        let result = embedder.embed(texts).unwrap();

        assert_eq!(result.len(), 2, "one vector per input text");
        assert_eq!(result[0].len(), 4, "vector length equals dim");
        assert_eq!(result[1].len(), 4);
        assert!(
            result[0].iter().all(|&v| v == 0.0),
            "zero fill expected"
        );
    }

    #[test]
    fn test_mock_embedder_dim() {
        let embedder = MockEmbedder { dim: 768, fill: 1.0 };
        assert_eq!(embedder.embedding_dim(), 768);

        let result = embedder.embed(&["test"]).unwrap();
        assert_eq!(result[0].len(), 768);
        assert!(result[0].iter().all(|&v| v == 1.0));
    }

    #[test]
    fn test_mock_embedder_empty_batch() {
        let embedder = MockEmbedder::zeros(32);
        let result = embedder.embed(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_embed_error_is_astraea_error() {
        // Ensure EmbeddingProvider::embed returns AstraeaError (type-check only).
        fn takes_provider(p: &dyn EmbeddingProvider) -> Result<Vec<Vec<f32>>> {
            p.embed(&["a"])
        }

        let embedder = MockEmbedder::zeros(8);
        let r: Result<Vec<Vec<f32>>> = takes_provider(&embedder);
        assert!(r.is_ok());
    }
}
