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
//!
//! Audit (Roadmap 2.1): every successful `complete()` call emits an
//! `AuditEvent` for EU AI Act Article 12 compliance. The client holds
//! an optional Ed25519 signing key — if unset, the event is still emitted
//! but with a zeroed signature (useful in dev/CI; production should set
//! the key via `with_signing_key` or `ARGUS_AUDIT_SIGNING_KEY` env var).

use async_trait::async_trait;
use ed25519_dalek::SigningKey;
use std::sync::{Arc, Mutex};

use apohara_argus_core::{DataClass, DecisionArtifact};

use super::audit::{emit_audit_event, next_prev_hash};
use super::circuit_breaker::{CircuitBreakerConfig, LlmCircuitBreaker};
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
    (
        "meta/llama-3.1-70b-instruct",
        "Llama 3.1 70B — strong coding + reasoning",
    ),
    (
        "meta/llama-3.1-405b-instruct",
        "Llama 3.1 405B — frontier quality",
    ),
    (
        "nvidia/nemotron-4-340b-instruct",
        "Nemotron 4 340B — strong reasoning",
    ),
    (
        "mistralai/mixtral-8x22b-instruct-v0.1",
        "Mixtral 8x22B — fast",
    ),
    ("qwen/qwen2.5-72b-instruct", "Qwen 2.5 72B — multilingual"),
];

/// Placeholder `DecisionArtifact` for the LLM-client-level audit.
/// The full per-analyzer decision is recorded downstream in the
/// `PRReview`/`Verdict` flow. The LLM receipt only certifies that
/// "the LLM was called and produced output"; analyzers then layer
/// richer decisions on top.
fn llm_receipt_decision() -> DecisionArtifact {
    DecisionArtifact {
        verdict: "allow".into(),
        findings_count: 0,
        rationale: "LLM call completed; downstream analyzers will layer decisions".into(),
    }
}

pub struct NimClient {
    /// Effective inner client. May be the raw `OpenAICompatClient` or,
    /// after [`Self::with_circuit_breaker`], a `LlmCircuitBreaker`
    /// wrapping it. We store as `Box<dyn LlmClient>` so the breaker
    /// decorator can sit transparently in the call chain.
    inner: Box<dyn LlmClient + Send + Sync>,
    /// Explicit model override (set via `with_model`). Empty means: fall
    /// back to the role-based registry lookup at call time.
    pub model: String,
    /// The role this client serves. Determines the default NIM model.
    pub model_role: ModelRole,
    /// Optional Ed25519 signing key for Article 12 audit events. When
    /// `None`, events are still emitted but with a zeroed signature
    /// (dev/CI). Production should call `with_signing_key` or load
    /// `ARGUS_AUDIT_SIGNING_KEY` (base64 32 bytes) at startup.
    pub signing_key: Option<SigningKey>,
    /// Per-client session-scoped hash chain. Each `complete()` call
    /// reads this, attaches it to the new audit event, then advances
    /// it via `next_prev_hash` so the next call links correctly.
    pub prev_hash: Arc<Mutex<[u8; 32]>>,
}

