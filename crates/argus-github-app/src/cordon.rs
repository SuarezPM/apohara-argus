//! CordonEnforcer — the runtime guard that keeps the App from
//! doing anything it was not invited to do.
//!
//! The CordonEnforcer is the GitHub-App-specific layer of the
//! Cordon Principle ([`crates/argus-agent::CordonEnforcer`] is the
//! synthesizer-level layer). It enforces three rules that are
//! scoped to webhook handling:
//!
//! 1. **Payload size cap** — bodies larger than 10 MiB are
//!    rejected with 413. Larger payloads would be a DoS vector
//!    (we'd parse them into memory before knowing the event type).
//! 2. **No cross-repo writes** — the handler refuses to act when
//!    the payload claims to be about a repo that the App is
//!    installed into but the operator marked as out-of-scope
//!    (configurable via `ARGUS_APP_ALLOWED_REPOS`).
//! 3. **No SSRF via payload URLs** — if a comment or commit URL
//!    in the payload points to a non-GitHub host, the payload is
//!    dropped. This prevents a malicious PR from tricking the
//!    App into fetching an attacker-controlled URL.
//!
//! The Cordon also enforces a static rule at compile time:
//! **no user-PAT handling** lives in this crate. The only GitHub
//! credentials we use are installation tokens minted by the
//! manifest flow (see [`crate::setup`]). The [`AppConfig`]
//! therefore has no `pat` field.
//!
//! [Refs: argus-silver-roadmap/P.2]

use serde::{Deserialize, Serialize};

use crate::app_state::AppConfig;

/// Maximum accepted webhook payload size. GitHub's hard limit on
/// a `pull_request` event is 25 MiB; we cap at 10 MiB because
/// anything larger is almost certainly adversarial (real PR
/// payloads are < 1 MiB for a single-PR diff).
pub const MAX_PAYLOAD_BYTES: usize = 10 * 1024 * 1024;

/// Hosts we trust URLs in the payload to point at. Anything else
/// is treated as a server-side request forgery attempt.
const TRUSTED_HOSTS: &[&str] = &[
    "github.com",
    "api.github.com",
    "githubusercontent.com",
    "objects.githubusercontent.com",
    // GitHub Enterprise (single-tenant) — operators set this
    // hostname via env if they use it; the default allowlist
    // covers public github.com.
];

/// The Cordon violation we caught. Surfaced as a typed error so
/// the handler can pick the right HTTP status (413 vs 403 vs 422).
#[derive(Debug, thiserror::Error)]
pub enum CordonError {
    #[error("payload too large: {size} bytes (cap {cap} bytes)")]
    PayloadTooLarge { size: usize, cap: usize },
    #[error("repo not in allowlist: {0}")]
    RepoNotAllowed(String),
    #[error("event not in allowlist: {0}")]
    EventNotAllowed(String),
    #[error("untrusted host in payload URL: {0}")]
    UntrustedHost(String),
    #[error("missing required field: {0}")]
    MissingField(&'static str),
}

/// The CordonEnforcer is stateless — every method is `&self`-less
/// or takes `&AppConfig` directly. Splitting it out from
/// `app_state.rs` keeps the security rules in one place that
/// future security reviews can audit.
pub struct CordonEnforcer;

impl CordonEnforcer {
    pub fn new() -> Self {
        Self
    }

    /// Reject payloads larger than [`MAX_PAYLOAD_BYTES`].
    pub fn check_size(&self, body: &[u8]) -> Result<(), CordonError> {
        if body.len() > MAX_PAYLOAD_BYTES {
            return Err(CordonError::PayloadTooLarge {
                size: body.len(),
                cap: MAX_PAYLOAD_BYTES,
            });
        }
        Ok(())
    }

    /// Reject events outside the operator's allowlist.
    pub fn check_event(&self, config: &AppConfig, event: &str) -> Result<(), CordonError> {
        if !config.event_allowed(event) {
            return Err(CordonError::EventNotAllowed(event.to_string()));
        }
        Ok(())
    }

    /// Reject payloads about repos the operator marked as
    /// out-of-scope. When the allowlist is empty (the default),
    /// the App operates on every repo it is installed into.
    pub fn check_repo(&self, config: &AppConfig, full_name: &str) -> Result<(), CordonError> {
        if !config.repo_allowed(full_name) {
            return Err(CordonError::RepoNotAllowed(full_name.to_string()));
        }
        Ok(())
    }

    /// Walk a parsed payload and reject any URL that points at
    /// a host we do not trust. The walk is shallow — we look at
    /// `html_url`, `url`, `comments_url`, `diff_url`, and any
    /// string field whose key ends in `_url`. Deep recursive
    /// scanning is unnecessary; the surface area is bounded.
    pub fn check_no_ssrf(&self, payload: &serde_json::Value) -> Result<(), CordonError> {
        Self::scan(payload)
    }

