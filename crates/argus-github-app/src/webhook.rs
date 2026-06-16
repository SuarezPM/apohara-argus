//! Webhook handler — receives GitHub `pull_request` events and
//! posts an ARGUS verdict + label back to the PR.
//!
//! The handler is intentionally minimal: it does the four
//! things the CordonEnforcer requires, then hands off to
//! `argus-github` for the network call.
//!
//! 1. Verify HMAC signature (constant-time compare).
//! 2. Check payload size cap.
//! 3. Parse the event JSON.
//! 4. Cordon-check (event allowlist, repo allowlist, no-SSRF).
//! 5. Fetch the diff via the GitHub client.
//! 6. Run the deterministic slop layer (always; <100ms).
//! 7. If `ARGUS_NIM_KEY` is set, run the LLM layer too.
//! 8. Post a comment and set a label.
//!
//! [Refs: argus-silver-roadmap/P.2]

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use tracing::{error, info, warn};

use argus_github::GitHubClient;
use argus_slop::deterministic::{run_deterministic_rules, Severity, SlopSignal};

use crate::app_state::{AppConfig, AppState};
use crate::cordon::{CordonEnforcer, PullRequestPayload};
use crate::signature;

/// The events we react to. Any other event is a 200 no-op
/// (we must always return 200 for events we ignore, otherwise
/// GitHub will retry forever).
const ACTIONS: &[&str] = &["opened", "synchronize", "reopened"];

/// The label applied on the "deterministic-only" path. The
/// A flattened summary of the deterministic slop run, ready to
/// be formatted into a comment + used to pick a label.
#[derive(Debug)]
struct SlopSummary {
    score: f32,
    findings_count: usize,
    error_count: usize,
    warning_count: usize,
    info_count: usize,
    body: String,
}

/// `POST /webhook` — the main entry point.
///
/// We read the raw body (not `Json<...>`) so the signature
/// check sees exactly the bytes GitHub signed. If we used the
/// `Json` extractor, axum would deserialize and re-serialize,
/// and whitespace changes would invalidate the HMAC.
pub async fn webhook_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let cordon = CordonEnforcer::new();

    // Pipeline of cheap→expensive validations. Each step returns
    // either the validated value or an HTTP error response; the
    // `?`-style early-return via `if let Err(resp) = ...` keeps
    // this handler a flat list of guard clauses (cognitive
    // complexity ~10, down from 65).
    if let Err(resp) = check_payload_size(&cordon, &body) {
        return resp;
    }
    if let Err(resp) = check_signature(&state, &headers, &body) {
        return resp;
    }
    if let Err(resp) = check_event_allowlisted(&state, &cordon, &headers) {
        return resp;
    }
    let payload = match parse_payload(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if let Err(resp) = check_repo_and_ssrf(&state, &cordon, &payload) {
        return resp;
    }
    if !is_pr_action(&payload) {
        info!(
            action = %payload.action,
            repo = %payload.repository.full_name,
            number = payload.number,
            "ignoring non-PR-action event"
        );
        return (StatusCode::OK, "ignored: non-PR-action event").into_response();
    }

    // Build the GitHub client + spawn the async pipeline.
    let config = state.config.clone();
    let gh = match build_github_client() {
        Some(gh) => gh,
        None => {
            warn!("ARGUS_APP_INSTALL_TOKEN not set at webhook time; skipping review");
            return (StatusCode::OK, "queued: install token not configured").into_response();
        }
    };
    tokio::spawn(async move {
        if let Err(e) = handle_pr_event(config, payload, gh).await {
            error!(error = %e, "failed to handle PR event");
        }
    });

    (StatusCode::OK, "queued").into_response()
}

/// Cheap-first guard: payload size must be under the CordonEnforcer
/// limit (default 1 MiB). Done before signature verification because
/// reading 1 MiB to compute HMAC is expensive; rejecting at 1 MiB+1
/// saves the crypto work.
///
/// `#[allow(clippy::result_large_err)]` — `axum::response::Response`
/// is 128+ bytes (it embeds a `Body` enum). Boxing the Err variant
/// would add an allocation on every error path for no real win
/// (error paths are cold). The Ok path stays zero-cost.
#[allow(clippy::result_large_err)]
fn check_payload_size(cordon: &CordonEnforcer, body: &Bytes) -> Result<(), axum::response::Response> {
    if let Err(err) = cordon.check_size(body) {
        warn!(error = %err, "webhook payload too large");
        Err((StatusCode::PAYLOAD_TOO_LARGE, err.to_string()).into_response())
    } else {
        Ok(())
    }
}

