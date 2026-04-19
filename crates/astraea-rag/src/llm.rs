//! LLM provider abstraction for GraphRAG.
//!
//! Provides a trait for LLM providers and concrete implementations for
//! OpenAI, Anthropic, and Ollama APIs. No HTTP dependencies are included
//! in this crate; instead, users inject their own HTTP handler via a
//! callback function.
//!
//! A [`MockProvider`] is included for testing.
//!
//! See also [`crate::embedding`] for the [`EmbeddingProvider`](crate::embedding::EmbeddingProvider)
//! trait and the [`OllamaProvider`] embedding implementation.

use astraea_core::error::{AstraeaError, Result};
use crate::embedding::EmbeddingProvider;

/// Configuration for an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// The provider type (OpenAI, Anthropic, Ollama, Mock).
    pub provider: ProviderType,
    /// The model identifier (e.g., "gpt-4o", "claude-sonnet-4-20250514", "llama3").
    pub model: String,
    /// API key for authentication (not required for Ollama or Mock).
    pub api_key: Option<String>,
    /// The API endpoint URL.
    pub endpoint: String,
    /// Sampling temperature (0.0 = deterministic, 1.0 = creative).
    pub temperature: f32,
    /// Maximum tokens to generate in the response.
    pub max_tokens: usize,
}

/// Supported LLM provider types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderType {
    /// OpenAI-compatible API (also works with Azure OpenAI, vLLM, etc.).
    OpenAi,
    /// Anthropic Messages API.
    Anthropic,
    /// Local Ollama instance.
    Ollama,
    /// Mock provider for testing.
    Mock,
}

/// Trait for LLM providers.
///
/// Implementations must be `Send + Sync` so they can be shared across threads.
pub trait LlmProvider: Send + Sync {
    /// Generate a completion given a prompt and context.
    ///
    /// The `prompt` is the fully assembled prompt including system instructions,
    /// graph context, and the user question. The `context` parameter is provided
    /// separately for logging and debugging purposes.
    fn complete(&self, prompt: &str, context: &str) -> Result<String>;

    /// Maximum number of tokens this provider will emit in a single
    /// completion (the *output* cap, typically 4096–16k for modern models).
    /// astraeadb-issues.md #17 — this was named `context_window_tokens`;
    /// the old name promised the input budget, which it never delivered.
    fn max_output_tokens(&self) -> usize;

    /// Maximum tokens the model accepts in a single prompt (the *input*
    /// window, typically 128k+ for modern models). Callers sizing
    /// retrieval budgets should use this, not `max_output_tokens`.
    /// Default: 32k as a conservative assumption.
    fn input_context_tokens(&self) -> usize {
        32_000
    }

    /// The provider name for logging.
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// MockProvider
// ---------------------------------------------------------------------------

/// A mock LLM provider for testing.
///
/// Returns a canned response that includes the prompt and context length,
/// allowing tests to verify that the pipeline correctly assembles prompts.
pub struct MockProvider {
    /// Prefix for the canned response.
    pub response_prefix: String,
    /// Reported context window size.
    pub context_window: usize,
}

impl LlmProvider for MockProvider {
    fn complete(&self, prompt: &str, context: &str) -> Result<String> {
        Ok(format!(
            "{} [{}] Context had {} chars.",
            self.response_prefix,
            prompt,
            context.len()
        ))
    }

    fn max_output_tokens(&self) -> usize {
        self.context_window
    }

    fn input_context_tokens(&self) -> usize {
        self.context_window
    }

