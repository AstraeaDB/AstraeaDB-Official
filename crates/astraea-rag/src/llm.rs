//! LLM provider abstraction for GraphRAG.
//!
//! Provides a trait for LLM providers and concrete implementations for
//! OpenAI, Anthropic, and Ollama APIs. No HTTP dependencies are included
//! in this crate; instead, users inject their own HTTP handler via a
//! callback function.
//!
//! A [`MockProvider`] is included for testing.

use astraea_core::error::{AstraeaError, Result};

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

    /// The maximum context window size in tokens.
    fn context_window_tokens(&self) -> usize;

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

    fn context_window_tokens(&self) -> usize {
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

    fn context_window_tokens(&self) -> usize {
        self.config.max_tokens
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

    fn context_window_tokens(&self) -> usize {
        self.config.max_tokens
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
pub struct OllamaProvider {
    config: LlmConfig,
    /// Optional HTTP handler for actual API calls.
    http_fn: Option<Box<dyn Fn(&str, &serde_json::Value) -> Result<String> + Send + Sync>>,
}

impl OllamaProvider {
    /// Create a new Ollama provider with the given configuration.
    ///
    /// If the config endpoint is empty, defaults to `http://localhost:11434`.
    pub fn new(mut config: LlmConfig) -> Self {
        if config.endpoint.is_empty() {
            config.endpoint = "http://localhost:11434".to_string();
        }
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

    fn context_window_tokens(&self) -> usize {
        self.config.max_tokens
    }

    fn name(&self) -> &str {
        "ollama"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(provider.context_window_tokens(), 4096);
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
}
