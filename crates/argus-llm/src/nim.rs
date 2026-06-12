//! NVIDIA NIM client.
//!
//! NVIDIA NIM exposes an OpenAI-compatible API at `integrate.api.nvidia.com`.
//! This is a thin wrapper around `OpenAICompatClient` with the NIM base URL
//! and a default model catalog.

use async_trait::async_trait;

use super::openai_compat::OpenAICompatClient;
use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError};

pub const NIM_DEFAULT_BASE_URL: &str = "https://integrate.api.nvidia.com/v1";

/// Default model for ARGUS (good balance of quality and free-tier availability).
pub const NIM_DEFAULT_MODEL: &str = "meta/llama-3.1-70b-instruct";

/// Available NIM models (free tier as of 2026).
/// See: https://build.nvidia.com/explore/discover
pub const NIM_AVAILABLE_MODELS: &[(&str, &str)] = &[
    ("meta/llama-3.1-70b-instruct", "Llama 3.1 70B — strong coding + reasoning"),
    ("meta/llama-3.1-405b-instruct", "Llama 3.1 405B — frontier quality"),
    ("nvidia/nemotron-4-340b-instruct", "Nemotron 4 340B — strong reasoning"),
    ("mistralai/mixtral-8x22b-instruct-v0.1", "Mixtral 8x22B — fast"),
    ("qwen/qwen2.5-72b-instruct", "Qwen 2.5 72B — multilingual"),
];

#[derive(Debug, Clone)]
pub struct NimClient {
    inner: OpenAICompatClient,
    pub model: String,
}

impl Default for NimClient {
    fn default() -> Self {
        Self {
            inner: OpenAICompatClient::new(NIM_DEFAULT_BASE_URL),
            model: NIM_DEFAULT_MODEL.to_string(),
        }
    }
}

impl NimClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.inner = OpenAICompatClient::new(base_url);
        self
    }
}

#[async_trait]
impl LlmClient for NimClient {
    fn provider_name(&self) -> &str {
        "nim"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        api_key: &str,
    ) -> Result<CompletionResponse, LlmError> {
        // Use the model from the request, or fall back to the client default.
        let model = if request.model.is_empty() {
            self.model.clone()
        } else {
            request.model.clone()
        };
        let mut req = request;
        req.model = model;
        self.inner.complete(req, api_key).await
    }
}