    fn name(&self) -> &str {
        "mock"
    }
}

// ---------------------------------------------------------------------------
// OpenAiProvider
// ---------------------------------------------------------------------------

/// An OpenAI-compatible LLM provider.
///
/// This provider does not include an HTTP client. Instead, callers inject
/// a callback via [`with_http_fn`](OpenAiProvider::with_http_fn) that
/// performs the actual HTTP request. If no callback is set, [`complete`]
/// returns an error.
///
/// The callback receives `(endpoint_url, request_body_json)` and must
/// return the raw response body as a string.
pub struct OpenAiProvider {
    config: LlmConfig,
    /// Optional HTTP handler for actual API calls.
    http_fn: Option<Box<dyn Fn(&str, &serde_json::Value) -> Result<String> + Send + Sync>>,
}

impl OpenAiProvider {
    /// Create a new OpenAI provider with the given configuration.
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            http_fn: None,
        }
    }

    /// Set a custom HTTP handler for API calls.
    ///
    /// The handler receives `(endpoint_url, request_body_json)` and returns
    /// the response body as a string.
    pub fn with_http_fn(
        mut self,
        f: impl Fn(&str, &serde_json::Value) -> Result<String> + Send + Sync + 'static,
    ) -> Self {
        self.http_fn = Some(Box::new(f));
        self
    }

    /// Build the OpenAI-compatible chat completions request body.
    fn build_request(&self, prompt: &str, context: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "system",
                    "content": context
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens
        })
    }

    /// Parse the response from the OpenAI API to extract the completion text.
    fn parse_response(response_body: &str) -> Result<String> {
        let parsed: serde_json::Value = serde_json::from_str(response_body).map_err(|e| {
            AstraeaError::QueryExecution(format!("failed to parse OpenAI response: {e}"))
        })?;

        parsed["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                AstraeaError::QueryExecution(
                    "OpenAI response missing choices[0].message.content".into(),
                )
            })
    }
}

impl LlmProvider for OpenAiProvider {
    fn complete(&self, prompt: &str, context: &str) -> Result<String> {
        let http_fn = self.http_fn.as_ref().ok_or_else(|| {
            AstraeaError::Config(
                "OpenAI provider has no HTTP handler configured. \
                 Call with_http_fn() to provide an HTTP client callback."
                    .into(),
            )
        })?;

        let url = format!("{}/chat/completions", self.config.endpoint.trim_end_matches('/'));
        let body = self.build_request(prompt, context);
        let response_body = http_fn(&url, &body)?;
        Self::parse_response(&response_body)
    }

    fn max_output_tokens(&self) -> usize {
        self.config.max_tokens
    }

    fn input_context_tokens(&self) -> usize {
        // Conservative — most current OpenAI-compatible models accept 128k+,
        // but we don't know which model the config names.
        128_000
    }

    fn name(&self) -> &str {
        "openai"
    }
}

// ---------------------------------------------------------------------------
// AnthropicProvider
// ---------------------------------------------------------------------------

/// An Anthropic Messages API provider.
///
/// Like [`OpenAiProvider`], this does not include an HTTP client. Callers
/// must inject a callback via [`with_http_fn`](AnthropicProvider::with_http_fn).
pub struct AnthropicProvider {
    config: LlmConfig,
    /// Optional HTTP handler for actual API calls.
    http_fn: Option<Box<dyn Fn(&str, &serde_json::Value) -> Result<String> + Send + Sync>>,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider with the given configuration.
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            http_fn: None,
        }
    }

    /// Set a custom HTTP handler for API calls.
    ///
    /// The handler receives `(endpoint_url, request_body_json)` and returns
    /// the response body as a string.
    pub fn with_http_fn(
        mut self,
        f: impl Fn(&str, &serde_json::Value) -> Result<String> + Send + Sync + 'static,
    ) -> Self {
        self.http_fn = Some(Box::new(f));
        self
    }

    /// Build the Anthropic Messages API request body.
    fn build_request(&self, prompt: &str, context: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "system": context,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": self.config.temperature
        })
    }

    /// Parse the response from the Anthropic API to extract the completion text.
    fn parse_response(response_body: &str) -> Result<String> {
        let parsed: serde_json::Value = serde_json::from_str(response_body).map_err(|e| {
            AstraeaError::QueryExecution(format!("failed to parse Anthropic response: {e}"))
        })?;

        parsed["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                AstraeaError::QueryExecution(
                    "Anthropic response missing content[0].text".into(),
                )
            })
    }
}

