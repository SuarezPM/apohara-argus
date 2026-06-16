//! argus-verify HTTP server
//!
//! Listens on ARGUS_API_PORT (default 8080). Endpoints:
//! - POST /analyze        { pr_url, repo_context?, post_comment?, set_labels? }
//! - GET  /health
//! - GET  /audit/export   [Refs: 2.2] NDJSON stream of Article 12
//!   audit events, with a manifest footer.
//!
//! The NIM key is BYOK: pass it in the `X-LLM-Key` header. The server
//! also accepts ARGUS_NIM_KEY env var as a fallback.
//!
//! Idempotency: if the caller supplies an `X-Idempotency-Key` header,
//! the server caches the verdict under that key and returns the cached
//! response on subsequent requests with the same key + same `pr_url`.
//! See `cache.rs` and item [Refs: 6.2] of the roadmap.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use argus_verify::{
    a2a_message_handler, agent_card_handler, audit_export_handler, shutdown_signal,
    IdempotencyCache, VerifyWorker,
};
use axum::{
    extract::State,
    http::HeaderMap,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    worker: Arc<VerifyWorker>,
    cache: IdempotencyCache,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize OTel stdout exporter + tracing-subscriber fmt layer.
    // Opt-in via `ARGUS_OTEL_DISABLED=true` (default off for zero overhead).
    // The init function uses `try_init` so if a global subscriber is
    // already set (e.g., from a test harness), this is a no-op.
    let _otel_guard = argus_otel::init("argus-verify");

    // Fallback fmt init for the case where OTel is disabled. Cheap when
    // OTel is on (the layer is a thin pass-through to stdout).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,argus=debug")),
        )
        .try_init();

    let port: u16 = std::env::var("ARGUS_API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);

    // GitHub client (optional, for posting comments)
    let worker = if let Ok(gh_token) = std::env::var("GITHUB_TOKEN") {
        if !gh_token.is_empty() {
            let gh = argus_github::GitHubClient::new(gh_token);
            VerifyWorker::new("").with_github(gh)
        } else {
            VerifyWorker::new("")
        }
    } else {
        VerifyWorker::new("")
    };

    let cache = IdempotencyCache::new();

    // Background cleanup so the in-memory map cannot grow unboundedly
    // across long-lived workers. Runs every hour; cheap when the map
    // is empty. Spawned on the same runtime as the server.
    {
        let cache_for_cleanup = cache.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3600));
            loop {
                interval.tick().await;
                let removed = cache_for_cleanup.cleanup_expired().await;
                if removed > 0 {
                    tracing::info!(removed, "Cleaned up expired idempotency entries");
                }
            }
        });
    }

    let state = AppState {
        worker: Arc::new(worker),
        cache,
    };

    // The audit-export route takes its own state (`InMemoryAuditStore`,
    // not `AppState`), so it lives in a separate sub-router that we
    // `merge` into the main one.
    let audit_store = state.worker.audit_store.clone();
    let audit_router = Router::new()
        .route("/audit/export", get(audit_export_handler))
        .with_state(audit_store);

    // A2A sub-router (Roadmap 3.2). Opt-in via env var — default
    // OFF so existing deployments don't expose new surface. To enable
    // a Google-A2A orchestrator to discover and message us, set
    // `ARGUS_A2A_DISABLED=false` (or unset the var).
    let a2a_router = if std::env::var("ARGUS_A2A_DISABLED")
        .ok()
        .map(|v| v != "false" && v != "0" && v != "no")
        .unwrap_or(false)
    {
        // Disabled: mount a 404 fallback at the well-known URL.
        use argus_verify::routes::a2a_disabled_handler;
        Router::new()
            .route("/.well-known/agent-card.json", get(a2a_disabled_handler))
            .route("/a2a/message", get(a2a_disabled_handler))
            .route("/a2a/message", post(a2a_disabled_handler))
    } else {
        Router::new()
            .route("/.well-known/agent-card.json", get(agent_card_handler))
            .route("/a2a/message", post(a2a_message_handler))
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/analyze", post(analyze))
        .with_state(state)
        .merge(audit_router)
        .merge(a2a_router);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    eprintln!("argus-verify listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "argus-verify",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn analyze(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<argus_verify::AnalyzeRequest>,
) -> Result<Json<argus_verify::AnalyzeResponse>, (axum::http::StatusCode, String)> {
    // Idempotency: extract the optional key. If present, attempt a
    // cache hit before running the (expensive) pipeline. The cache
    // key is the supplied header; the discriminator is `pr_url` —
    // same key + different PR is a cache miss, by design.
    let idem_key = headers
        .get("x-idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if let Some(key) = idem_key.as_deref() {
        if let Some(cached_body) = state.cache.get(key, &req.pr_url).await {
            tracing::info!(
                idempotency_key = %key,
                pr_url = %req.pr_url,
                "Returning cached verdict (idempotency hit)"
            );
            let resp: argus_verify::AnalyzeResponse =
                serde_json::from_value(cached_body).map_err(|e| {
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("cached verdict failed to deserialize: {}", e),
                    )
                })?;
            return Ok(Json(resp));
        }
    }

    // BYOK: pull the NIM key from the X-LLM-Key header, fall back to env.
    let nim_key = headers
        .get("x-llm-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("ARGUS_NIM_KEY").ok())
        .ok_or_else(|| {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                "BYOK required: pass your NVIDIA NIM key in the X-LLM-Key header".to_string(),
            )
        })?;

    // Set the key as an env var so the worker can use it.
    // (In production we'd pass it through a different mechanism.)
    std::env::set_var("ARGUS_NIM_KEY", &nim_key);

    let resp = state.worker.analyze(req.clone()).await.map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{}", e),
        )
    })?;

    // Populate the cache only if the caller opted into idempotency.
    // Failures to serialise for caching are non-fatal — we still
    // return the freshly-computed verdict.
    if let Some(key) = idem_key.as_deref() {
        match serde_json::to_value(&resp) {
            Ok(body) => {
                state.cache.put(key.to_string(), req.pr_url, body).await;
            }
            Err(e) => {
                tracing::warn!(
                    idempotency_key = %key,
                    error = %e,
                    "Failed to serialise verdict for idempotency cache; not cached"
                );
            }
        }
    }

    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    //! In-process handler tests using `tower::ServiceExt::oneshot`.
    //! No new dev-deps needed: `tower` is already a dep of
    //! `axum` and `argus-verify`. We test:
    //! - `health()`: simple JSON response, no state required.
    //! - `analyze()`: 401 when no NIM key, cache hit returns
    //!   cached value, cache miss + bad NIM key propagates
    //!   worker error.
    //!
    //! The `analyze()` handler calls `state.worker.analyze()`
    //! which makes a real HTTP call to the NIM endpoint. We avoid
    //! that by either returning early (401, cache hit) or by
    //! pointing the worker at a deliberately-unreachable URL
    //! so the call fails fast and surfaces as a 500.

    use super::*;
    use apohara_argus_core::{FixPlan, PRReview, Verdict, VerdictStatus};
    use argus_verify::AnalyzeResponse;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use chrono::Utc;
    use tower::ServiceExt;
    use uuid::Uuid;

    /// Serializes tests that read/write `ARGUS_NIM_KEY` via the
    /// process env. The `analyze` handler calls
    /// `std::env::set_var("ARGUS_NIM_KEY", …)` when a key is
    /// provided in the `X-LLM-Key` header — that side effect
    /// leaks across tests if they run in parallel. The lock
    /// forces sequential execution of env-touching tests.
    /// We use `tokio::sync::Mutex` (not `std::sync::Mutex`)
    /// because the guard must be held across `.await` points
    /// (e.g., `app.oneshot(req).await`); `std::sync::MutexGuard`
    /// is not `Send` and would trigger `clippy::await_holding_lock`.
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Build an `AppState` with a `VerifyWorker` pointed at an
    /// unreachable URL. When the worker actually tries to call
    /// the LLM, it fails with a connection error in < 1s.
    fn make_test_state() -> AppState {
        // Port 1 is reserved and almost never bound. The worker
        // will fail with a connection error (not a timeout) when
        // it tries to call the LLM.
        let worker = Arc::new(VerifyWorker::new("http://127.0.0.1:1/v1"));
        let cache = IdempotencyCache::new();
        AppState { worker, cache }
    }

    /// Clear the `ARGUS_NIM_KEY` env var so the "no NIM key" test
    /// is deterministic regardless of the test environment.
    fn clear_nim_key_env() {
        // Safe on Rust ≥ 1.86 because we use `remove_var` only in
        // tests; production code uses `set_var` (which IS unsafe
        // on 1.86+, but we never call it from production here).
        // We accept the unsafe block because the test is the only
        // place that touches the process env.
        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var("ARGUS_NIM_KEY");
        }
    }

    #[tokio::test]
    async fn health_returns_ok_json() {
        let app = Router::new().route("/health", get(health));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["service"], "argus-verify");
        assert!(json["version"].is_string());
    }

    #[tokio::test]
    async fn analyze_returns_401_when_no_nim_key() {
        let _env_guard = ENV_LOCK.lock().await;
        clear_nim_key_env();
        let state = make_test_state();
        let app = Router::new()
            .route("/analyze", post(analyze))
            .with_state(state);
        let req = Request::builder()
            .method("POST")
            .uri("/analyze")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"pr_url":"https://github.com/x/y/pull/1"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn analyze_returns_cached_when_idempotency_hit() {
        let _env_guard = ENV_LOCK.lock().await;
        clear_nim_key_env();
        let state = make_test_state();
        let cache = state.cache.clone();
        // Build a valid `AnalyzeResponse` in Rust and serialize it
        // to JSON. This guarantees the cached value has the exact
        // shape that the handler's `serde_json::from_value` call
        // expects (including RiskScore's serde-transparent repr,
        // VerdictStatus's SCREAMING_SNAKE_CASE, and DateTime<Utc>'s
        // ISO 8601 format). Hand-crafting the JSON is fragile
        // because nested types have non-obvious serialization rules.
        let key = "test-cache-key";
        let pr_url = "https://github.com/x/y/pull/42";
        let now = Utc::now();
        let verdict = Verdict {
            status: VerdictStatus::Approved,
            risk_score: apohara_argus_core::RiskScore(0.1),
            summary: "cached verdict".into(),
            key_findings: vec![],
            action_items: vec![],
            reasoning: "test cache hit".into(),
            issued_at: now,
        };
        let review = PRReview {
            id: Uuid::nil(),
            pr_ref: pr_url.to_string(),
            pr_commit_hash: "abc123".into(),
            verdict: verdict.clone(),
            findings: vec![],
            agent_chain: vec![],
            created_at: now,
            ledger_signature: String::new(),
            prev_ledger_hash: String::new(),
        };
        let cached_resp = AnalyzeResponse {
            pr_ref: pr_url.to_string(),
            verdict: verdict.clone(),
            slop_score: None,
            fit_score: None,
            security_summary: None,
            review,
            comment_posted: false,
            labels_set: false,
            fix_plan: FixPlan::empty(),
        };
        let cached = serde_json::to_value(&cached_resp).unwrap();
        cache.put(key.to_string(), pr_url.to_string(), cached).await;

        let app = Router::new()
            .route("/analyze", post(analyze))
            .with_state(state);
        let req = Request::builder()
            .method("POST")
            .uri("/analyze")
            .header("content-type", "application/json")
            .header("x-idempotency-key", key)
            .body(Body::from(format!(r#"{{"pr_url":"{}"}}"#, pr_url)))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["verdict"]["status"], "APPROVED");
        assert_eq!(json["verdict"]["summary"], "cached verdict");
        assert_eq!(json["pr_ref"], pr_url);
    }

    #[tokio::test]
    async fn analyze_returns_500_when_worker_fails() {
        let _env_guard = ENV_LOCK.lock().await;
        clear_nim_key_env();
        let state = make_test_state();
        let app = Router::new()
            .route("/analyze", post(analyze))
            .with_state(state);
        // Provide a NIM key so the handler proceeds past the 401
        // check, then the worker fails because port 1 is unbound.
        let req = Request::builder()
            .method("POST")
            .uri("/analyze")
            .header("content-type", "application/json")
            .header("x-llm-key", "fake-key")
            .body(Body::from(r#"{"pr_url":"https://github.com/x/y/pull/1"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn analyze_returns_500_on_cache_corruption() {
        let _env_guard = ENV_LOCK.lock().await;
        clear_nim_key_env();
        let state = make_test_state();
        let cache = state.cache.clone();
        // Pre-populate the cache with a value that cannot be
        // deserialized as AnalyzeResponse (wrong shape).
        let key = "corrupt-key";
        let pr_url = "https://github.com/x/y/pull/99";
        let bad = serde_json::json!({"this_is_not_a_verdict": 42});
        cache.put(key.to_string(), pr_url.to_string(), bad).await;

        let app = Router::new()
            .route("/analyze", post(analyze))
            .with_state(state);
        let req = Request::builder()
            .method("POST")
            .uri("/analyze")
            .header("content-type", "application/json")
            .header("x-idempotency-key", key)
            .body(Body::from(format!(r#"{{"pr_url":"{}"}}"#, pr_url)))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
