//! argus-verify HTTP server
//!
//! Listens on ARGUS_API_PORT (default 8080). Endpoints:
//! - POST /analyze   { pr_url, repo_context?, post_comment?, set_labels? }
//! - GET  /health
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

use argus_verify::{shutdown_signal, IdempotencyCache, VerifyWorker};
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
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,argus=debug")))
        .init();

    let port: u16 = std::env::var("ARGUS_API_PORT")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(8080);

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
                    tracing::info!(
                        removed,
                        "Cleaned up expired idempotency entries"
                    );
                }
            }
        });
    }

    let state = AppState {
        worker: Arc::new(worker),
        cache,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/analyze", post(analyze))
        .with_state(state);

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
            let resp: argus_verify::AnalyzeResponse = serde_json::from_value(cached_body)
                .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("cached verdict failed to deserialize: {}", e)))?;
            return Ok(Json(resp));
        }
    }

    // BYOK: pull the NIM key from the X-LLM-Key header, fall back to env.
    let nim_key = headers.get("x-llm-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("ARGUS_NIM_KEY").ok())
        .ok_or_else(|| (axum::http::StatusCode::UNAUTHORIZED,
            "BYOK required: pass your NVIDIA NIM key in the X-LLM-Key header".to_string()))?;

    // Set the key as an env var so the worker can use it.
    // (In production we'd pass it through a different mechanism.)
    std::env::set_var("ARGUS_NIM_KEY", &nim_key);

    let resp = state.worker.analyze(req.clone()).await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

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