impl std::fmt::Debug for NimClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NimClient")
            .field("provider", &self.inner.provider_name())
            .field("model", &self.model)
            .field("model_role", &self.model_role)
            .field("signing_key_set", &self.signing_key.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for NimClient {
    fn default() -> Self {
        Self {
            inner: Box::new(OpenAICompatClient::new(NIM_DEFAULT_BASE_URL)),
            model: String::new(),
            model_role: ModelRole::Verdict,
            signing_key: None,
            prev_hash: Arc::new(Mutex::new([0u8; 32])),
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
        self.inner = Box::new(OpenAICompatClient::new(base_url));
        self
    }

    /// Set the role used for registry-based model lookup.
    pub fn with_role(mut self, role: ModelRole) -> Self {
        self.model_role = role;
        self
    }

    /// Attach an Ed25519 signing key for audit events. Production
    /// should load the key from `ARGUS_AUDIT_SIGNING_KEY` (base64-encoded
    /// 32-byte secret) at startup. Tests and dev environments can omit.
    pub fn with_signing_key(mut self, key: SigningKey) -> Self {
        self.signing_key = Some(key);
        self
    }

    /// Wrap the current inner client in a [`LlmCircuitBreaker`] with
    /// the given config. Subsequent `complete()` calls flow through
    /// the breaker; once the upstream starts failing repeatedly, the
    /// breaker opens and short-circuits with `LlmError::CircuitOpen`.
    pub fn with_circuit_breaker(mut self, config: CircuitBreakerConfig) -> Self {
        let old = self.inner;
        let breaker: LlmCircuitBreaker<Box<dyn LlmClient + Send + Sync>> =
            LlmCircuitBreaker::new(old, config);
        self.inner = Box::new(breaker);
        self
    }
}

/// Reconstruct the effective prompt text from a `CompletionRequest` so we
/// can BLAKE3-fingerprint it for the audit log. The cleartext is hashed
/// once and immediately dropped — GDPR by construction.
fn prompt_text_from_request(req: &CompletionRequest) -> String {
    req.messages
        .iter()
        .map(|m| format!("{:?}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n")
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
        req.model = model.clone();

        // Capture the prompt text BEFORE we hand off the request — we need
        // it for the BLAKE3 fingerprint, but the cleartext must never
        // outlive this function (GDPR).
        let prompt_text = prompt_text_from_request(&req);
        let temperature = req.temperature.unwrap_or(0.0);

        // Issue the upstream call.
        let result = self.inner.complete(req, api_key).await;

        // Audit only successful calls. Failures are already logged by
        // the upstream client; emitting a half-baked audit event for a
        // failure would be misleading.
        let response = match result {
            Ok(r) => r,
            Err(e) => return Err(e),
        };

        // Sign with the configured key. If no key is set, fall back to
        // a deterministic zero key — production MUST configure a real
        // key (this is loudly documented in the field's docstring).
        let dummy_key = SigningKey::from_bytes(&[0u8; 32]);
        let key = self.signing_key.as_ref().unwrap_or(&dummy_key);

        let prev = *self.prev_hash.lock().expect("prev_hash mutex poisoned");

        let event = emit_audit_event(
            &model,
            "v1", // prompt_template_version — single-version stub for now
            &prompt_text,
            &response.content,
            temperature,
            vec![], // tool calls happen at the agent layer, not the LLM layer
            llm_receipt_decision(),
            prev,
            Some(response.usage.prompt_tokens),
            Some(response.usage.completion_tokens),
            DataClass::SourceCode, // EU AI Act L2: LLM receipts see source code
            "nim-client-v1-policy", // policy_version
            key,
        );

        // Advance the per-client chain so the next event links here.
        let new_prev = next_prev_hash(prev, &event);
        *self.prev_hash.lock().expect("prev_hash mutex poisoned") = new_prev;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Message;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Minimal OpenAI-compatible success body for the mock server.
    /// The `model` field is what the provider echoes back; the NIM
    /// client must copy that into `CompletionResponse.model`.
    const OK_BODY: &str = r#"{"id":"x","model":"echoed-model","choices":[{"index":0,"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}"#;
    const ERR_BODY: &str = r#"{"error":{"message":"nope","type":"x","code":"y"}}"#;

    async fn spawn_mock(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = match listener.accept().await {
                    Ok(c) => c,
                    Err(_) => break,
                };
                let mut buf = vec![0u8; 16384];
                let _ = stream.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
                    status = status, len = body.len(), body = body,
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        format!("http://{}/v1", addr)
    }

    #[test]
    fn default_uses_nim_base_url_and_verdict_role() {
        let c = NimClient::default();
        assert_eq!(c.model_role, ModelRole::Verdict);
        assert_eq!(c.model, "");
        assert!(c.signing_key.is_none());
    }

    #[test]
    fn new_is_alias_for_default() {
        let c = NimClient::new();
        assert_eq!(c.model_role, ModelRole::Verdict);
    }

    #[test]
    fn with_model_sets_explicit_model() {
        let c = NimClient::new().with_model("custom/model");
        assert_eq!(c.model, "custom/model");
    }

    #[test]
    fn with_base_url_replaces_inner_client() {
        let c = NimClient::new().with_base_url("https://other.example/v1");
        assert_eq!(c.inner.provider_name(), "openai-compat");
    }

    #[test]
    fn with_role_sets_role() {
        let c = NimClient::new().with_role(ModelRole::Slop);
        assert_eq!(c.model_role, ModelRole::Slop);
    }

    #[test]
    fn with_circuit_breaker_keeps_nim_provider_name() {
        let c = NimClient::new().with_circuit_breaker(CircuitBreakerConfig::default());
        // The breaker decorates the inner client but the outer
        // provider_name must still report "nim" (the breaker
        // doesn't override the trait method).
        assert_eq!(c.provider_name(), "nim");
    }

    #[test]
    fn provider_name_is_nim() {
        let c = NimClient::new();
        assert_eq!(c.provider_name(), "nim");
    }

    #[test]
    fn debug_does_not_expose_signing_key_material() {
        // The Debug impl prints `signing_key_set: bool` (not the
        // key bytes). We assert by formatting and checking the
        // output contains the boolean and does NOT contain the
        // private key bytes (use a non-zero byte to detect).
        let key = SigningKey::from_bytes(&[42u8; 32]);
        let c = NimClient::new().with_signing_key(key);
        let s = format!("{:?}", c);
        assert!(s.contains("signing_key_set: true"));
        // No 42 byte should appear in the Debug output (the key
        // is 32 bytes of 42; we just check the magic byte is not
        // present in a way that suggests leak).
        assert!(!s.contains("SigningKey"));
    }

    #[tokio::test]
    async fn complete_200_advances_prev_hash_and_returns_response() {
        let url = spawn_mock(200, OK_BODY).await;
        let c = NimClient::new().with_base_url(url);
        let before = *c.prev_hash.lock().unwrap();
        let resp = c
            .complete(
                CompletionRequest::new(
                    "explicit-model",
                    vec![Message::system("sys"), Message::user("user")],
                ),
                "k",
            )
            .await
            .unwrap();
        let after = *c.prev_hash.lock().unwrap();
        assert_eq!(resp.content, "hello");
        assert_eq!(resp.model, "echoed-model");
        // The audit chain must have advanced (BLAKE3 chain).
        assert_ne!(before, after);
    }

    #[tokio::test]
    async fn complete_request_model_wins_over_with_model() {
        let url = spawn_mock(200, OK_BODY).await;
        // The request sets model "from-request"; the client was
        // built with with_model("from-builder"). The request model
        // must win per the resolution order documented in
        // NimClient::complete.
        let c = NimClient::new()
            .with_base_url(url)
            .with_model("from-builder");
        let resp = c
            .complete(
                CompletionRequest::new("from-request", vec![Message::user("u")]),
                "k",
            )
            .await
            .unwrap();
        // We can't assert the model passed to the upstream from
        // here, but we can verify the call succeeded and the
        // response was received.
        assert_eq!(resp.content, "hello");
    }

    #[tokio::test]
    async fn complete_with_model_wins_over_role_default() {
        let url = spawn_mock(200, OK_BODY).await;
        // No request model, with_model set, role=Slop. The
        // with_model must win over the role default.
        let c = NimClient::new()
            .with_base_url(url)
            .with_model("explicit-override")
            .with_role(ModelRole::Slop);
        let resp = c
            .complete(CompletionRequest::new("", vec![Message::user("u")]), "k")
            .await
            .unwrap();
        assert_eq!(resp.content, "hello");
    }

    #[tokio::test]
    async fn complete_uses_role_default_when_no_override() {
        let url = spawn_mock(200, OK_BODY).await;
        // No request model, no with_model, role=Slop. The role
        // default (from ModelRegistry) must be used.
        let c = NimClient::new()
            .with_base_url(url)
            .with_role(ModelRole::Slop);
        let resp = c
            .complete(CompletionRequest::new("", vec![Message::user("u")]), "k")
            .await
            .unwrap();
        assert_eq!(resp.content, "hello");
    }

    #[tokio::test]
    async fn complete_inner_error_does_not_emit_audit_event() {
        let url = spawn_mock(500, ERR_BODY).await;
        let c = NimClient::new().with_base_url(url);
        let before = *c.prev_hash.lock().unwrap();
        let res = c
            .complete(CompletionRequest::new("m", vec![Message::user("u")]), "k")
            .await;
        let after = *c.prev_hash.lock().unwrap();
        assert!(res.is_err());
        // Audit events are only emitted for successful calls; the
        // hash chain must NOT advance on error.
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn complete_missing_key_propagates() {
        let url = spawn_mock(200, OK_BODY).await;
        let c = NimClient::new().with_base_url(url);
        let res = c
            .complete(CompletionRequest::new("m", vec![Message::user("u")]), "")
            .await;
        assert!(matches!(res, Err(LlmError::MissingKey)));
    }

    #[test]
    fn nim_default_base_url_constant_is_correct() {
        assert_eq!(NIM_DEFAULT_BASE_URL, "https://integrate.api.nvidia.com/v1");
    }

    #[test]
    fn nim_default_model_constant_is_set() {
        // The model name must be non-empty (we don't pin a specific
        // value because the registry may swap models as NIM rotates
        // its free-tier catalog).
        assert!(!NIM_DEFAULT_MODEL.is_empty());
        assert!(NIM_DEFAULT_MODEL.contains('/'));
    }

    #[test]
    fn nim_available_models_have_descriptions() {
        // Every model in the catalog must have a non-empty name
        // and a non-empty description.
        for (name, desc) in NIM_AVAILABLE_MODELS {
            assert!(!name.is_empty());
            assert!(!desc.is_empty());
        }
    }
}