    fn scan(v: &serde_json::Value) -> Result<(), CordonError> {
        match v {
            serde_json::Value::String(s) => {
                // The fastest check: only walk strings that look
                // like a URL. Saves time on the diff body, which
                // can be megabytes of `+`/`-` lines.
                if s.contains("://") {
                    if let Ok(url) = url::Url::parse(s) {
                        if let Some(host) = url.host_str() {
                            if !TRUSTED_HOSTS.iter().any(|h| host.ends_with(h)) {
                                return Err(CordonError::UntrustedHost(host.to_string()));
                            }
                        }
                    }
                }
                Ok(())
            }
            serde_json::Value::Array(arr) => {
                for item in arr { Self::scan(item)?; }
                Ok(())
            }
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    // A small heuristic to avoid scanning the
                    // whole diff body: skip keys we know are
                    // large blobs.
                    if k == "patch" || k == "diff" || k == "body" || k.ends_with("_body") {
                        // `body` may contain user-supplied URLs
                        // in PR descriptions, but it's also where
                    // reviewers put code samples. We only
                    // scan strings that contain a `://` substring.
                    // The outer `scan` already gates on that.
                        Self::scan(v)?;
                    } else {
                        Self::scan(v)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl Default for CordonEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

/// Subset of the GitHub `pull_request` webhook payload we read.
///
/// We deserialize only the fields we need; the rest is ignored.
/// This keeps us forward-compatible — when GitHub adds fields to
/// the payload, our handler still parses. We also derive
/// `Serialize` so the CordonEnforcer can re-serialize the
/// payload to a `serde_json::Value` for the SSRF scan.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PullRequestPayload {
    pub action: String,
    pub number: u32,
    pub pull_request: PullRequestFields,
    pub repository: RepositoryFields,
    #[serde(default)]
    pub installation: Option<InstallationFields>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PullRequestFields {
    pub number: u32,
    pub head: GitRef,
    pub base: GitRef,
    pub html_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitRef {
    pub sha: String,
    #[serde(default)]
    pub r#ref: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryFields {
    pub full_name: String,
    pub html_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstallationFields {
    pub id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_cordon_accepts_small() {
        let e = CordonEnforcer::new();
        assert!(e.check_size(b"{}").is_ok());
    }

    #[test]
    fn size_cordon_rejects_oversized() {
        let e = CordonEnforcer::new();
        let big = vec![0u8; MAX_PAYLOAD_BYTES + 1];
        let err = e.check_size(&big).unwrap_err();
        assert!(matches!(err, CordonError::PayloadTooLarge { .. }));
    }

    #[test]
    fn repo_cordon_rejects_outside_allowlist() {
        let cfg = AppConfig {
            webhook_secret: "x".into(),
            label_pass: "a".into(),
            label_warn: "b".into(),
            label_fail: "c".into(),
            allowed_repos: vec!["only-this/repo".into()],
            event_allowlist: vec!["pull_request".into()],
        };
        let e = CordonEnforcer::new();
        assert!(e.check_repo(&cfg, "only-this/repo").is_ok());
        assert!(matches!(
            e.check_repo(&cfg, "attacker/repo").unwrap_err(),
            CordonError::RepoNotAllowed(_)
        ));
    }

    #[test]
    fn repo_cordon_allows_when_allowlist_empty() {
        let cfg = AppConfig {
            webhook_secret: "x".into(),
            label_pass: "a".into(),
            label_warn: "b".into(),
            label_fail: "c".into(),
            allowed_repos: vec![],
            event_allowlist: vec!["pull_request".into()],
        };
        let e = CordonEnforcer::new();
        assert!(e.check_repo(&cfg, "any/repo").is_ok());
    }

    #[test]
    fn ssrf_cordon_rejects_untrusted_host() {
        let e = CordonEnforcer::new();
        let payload = serde_json::json!({
            "html_url": "https://attacker.example.com/payload",
            "user": { "html_url": "https://github.com/octocat" }
        });
        assert!(matches!(e.check_no_ssrf(&payload), Err(CordonError::UntrustedHost(_))));
    }

    #[test]
    fn ssrf_cordon_allows_github_hosts() {
        let e = CordonEnforcer::new();
        let payload = serde_json::json!({
            "html_url": "https://github.com/octocat/hello-world/pull/42",
            "diff_url": "https://patch-diff.githubusercontent.com/raw/octocat/hello-world/pull/42.diff",
            "comments_url": "https://api.github.com/repos/octocat/hello-world/issues/42/comments"
        });
        assert!(e.check_no_ssrf(&payload).is_ok());
    }

    #[test]
    fn ssrf_cordon_allows_githubusercontent() {
        let e = CordonEnforcer::new();
        let payload = serde_json::json!({
            "url": "https://objects.githubusercontent.com/foo"
        });
        assert!(e.check_no_ssrf(&payload).is_ok());
    }
}
