//! Shared application state for the argus-github-app HTTP server.
//!
//! The state is a single `AppState` cloned into every axum handler
//! via `axum::extract::State`. It holds:
//! - the webhook secret (HMAC key for signature verification)
//! - a default set of labels to apply (configurable via env)
//! - the install-restriction list (which repos the App is allowed
//!   to operate on; empty = "all installs")
//!
//! The state is intentionally small and cheaply cloneable. Anything
//! that needs I/O lives behind a `tokio::sync::Mutex` (currently
//! none — the App does not cache tokens or diffs).
//!
//! [Refs: argus-silver-roadmap/P.2]

use serde::{Deserialize, Serialize};

/// Per-process configuration for the GitHub App backend.
///
/// All fields are populated from environment variables at startup
/// (see [`AppState::from_env`]). The struct is `Clone` so the
/// axum `State` extractor can hand a copy to every handler without
/// lifetime gymnastics.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// The webhook secret. GitHub uses this to compute the
    /// `X-Hub-Signature-256` header; we must verify it before
    /// trusting the payload.
    pub webhook_secret: String,
    /// The label applied when the deterministic slop layer (and
    /// optionally the LLM layer) returns an `Approved` verdict.
    /// Defaults to `argus/approved`.
    pub label_pass: String,
    /// The label applied when the verdict is `ReviewRequired`
    /// (the LLM said "look closer" but no halt). Defaults to
    /// `argus/needs-review`.
    pub label_warn: String,
    /// The label applied when the verdict is `Halted` (critical
    /// finding — usually a security issue). Defaults to
    /// `argus/halted`.
    pub label_fail: String,
    /// Optional list of `owner/repo` slugs the App is allowed to
    /// operate on. When non-empty, webhooks for repos outside the
    /// allowlist return 403. When empty (the default), the App
    /// operates on every repo the GitHub App is installed into.
    pub allowed_repos: Vec<String>,
    /// Optional comma-separated allowlist of events we act on.
    /// Defaults to `pull_request` (we ignore every other event
    /// type with a 200 no-op).
    pub event_allowlist: Vec<String>,
}

impl AppConfig {
    /// Build an `AppConfig` from environment variables. Missing
    /// required variables are surfaced as `Err` with a clear
    /// message so the operator learns what to fix.
    ///
    /// Required:
    /// - `ARGUS_APP_WEBHOOK_SECRET` — the HMAC secret
    ///
    /// Optional (with sensible defaults):
    /// - `ARGUS_APP_LABEL_PASS` — default `argus/approved`
    /// - `ARGUS_APP_LABEL_WARN` — default `argus/needs-review`
    /// - `ARGUS_APP_LABEL_FAIL` — default `argus/halted`
    /// - `ARGUS_APP_ALLOWED_REPOS` — comma-separated `owner/repo`
    /// - `ARGUS_APP_EVENTS` — comma-separated event names
    pub fn from_env() -> Result<Self, ConfigError> {
        let webhook_secret = std::env::var("ARGUS_APP_WEBHOOK_SECRET")
            .map_err(|_| ConfigError::Missing("ARGUS_APP_WEBHOOK_SECRET"))?;
        if webhook_secret.is_empty() {
            return Err(ConfigError::Invalid(
                "ARGUS_APP_WEBHOOK_SECRET must not be empty".into(),
            ));
        }
        let label_pass =
            std::env::var("ARGUS_APP_LABEL_PASS").unwrap_or_else(|_| "argus/approved".to_string());
        let label_warn = std::env::var("ARGUS_APP_LABEL_WARN")
            .unwrap_or_else(|_| "argus/needs-review".to_string());
        let label_fail =
            std::env::var("ARGUS_APP_LABEL_FAIL").unwrap_or_else(|_| "argus/halted".to_string());
        let allowed_repos = std::env::var("ARGUS_APP_ALLOWED_REPOS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let event_allowlist = std::env::var("ARGUS_APP_EVENTS")
            .unwrap_or_else(|_| "pull_request".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(Self {
            webhook_secret,
            label_pass,
            label_warn,
            label_fail,
            allowed_repos,
            event_allowlist,
        })
    }

    /// Pick the label for a given verdict slug (`approved` /
    /// `needs-review` / `halted`).
    pub fn label_for(&self, slug: &str) -> &str {
        match slug {
            "approved" => &self.label_pass,
            "needs-review" => &self.label_warn,
            "halted" => &self.label_fail,
            // Unknown verdicts fall through to the warn label —
            // we always want SOME signal, even on edge cases.
            _ => &self.label_warn,
        }
    }

    /// `true` when the given `owner/repo` slug is in the allowlist.
    /// An empty allowlist means "allow everything" (the App is
    /// restricted only by the GitHub installation scope).
    pub fn repo_allowed(&self, full_name: &str) -> bool {
        self.allowed_repos.is_empty() || self.allowed_repos.iter().any(|r| r == full_name)
    }

    /// `true` when we act on the given event name.
    pub fn event_allowed(&self, event: &str) -> bool {
        self.event_allowlist.iter().any(|e| e == event)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable: {0}")]
    Missing(&'static str),
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

/// The shared state passed to every handler.
#[derive(Debug, Clone)]
pub struct AppState {
    pub config: AppConfig,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }
}

/// The JSON shape returned by `GET /health`. Kept simple on purpose
/// — Fly's health check, k8s probes, and uptime monitors all want
/// the same minimal answer.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub service: &'static str,
    pub version: &'static str,
}

impl HealthResponse {
    pub fn current() -> Self {
        Self {
            ok: true,
            service: "argus-github-app",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

/// The JSON shape returned by `GET /version`. Includes the build
/// SHA when present (set by the release workflow; the Dockerfile
/// passes it as `ARGUS_GIT_SHA`).
#[derive(Debug, Serialize, Deserialize)]
pub struct VersionResponse {
    pub name: &'static str,
    pub version: &'static str,
    pub git_sha: Option<String>,
}

impl VersionResponse {
    pub fn current() -> Self {
        Self {
            name: "argus-github-app",
            version: env!("CARGO_PKG_VERSION"),
            git_sha: std::env::var("ARGUS_GIT_SHA")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }
}
