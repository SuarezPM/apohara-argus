//! NVIDIA NIM client.
//!
//! NVIDIA NIM exposes an OpenAI-compatible API at `integrate.api.nvidia.com`.
//! This is a thin wrapper around `OpenAICompatClient` with the NIM base URL
//! and a default model catalog.
//!
//! Model selection is role-based: each `NimClient` is associated with a
//! `ModelRole` (slop / security / arch / verdict / lens) and resolves its
//! NIM model via `ModelRegistry`. Callers can still pin an exact model with
//! `with_model()` (explicit override wins over the role default).

use async_trait::async_trait;

use super::model_registry::{ModelRegistry, ModelRole};
use super::openai_compat::OpenAICompatClient;
use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError};

pub const NIM_DEFAULT_BASE_URL: &str = "https://integrate.api.nvidia.com/v1";

/// Default model for ARGUS (good balance of quality and free-tier availability).
///
/// Retained for backward compatibility — new code should use
/// `ModelRegistry::default_for_role(ModelRole::*)` instead.
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
    /// Explicit model override (set via `with_model`). Empty means: fall
    /// back to the role-based registry lookup at call time.
    pub model: String,
    /// The role this client serves. Determines the default NIM model.
    pub model_role: ModelRole,
}

impl Default for NimClient {
    fn default() -> Self {
        Self {
            inner: OpenAICompatClient::new(NIM_DEFAULT_BASE_URL),
            model: String::new(),
            model_role: ModelRole::Verdict,
        }
    }
}

impl NimClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pin an exact model (overrides the role-based registry default).
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.inner = OpenAICompatClient::new(base_url);
        self
    }

    /// Set the role used for registry-based model lookup.
    pub fn with_role(mut self, role: ModelRole) -> Self {
        self.model_role = role;
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
        // Resolution order:
        // 1. Model specified in the request.
        // 2. Explicit override set via `with_model`.
        // 3. Role-based lookup via the model registry.
        let model = if !request.model.is_empty() {
            request.model.clone()
        } else if !self.model.is_empty() {
            self.model.clone()
        } else {
            ModelRegistry::default_for_role(self.model_role)
        };
        let mut req = request;
        req.model = model;
        self.inner.complete(req, api_key).await
    }
}
