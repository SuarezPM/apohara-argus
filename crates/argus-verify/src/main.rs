//! argus-verify HTTP server
//!
//! Listens on ARGUS_API_PORT (default 8080). Endpoints:
//! - POST /analyze   { pr_url, repo_context?, post_comment?, set_labels? }
//! - GET  /health
//!
//! The NIM key is BYOK: pass it in the `X-LLM-Key` header. The server
//! also accepts ARGUS_NIM_KEY env var as a fallback.

use argus_verify::{shutdown_signal, VerifyWorker};
use axum::{extract::State, http::HeaderMap, response::IntoResponse, routing::{get, post}, Json, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    worker: Arc<VerifyWorker>,
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

    let state = AppState { worker: Arc::new(worker) };

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

    let resp = state.worker.analyze(req).await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
    Ok(Json(resp))
}