impl LlmProvider for AnthropicProvider {
    fn complete(&self, prompt: &str, context: &str) -> Result<String> {
        let http_fn = self.http_fn.as_ref().ok_or_else(|| {
            AstraeaError::Config(
                "Anthropic provider has no HTTP handler configured. \
                 Call with_http_fn() to provide an HTTP client callback."
                    .into(),
            )
        })?;

        let url = format!("{}/messages", self.config.endpoint.trim_end_matches('/'));
        let body = self.build_request(prompt, context);
        let response_body = http_fn(&url, &body)?;
        Self::parse_response(&response_body)
    }

    fn max_output_tokens(&self) -> usize {
        self.config.max_tokens
    }

    fn input_context_tokens(&self) -> usize {
        // Claude 3.5+ models accept 200k tokens in.
        200_000
    }

    fn name(&self) -> &str {
        "anthropic"
    }
}

// ---------------------------------------------------------------------------
// OllamaProvider
// ---------------------------------------------------------------------------

/// A local Ollama instance provider.
///
/// Defaults to `http://localhost:11434` as the endpoint. Like the other
/// providers, this requires an injected HTTP handler.
///
/// In addition to chat completions via [`LlmProvider`], this struct also
/// implements [`EmbeddingProvider`]. The chat model (`config.model`) and the
/// embedding model (`embedding_model`) are configured independently so that
/// different Ollama models can be used for each task.
///
/// # Embedding usage
///
/// ```rust,ignore
/// use astraea_rag::llm::{LlmConfig, OllamaProvider, ProviderType};
/// use astraea_rag::embedding::EmbeddingProvider;
///
/// let config = LlmConfig {
///     provider: ProviderType::Ollama,
///     model: "llama3".into(),
///     api_key: None,
///     endpoint: String::new(),
///     temperature: 0.7,
///     max_tokens: 1000,
/// };
///
/// let provider = OllamaProvider::new(config)
///     .with_embedding_model("embeddinggemma")
///     .with_embedding_dim(768)
///     .with_embed_http_fn(my_http_fn);
///
/// let vectors = provider.embed(&["hello world"]).unwrap();
/// ```
pub struct OllamaProvider {
    config: LlmConfig,
    /// Optional HTTP handler for chat completion API calls.
    http_fn: Option<Box<dyn Fn(&str, &serde_json::Value) -> Result<String> + Send + Sync>>,
    /// The Ollama model to use for embedding. Defaults to `"embeddinggemma"`.
    embedding_model: String,
    /// Dimension of the vectors the configured embedding model returns.
    ///
    /// This is a caller-supplied hint — the provider does not query the model
    /// at construction time. Default is `768` (correct for `embeddinggemma`).
    /// Update via [`with_embedding_dim`](OllamaProvider::with_embedding_dim) if
    /// you use a different model.
    embedding_dim: usize,
    /// Optional HTTP handler for embedding API calls (`POST /api/embed`).
    embed_http_fn: Option<Box<dyn Fn(&str, &serde_json::Value) -> Result<String> + Send + Sync>>,
}

impl OllamaProvider {
    /// Create a new Ollama provider with the given configuration.
    ///
    /// If the config endpoint is empty, defaults to `http://localhost:11434`.
    ///
    /// Embedding defaults: model = `"embeddinggemma"`, dim = `768`.
    pub fn new(mut config: LlmConfig) -> Self {
        if config.endpoint.is_empty() {
            config.endpoint = "http://localhost:11434".to_string();
        }
        Self {
            config,
            http_fn: None,
            embedding_model: "embeddinggemma".to_string(),
            embedding_dim: 768,
            embed_http_fn: None,
        }
    }

