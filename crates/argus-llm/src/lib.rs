//! argus-llm — Unified LLM client for ARGUS
//!
//! BYOK model. The user provides their own NVIDIA NIM API key per request.
//! The server never persists keys — they arrive in the `X-LLM-Key` HTTP header
//! and live only for the duration of the request.
//!
//! Supports:
//! - NVIDIA NIM (primary) — OpenAI-compatible API
//! - Any OpenAI-compatible endpoint (Together, Groq, local llama.cpp, etc.)
//! - Mock provider for testing
//!
//! Optional integrations (feature-gated):
//! - DALL-E 3 (OpenAI) for image generation
//! - HeyGen / D-ID for avatar video generation

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

pub mod audit;
pub mod circuit_breaker;
pub mod mock;
pub mod model_registry;
pub mod nim;
pub mod openai_compat;
pub mod retry;

#[cfg(test)]
pub mod test_util;

pub use model_registry::{ModelRegistry, ModelRole};

pub use circuit_breaker::{CircuitBreakerConfig, CircuitError, CircuitState, LlmCircuitBreaker};
pub use mock::MockClient;
pub use nim::NimClient;
pub use openai_compat::OpenAICompatClient;
pub use retry::{RetryClient, RetryConfig};

#[derive(Error, Debug, Clone)]
pub enum LlmError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("API error: {status} — {message}")]
    Api { status: u16, message: String },
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Missing API key (BYOK required)")]
    MissingKey,
    #[error("Timeout after {0:?}")]
    Timeout(Duration),
    #[error("Rate limited — retry after {retry_after:?}")]
    RateLimited { retry_after: Option<Duration> },
    /// Circuit breaker is open — calls are being short-circuited.
    /// Not retryable; the upstream should be considered down.
    #[error("Circuit breaker is open — upstream is unhealthy")]
    CircuitOpen,
}

impl LlmError {
    /// Whether the retry decorator should attempt another call.
    ///
    /// Retryable: network errors, timeouts, 429, 503, 5xx.
    /// Not retryable: 4xx (except 429), parse errors, missing key, circuit open.
    pub fn is_retryable(&self) -> bool {
        match self {
            LlmError::RateLimited { .. } => true,
            LlmError::Timeout(_) => true,
            LlmError::Http(_) => true,
            LlmError::Api { status, .. } => *status == 429 || *status == 503 || *status >= 500,
            LlmError::Parse(_) => false,
            LlmError::MissingKey => false,
            LlmError::CircuitOpen => false,
        }
    }
}

/// A chat message. We use the OpenAI-compatible format internally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Completion request to an LLM.
#[derive(Debug, Clone, Serialize)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

impl CompletionRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            top_p: None,
            stop: None,
        }
    }
    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }
    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }
}

/// Completion response from an LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    pub model: String,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Unified LLM client trait. Analyzers depend on this, not on a specific provider.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// The provider name (e.g., "nim", "openai", "mock").
    fn provider_name(&self) -> &str;

    /// Send a completion request. Returns the response or an error.
    async fn complete(
        &self,
        request: CompletionRequest,
        api_key: &str,
    ) -> Result<CompletionResponse, LlmError>;

    /// Convenience: single-turn completion (system + user).
    async fn complete_one_shot(
        &self,
        model: &str,
        system: &str,
        user: &str,
        api_key: &str,
        temperature: f32,
        max_tokens: u32,
    ) -> Result<CompletionResponse, LlmError> {
        let req = CompletionRequest::new(model, vec![Message::system(system), Message::user(user)])
            .with_temperature(temperature)
            .with_max_tokens(max_tokens);
        self.complete(req, api_key).await
    }
}

/// Blanket impl so a `Box<dyn LlmClient>` can itself be wrapped in
/// another decorator (e.g., `LlmCircuitBreaker<Box<dyn LlmClient>>`).
/// Without this, composing `RetryClient<OpenAICompatClient>` inside a
/// `LlmCircuitBreaker` would require concrete types at every layer.
#[async_trait]
impl LlmClient for Box<dyn LlmClient + Send + Sync> {
    fn provider_name(&self) -> &str {
        (**self).provider_name()
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        api_key: &str,
    ) -> Result<CompletionResponse, LlmError> {
        (**self).complete(request, api_key).await
    }
}

/// Build a client based on environment / config.
pub fn client_from_env() -> Box<dyn LlmClient> {
    // Default to NVIDIA NIM. The user passes their key per-request.
    Box::new(NimClient::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_helpers_work() {
        let m = Message::system("sys");
        assert_eq!(m.role, Role::System);
        assert_eq!(m.content, "sys");
    }

    #[test]
    fn request_builder_chains() {
        let req = CompletionRequest::new("test-model", vec![Message::user("hi")])
            .with_temperature(0.5)
            .with_max_tokens(100);
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_tokens, Some(100));
    }
}