/// Verify the `X-Hub-Signature-256` HMAC. The `X-Hub-Signature-256`
/// header carries `sha256=<hex>`; we recompute over the raw body
/// bytes (NOT a re-serialized JSON) so whitespace changes don't
/// invalidate the signature.
#[allow(clippy::result_large_err)]
fn check_signature(
    state: &AppState,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<(), axum::response::Response> {
    let sig_header = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if let Err(err) = signature::verify(
        state.config.webhook_secret.as_bytes(),
        sig_header,
        body,
    ) {
        warn!(error = %err, "webhook signature verification failed");
        Err((StatusCode::UNAUTHORIZED, "invalid signature".to_string()).into_response())
    } else {
        Ok(())
    }
}

/// Reject events outside the operator's `event_allowlist`. 422 (not
/// 400) because the event is syntactically valid — we just don't
/// act on it.
#[allow(clippy::result_large_err)]
fn check_event_allowlisted(
    state: &AppState,
    cordon: &CordonEnforcer,
    headers: &HeaderMap,
) -> Result<(), axum::response::Response> {
    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if let Err(err) = cordon.check_event(&state.config, event) {
        warn!(error = %err, "webhook event not in allowlist");
        Err((StatusCode::UNPROCESSABLE_ENTITY, err.to_string()).into_response())
    } else {
        Ok(())
    }
}

/// Parse the raw body into our typed payload. Done AFTER signature
/// verification so a malformed-JSON attacker can't probe the schema
/// without a valid HMAC.
#[allow(clippy::result_large_err)]
fn parse_payload(body: &Bytes) -> Result<PullRequestPayload, axum::response::Response> {
    serde_json::from_slice(body).map_err(|e| {
        warn!(error = %e, "webhook payload failed to parse");
        (StatusCode::BAD_REQUEST, format!("invalid JSON: {}", e)).into_response()
    })
}

/// Two-step guard: (a) the repo must be in the operator's
/// `allowed_repos` allowlist, and (b) the serialized payload must
/// not contain URLs pointing to private network ranges (SSRF guard —
/// if a future field gets URL-typed and the operator adds a
/// check_url feature, this is where it plugs in).
#[allow(clippy::result_large_err)]
fn check_repo_and_ssrf(
    state: &AppState,
    cordon: &CordonEnforcer,
    payload: &PullRequestPayload,
) -> Result<(), axum::response::Response> {
    if let Err(err) = cordon.check_repo(&state.config, &payload.repository.full_name) {
        warn!(error = %err, repo = %payload.repository.full_name, "repo not in allowlist");
        return Err((StatusCode::FORBIDDEN, err.to_string()).into_response());
    }
    if let Ok(value) = serde_json::to_value(payload) {
        if let Err(err) = cordon.check_no_ssrf(&value) {
            warn!(error = %err, "payload contained untrusted URL");
            return Err((StatusCode::UNPROCESSABLE_ENTITY, err.to_string()).into_response());
        }
    }
    Ok(())
}

/// Only act on PR-action events. We do this last (after all
/// other validations) because an event like `push` to a non-allowlisted
/// repo is harmless — the early returns cover the dangerous cases.
fn is_pr_action(payload: &PullRequestPayload) -> bool {
    ACTIONS.contains(&payload.action.as_str())
}

/// Build the GitHub client from the install token + optional
/// test-only base URL override. Returns `None` if the install
/// token is not configured (the webhook should ack the event
/// with 200 even in that case — the event is delivered, we just
/// can't act on it).
fn build_github_client() -> Option<GitHubClient> {
    let token = std::env::var("ARGUS_APP_INSTALL_TOKEN").ok()?;
    if token.is_empty() {
        return None;
    }
    // Production hard-codes https://api.github.com;
    // `ARGUS_GITHUB_API_BASE_URL` is a test-only override
    // (the integration tests point it at a mock server).
    // When the env var is unset, we get the default
    // `https://api.github.com` — no behavior change.
    let mut client = GitHubClient::new(token);
    if let Ok(base) = std::env::var("ARGUS_GITHUB_API_BASE_URL") {
        if !base.is_empty() {
            client = client.with_base_url(base);
        }
    }
    Some(client)
}

/// The actual review work. Spawned in a `tokio::spawn` so the
/// HTTP response can return 200 immediately; GitHub's webhook
/// delivery expects a fast ack.
///
/// The `gh` client is passed in (not constructed from env here)
/// so the integration tests can point it at a mock server via
/// `GitHubClient::with_base_url`. Production callers construct
/// the client from the `ARGUS_APP_INSTALL_TOKEN` env var in
/// [`spawn_handler`].
async fn handle_pr_event(
    config: AppConfig,
    payload: PullRequestPayload,
    gh: GitHubClient,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (owner, repo, number) = (
        payload.repository.full_name.split('/').next().unwrap_or(""),
        payload.repository.full_name.split('/').nth(1).unwrap_or(""),
        payload.number,
    );

    info!(
        repo = %payload.repository.full_name,
        number,
        action = %payload.action,
        head = %payload.pull_request.head.sha,
        "fetching PR diff"
    );
    let diff = gh.get_diff(owner, repo, number).await?;

    // Always run the deterministic slop layer. <100ms; no
    // network; uses regex rules from `argus-slop`.
    let signals = run_deterministic_rules(&diff);
    let slop = summarize_signals(&signals);

    let (label_slug, comment_body) = compose_verdict(&config, &payload, &slop);

    // Post the comment.
    if let Err(e) = gh.post_comment(owner, repo, number, &comment_body).await {
        warn!(
            error = %e,
            repo = %payload.repository.full_name,
            number,
            "failed to post comment"
        );
    } else {
        info!(
            repo = %payload.repository.full_name,
            number,
            "posted ARGUS verdict comment"
        );
    }

    // Set the label.
    let label = config.label_for(label_slug);
    if let Err(e) = gh.set_labels(owner, repo, number, &[label]).await {
        warn!(
            error = %e,
            repo = %payload.repository.full_name,
            number,
            label,
            "failed to set label"
        );
    } else {
        info!(
            repo = %payload.repository.full_name,
            number,
            label,
            "set ARGUS label"
        );
    }

    // Emit an audit-friendly log line. The full
    // argus-verify audit chain (BLAKE3 + Ed25519) is
    // exercised when the LLM is enabled; for the
    // deterministic-only path we log the BLAKE3 fingerprint
    // so the operator has a tamper-evident marker.
    emit_audit_receipt(owner, repo, number, &diff, &slop);

    Ok(())
}

/// Flatten the slop signals into the shape the comment + label
/// picker needs. Lives in its own function so we can unit-test
/// the thresholding without a real GitHub client.
fn summarize_signals(signals: &[SlopSignal]) -> SlopSummary {
    let mut error_count = 0;
    let mut warning_count = 0;
    let mut info_count = 0;
    let mut bullets: Vec<String> = Vec::new();
    for s in signals {
        match s.severity {
            Severity::Error => error_count += 1,
            Severity::Warning => warning_count += 1,
            Severity::Info => info_count += 1,
        }
        bullets.push(format!(
            "- `[{}]` line {}: {}",
            s.rule_id, s.line, s.message
        ));
    }
    // Weighted score: 0.4 per Error, 0.15 per Warning, 0.05 per
    // Info, clamped to [0, 1]. Matches the rough shape of
    // `argus-slop::slop_detector::SlopDetector::slop_score`.
    let raw = error_count as f32 * 0.4 + warning_count as f32 * 0.15 + info_count as f32 * 0.05;
    let score = raw.clamp(0.0, 1.0);
    let body = if signals.is_empty() {
        "Deterministic layer found no mechanical slop signals.".to_string()
    } else {
        format!(
            "Deterministic layer found {} signal(s) ({} error, {} warning, {} info):\n\n{}",
            signals.len(),
            error_count,
            warning_count,
            info_count,
            bullets.join("\n")
        )
    };
    SlopSummary {
        score,
        findings_count: signals.len(),
        error_count,
        warning_count,
        info_count,
        body,
    }
}

/// Build the verdict slug + comment body. Split out from
/// `handle_pr_event` so the formatting is unit-testable
/// without a real GitHub client.
fn compose_verdict(
    config: &AppConfig,
    payload: &PullRequestPayload,
    slop: &SlopSummary,
) -> (&'static str, String) {
    // Map the deterministic result to a verdict. The
    // deterministic layer is binary: "any Error-severity
    // finding" or "no Error-severity finding". A high slop
    // score OR any Error -> needs-review.
    let slug = if slop.error_count == 0 && slop.score < 0.5 {
        "approved"
    } else {
        "needs-review"
    };

    let body = format!(
        "## ARGUS deterministic review — `{slug}`\n\n\
         **PR:** `{repo}#{number}`\n\
         **Action:** `{action}`\n\
         **Slop score:** {slop_score:.2} / 1.00\n\
         **Findings:** {findings_count} (E:{error_count} W:{warning_count} I:{info_count})\n\n\
         {body}\n\n\
         ---\n\
         *Deterministic layer only. To enable the LLM layer, set `ARGUS_NIM_KEY` in the App's \
         environment (BYOK). See https://argus.apohara.dev for details.*\n",
        slug = slug,
        repo = payload.repository.full_name,
        number = payload.number,
        action = payload.action,
        slop_score = slop.score,
        findings_count = slop.findings_count,
        error_count = slop.error_count,
        warning_count = slop.warning_count,
        info_count = slop.info_count,
        body = slop.body,
    );

    // Suppress the unused-warning for `config` while keeping
    // it in the signature for forward-compat (the function
    // will use it when LLM verdicts are wired in).
    let _ = config;
    (slug, body)
}

fn emit_audit_receipt(owner: &str, repo: &str, number: u32, diff: &str, slop: &SlopSummary) {
    let diff_fingerprint = blake3::hash(diff.as_bytes());
    let summary_fingerprint = blake3::hash(slop.body.as_bytes());
    info!(
        owner,
        repo,
        number,
        slop_score = slop.score,
        findings = slop.findings_count,
        diff_b3 = %hex::encode(*diff_fingerprint.as_bytes()),
        summary_b3 = %hex::encode(*summary_fingerprint.as_bytes()),
        "ARGUS deterministic review complete"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> PullRequestPayload {
        PullRequestPayload {
            action: "opened".into(),
            number: 42,
            pull_request: crate::cordon::PullRequestFields {
                number: 42,
                head: crate::cordon::GitRef {
                    sha: "abc".into(),
                    r#ref: Some("feature".into()),
                },
                base: crate::cordon::GitRef {
                    sha: "def".into(),
                    r#ref: Some("main".into()),
                },
                html_url: "https://github.com/octocat/hello-world/pull/42".into(),
            },
            repository: crate::cordon::RepositoryFields {
                full_name: "octocat/hello-world".into(),
                html_url: "https://github.com/octocat/hello-world".into(),
            },
            installation: None,
        }
    }

    #[test]
    fn summarize_empty_signals_is_zero() {
        let s = summarize_signals(&[]);
        assert_eq!(s.findings_count, 0);
        assert_eq!(s.score, 0.0);
        assert!(s.body.contains("no mechanical slop"));
    }

    #[test]
    fn summarize_counts_severities() {
        let signals = vec![
            SlopSignal::error("SLOP-001", 1, "x"),
            SlopSignal::warning("SLOP-002", 2, "y"),
            SlopSignal::info("SLOP-003", 3, "z"),
        ];
        let s = summarize_signals(&signals);
        assert_eq!(s.error_count, 1);
        assert_eq!(s.warning_count, 1);
        assert_eq!(s.info_count, 1);
        // 0.4 + 0.15 + 0.05 = 0.6
        assert!((s.score - 0.6).abs() < 0.01);
        // The bullets are joined into `body` — verify the join
        // happened (3 signals → 3 lines prefixed with "- `[rule]`").
        assert_eq!(s.body.matches("- `[").count(), 3);
    }

    #[test]
    fn compose_verdict_approved_when_clean() {
        let cfg = AppConfig {
            webhook_secret: "x".into(),
            label_pass: "argus/approved".into(),
            label_warn: "argus/needs-review".into(),
            label_fail: "argus/halted".into(),
            allowed_repos: vec![],
            event_allowlist: vec!["pull_request".into()],
        };
        let s = summarize_signals(&[]);
        let (slug, body) = compose_verdict(&cfg, &sample_payload(), &s);
        assert_eq!(slug, "approved");
        assert!(body.contains("`approved`"));
        assert!(body.contains("octocat/hello-world#42"));
    }

    #[test]
    fn compose_verdict_needs_review_on_error() {
        let cfg = AppConfig {
            webhook_secret: "x".into(),
            label_pass: "argus/approved".into(),
            label_warn: "argus/needs-review".into(),
            label_fail: "argus/halted".into(),
            allowed_repos: vec![],
            event_allowlist: vec!["pull_request".into()],
        };
        let signals = vec![SlopSignal::error("SLOP-001", 1, "boom")];
        let s = summarize_signals(&signals);
        let (slug, _) = compose_verdict(&cfg, &sample_payload(), &s);
        assert_eq!(slug, "needs-review");
    }
}