    /// Set a custom HTTP handler for chat completion API calls.
    ///
    /// The handler receives `(endpoint_url, request_body_json)` and returns
    /// the response body as a string.
    pub fn with_http_fn(
        mut self,
        f: impl Fn(&str, &serde_json::Value) -> Result<String> + Send + Sync + 'static,
    ) -> Self {
        self.http_fn = Some(Box::new(f));
        self
    }

    /// Set the Ollama model to use for text embedding.
    ///
    /// Defaults to `"embeddinggemma"`. The embedding model is separate from
    /// the chat model (`config.model`) so you can use different models for
    /// each task.
    pub fn with_embedding_model(mut self, model: impl Into<String>) -> Self {
        self.embedding_model = model.into();
        self
    }

    /// Set the vector dimension for the configured embedding model.
    ///
    /// The provider does not query Ollama at construction time to discover
    /// the actual dimension. Callers must supply the correct value for the
    /// model they are using. Default is `768` (correct for `embeddinggemma`).
    pub fn with_embedding_dim(mut self, dim: usize) -> Self {
        self.embedding_dim = dim;
        self
    }

    /// Set a custom HTTP handler for embedding API calls (`POST /api/embed`).
    ///
    /// The handler receives `(endpoint_url, request_body_json)` and returns
    /// the response body as a string. If not set, [`embed`](EmbeddingProvider::embed)
    /// returns an [`AstraeaError::Config`] error.
    pub fn with_embed_http_fn(
        mut self,
        f: impl Fn(&str, &serde_json::Value) -> Result<String> + Send + Sync + 'static,
    ) -> Self {
        self.embed_http_fn = Some(Box::new(f));
        self
    }

    /// Build the Ollama generate API request body.
    fn build_request(&self, prompt: &str, context: &str) -> serde_json::Value {
        let full_prompt = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("{context}\n\n{prompt}")
        };

        serde_json::json!({
            "model": self.config.model,
            "prompt": full_prompt,
            "stream": false,
            "options": {
                "temperature": self.config.temperature,
                "num_predict": self.config.max_tokens
            }
        })
    }

    /// Parse the response from the Ollama API to extract the completion text.
    fn parse_response(response_body: &str) -> Result<String> {
        let parsed: serde_json::Value = serde_json::from_str(response_body).map_err(|e| {
            AstraeaError::QueryExecution(format!("failed to parse Ollama response: {e}"))
        })?;

        parsed["response"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                AstraeaError::QueryExecution("Ollama response missing 'response' field".into())
            })
    }
}

impl LlmProvider for OllamaProvider {
    fn complete(&self, prompt: &str, context: &str) -> Result<String> {
        let http_fn = self.http_fn.as_ref().ok_or_else(|| {
            AstraeaError::Config(
                "Ollama provider has no HTTP handler configured. \
                 Call with_http_fn() to provide an HTTP client callback."
                    .into(),
            )
        })?;

        let url = format!("{}/api/generate", self.config.endpoint.trim_end_matches('/'));
        let body = self.build_request(prompt, context);
        let response_body = http_fn(&url, &body)?;
        Self::parse_response(&response_body)
    }

    fn max_output_tokens(&self) -> usize {
        self.config.max_tokens
    }

    fn input_context_tokens(&self) -> usize {
        // Ollama exposes whatever the locally-loaded model allows. Without
        // querying the model at runtime we guess — 8k is a safe floor for
        // most small local models; override by implementing this method on
        // a custom provider if you know the model's real context.
        8_000
    }

    fn name(&self) -> &str {
        "ollama"
    }
}

impl EmbeddingProvider for OllamaProvider {
    /// Embed a batch of texts using Ollama's `/api/embed` endpoint.
    ///
    /// Requires an embed HTTP handler to be set via
    /// [`with_embed_http_fn`](OllamaProvider::with_embed_http_fn). Returns
    /// [`AstraeaError::Config`] if no handler is configured.
    ///
    /// Ollama request body: `{"model": "<embedding_model>", "input": [...]}`
    /// Ollama response: `{"embeddings": [[f32, ...], ...]}`
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let http_fn = self.embed_http_fn.as_ref().ok_or_else(|| {
            AstraeaError::Config(
                "Ollama provider has no embed HTTP handler configured. \
                 Call with_embed_http_fn() to provide an HTTP client callback."
                    .into(),
            )
        })?;

        let url = format!("{}/api/embed", self.config.endpoint.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.embedding_model,
            "input": texts
        });

        let response_body = http_fn(&url, &body)?;

        let parsed: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AstraeaError::QueryExecution(format!("failed to parse Ollama embed response: {e}"))
        })?;

        let embeddings = parsed["embeddings"].as_array().ok_or_else(|| {
            AstraeaError::QueryExecution(
                "Ollama embed response missing 'embeddings' field".into(),
            )
        })?;

        embeddings
            .iter()
            .enumerate()
            .map(|(i, vec_val)| {
                let arr = vec_val.as_array().ok_or_else(|| {
                    AstraeaError::QueryExecution(format!(
                        "Ollama embed response: embeddings[{i}] is not an array"
                    ))
                })?;
                arr.iter()
                    .enumerate()
                    .map(|(j, v)| {
                        v.as_f64()
                            .map(|f| f as f32)
                            .ok_or_else(|| {
                                AstraeaError::QueryExecution(format!(
                                    "Ollama embed response: embeddings[{i}][{j}] is not a number"
                                ))
                            })
                    })
                    .collect::<Result<Vec<f32>>>()
            })
            .collect::<Result<Vec<Vec<f32>>>>()
    }

    /// Return the vector dimension this provider is configured to produce.
    ///
    /// This is the value set via [`with_embedding_dim`](OllamaProvider::with_embedding_dim)
    /// (default: `768` for `embeddinggemma`). The provider does not make a
    /// live API call to discover the actual dimension.
    fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::EmbeddingProvider;

    #[test]
    fn test_mock_provider_complete() {
        let provider = MockProvider {
            response_prefix: "Answer:".to_string(),
            context_window: 8000,
        };

        let result = provider.complete("What is Rust?", "Rust is a language").unwrap();
        assert_eq!(result, "Answer: [What is Rust?] Context had 18 chars.");
    }

    #[test]
    fn test_mock_provider_context_window() {
        let provider = MockProvider {
            response_prefix: "Test".to_string(),
            context_window: 4096,
        };
        assert_eq!(provider.max_output_tokens(), 4096);
        assert_eq!(provider.input_context_tokens(), 4096);
        assert_eq!(provider.name(), "mock");
    }

    #[test]
    fn test_openai_provider_no_http_fn() {
        let config = LlmConfig {
            provider: ProviderType::OpenAi,
            model: "gpt-4o".into(),
            api_key: Some("test-key".into()),
            endpoint: "https://api.openai.com/v1".into(),
            temperature: 0.7,
            max_tokens: 1000,
        };

        let provider = OpenAiProvider::new(config);
        let result = provider.complete("Hello", "context");
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no HTTP handler configured"),
            "unexpected error: {err_msg}"
        );
    }

    #[test]
    fn test_openai_provider_with_http_fn() {
        let config = LlmConfig {
            provider: ProviderType::OpenAi,
            model: "gpt-4o".into(),
            api_key: Some("test-key".into()),
            endpoint: "https://api.openai.com/v1".into(),
            temperature: 0.7,
            max_tokens: 1000,
        };

        let provider = OpenAiProvider::new(config).with_http_fn(|_url, _body| {
            Ok(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "Hello from OpenAI!"
                    }
                }]
            })
            .to_string())
        });

        let result = provider.complete("Hello", "context").unwrap();
        assert_eq!(result, "Hello from OpenAI!");
    }

    #[test]
    fn test_anthropic_provider_no_http_fn() {
        let config = LlmConfig {
            provider: ProviderType::Anthropic,
            model: "claude-sonnet-4-20250514".into(),
            api_key: Some("test-key".into()),
            endpoint: "https://api.anthropic.com/v1".into(),
            temperature: 0.7,
            max_tokens: 1000,
        };

        let provider = AnthropicProvider::new(config);
        let result = provider.complete("Hello", "context");
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no HTTP handler configured"),
            "unexpected error: {err_msg}"
        );
    }

    #[test]
    fn test_anthropic_provider_with_http_fn() {
        let config = LlmConfig {
            provider: ProviderType::Anthropic,
            model: "claude-sonnet-4-20250514".into(),
            api_key: Some("test-key".into()),
            endpoint: "https://api.anthropic.com/v1".into(),
            temperature: 0.7,
            max_tokens: 1000,
        };

        let provider = AnthropicProvider::new(config).with_http_fn(|_url, _body| {
            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": "Hello from Anthropic!"
                }]
            })
            .to_string())
        });

        let result = provider.complete("Hello", "context").unwrap();
        assert_eq!(result, "Hello from Anthropic!");
    }

    #[test]
    fn test_ollama_provider_no_http_fn() {
        let config = LlmConfig {
            provider: ProviderType::Ollama,
            model: "llama3".into(),
            api_key: None,
            endpoint: String::new(), // should default to localhost
            temperature: 0.7,
            max_tokens: 1000,
        };

        let provider = OllamaProvider::new(config);
        assert_eq!(provider.name(), "ollama");

        let result = provider.complete("Hello", "context");
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no HTTP handler configured"),
            "unexpected error: {err_msg}"
        );
    }

    #[test]
    fn test_ollama_provider_default_endpoint() {
        let config = LlmConfig {
            provider: ProviderType::Ollama,
            model: "llama3".into(),
            api_key: None,
            endpoint: String::new(),
            temperature: 0.7,
            max_tokens: 1000,
        };

        // Verify endpoint is set by checking the URL passed to http_fn.
        let provider = OllamaProvider::new(config).with_http_fn(|url, _body| {
            assert!(
                url.starts_with("http://localhost:11434"),
                "unexpected URL: {url}"
            );
            Ok(serde_json::json!({"response": "ok"}).to_string())
        });

        let result = provider.complete("test", "ctx").unwrap();
        assert_eq!(result, "ok");
    }

    #[test]
    fn test_provider_type_equality() {
        assert_eq!(ProviderType::OpenAi, ProviderType::OpenAi);
        assert_ne!(ProviderType::OpenAi, ProviderType::Anthropic);
        assert_ne!(ProviderType::Ollama, ProviderType::Mock);
    }

    // -----------------------------------------------------------------------
    // OllamaProvider embedding tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ollama_embed_no_http_fn() {
        let config = LlmConfig {
            provider: ProviderType::Ollama,
            model: "llama3".into(),
            api_key: None,
            endpoint: String::new(),
            temperature: 0.7,
            max_tokens: 1000,
        };

        let provider = OllamaProvider::new(config);
        // No embed_http_fn set — should return a Config error.
        let result = provider.embed(&["hello"]);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no embed HTTP handler configured"),
            "unexpected error: {err_msg}"
        );
    }

    #[test]
    fn test_ollama_embed_with_http_fn() {
        let config = LlmConfig {
            provider: ProviderType::Ollama,
            model: "llama3".into(),
            api_key: None,
            endpoint: "http://localhost:11434".into(),
            temperature: 0.7,
            max_tokens: 1000,
        };

        let provider = OllamaProvider::new(config)
            .with_embedding_model("embeddinggemma")
            .with_embed_http_fn(|url, body| {
                assert!(url.ends_with("/api/embed"), "unexpected url: {url}");
                assert_eq!(body["model"], "embeddinggemma");
                // Return two canned 4-dimensional embeddings.
                Ok(serde_json::json!({
                    "embeddings": [
                        [0.1, 0.2, 0.3, 0.4],
                        [0.5, 0.6, 0.7, 0.8]
                    ]
                })
                .to_string())
            });

        let texts = &["hello", "world"];
        let result = provider.embed(texts).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 4);
        assert!((result[0][0] - 0.1f32).abs() < 1e-5);
        assert!((result[1][3] - 0.8f32).abs() < 1e-5);
    }

    #[test]
    fn test_ollama_embed_dim_default() {
        let config = LlmConfig {
            provider: ProviderType::Ollama,
            model: "llama3".into(),
            api_key: None,
            endpoint: String::new(),
            temperature: 0.0,
            max_tokens: 100,
        };

        let provider = OllamaProvider::new(config);
        // Default dimension for embeddinggemma is 768.
        assert_eq!(provider.embedding_dim(), 768);
    }

    #[test]
    fn test_ollama_embed_dim_custom() {
        let config = LlmConfig {
            provider: ProviderType::Ollama,
            model: "llama3".into(),
            api_key: None,
            endpoint: String::new(),
            temperature: 0.0,
            max_tokens: 100,
        };

        let provider = OllamaProvider::new(config)
            .with_embedding_dim(128);
        assert_eq!(provider.embedding_dim(), 128);
    }

    #[test]
    fn test_ollama_embed_url_uses_endpoint() {
        let config = LlmConfig {
            provider: ProviderType::Ollama,
            model: "llama3".into(),
            api_key: None,
            endpoint: "http://my-ollama-host:8080".into(),
            temperature: 0.0,
            max_tokens: 100,
        };

        let provider = OllamaProvider::new(config)
            .with_embed_http_fn(|url, _body| {
                assert_eq!(url, "http://my-ollama-host:8080/api/embed");
                Ok(serde_json::json!({"embeddings": [[1.0, 2.0]]}).to_string())
            });

        let result = provider.embed(&["test"]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], vec![1.0f32, 2.0f32]);
    }

    /// Integration test — requires a live Ollama instance.
    ///
    /// Run with:
    /// ```text
    /// OLLAMA_URL=http://localhost:11434 \
    ///   cargo test -p astraea-rag --lib -- --ignored --test-threads=1 test_ollama_embed_roundtrip
    /// ```
    #[test]
    #[ignore]
    fn test_ollama_embed_roundtrip() {
        let base_url = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());

        let config = LlmConfig {
            provider: ProviderType::Ollama,
            model: "llama3".into(),
            api_key: None,
            endpoint: base_url,
            temperature: 0.0,
            max_tokens: 100,
        };

        // Use a simple blocking HTTP call via ureq if available; otherwise
        // replicate the callback pattern with std::process::Command as a
        // last resort. For CI-less manual testing the simplest approach is
        // to shell out to curl and parse the result.
        //
        // This test just checks the shape — length of the returned vector
        // should match embedding_dim().
        let provider = OllamaProvider::new(config)
            .with_embedding_model("embeddinggemma")
            .with_embedding_dim(768)
            .with_embed_http_fn(|url, body| {
                // Naive sync HTTP using std — requires the test environment
                // to have curl available. This is intentionally simple.
                let body_str = serde_json::to_string(body).unwrap();
                let output = std::process::Command::new("curl")
                    .args([
                        "-s", "-X", "POST", url,
                        "-H", "Content-Type: application/json",
                        "-d", &body_str,
                    ])
                    .output()
                    .map_err(|e| {
                        AstraeaError::QueryExecution(format!("curl failed: {e}"))
                    })?;
                String::from_utf8(output.stdout).map_err(|e| {
                    AstraeaError::QueryExecution(format!("curl output not utf-8: {e}"))
                })
            });

        let texts = &["The quick brown fox jumps over the lazy dog."];
        let result = provider.embed(texts).expect("embed should succeed with live Ollama");

        assert_eq!(result.len(), 1, "one vector per input");
        assert_eq!(
            result[0].len(),
            provider.embedding_dim(),
            "vector length should match embedding_dim()"
        );
    }
}
