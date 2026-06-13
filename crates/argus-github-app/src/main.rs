//! argus-github-app — binary entry point.
//!
//! Boots the axum HTTP server on `PORT` (default 8080). The
//! four routes mounted are:
//! - `GET  /`         — landing page
//! - `GET  /health`   — liveness probe
//! - `GET  /version`  — version info
//! - `GET  /setup`    — manifest + install URL
//! - `POST /webhook`  — receives GitHub events
//!
//! Configuration is read from environment variables at
//! startup. See [`argus_github_app::AppConfig::from_env`] for
//! the full list. The required vars are
//! `ARGUS_APP_WEBHOOK_SECRET` and `ARGUS_APP_INSTALL_TOKEN`;
//! missing the first one is a fatal startup error, missing
//! the second is logged-but-non-fatal (the App still boots and
//! refuses to act on webhooks until the token is set, which
//! gives operators a way to seed the secret + token out of
//! band).

use std::net::SocketAddr;

use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use tracing_subscriber::EnvFilter;

use argus_github_app::{
    app_state::{AppConfig, AppState},
    setup::{health_handler, index_handler, setup_handler, version_handler},
    webhook::webhook_handler,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // OTel stdout exporter + tracing-subscriber fmt layer.
    // Opt-in via `ARGUS_OTEL_DISABLED=true` (default off for
    // zero overhead), matching the pattern in argus-verify's
    // main.rs. The `try_init` is a no-op when a global
    // subscriber is already set.
    let _otel_guard = argus_otel::init("argus-github-app");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,argus=debug")))
        .try_init();

    // Required config: the webhook secret. We fail loudly at
    // startup rather than at first request — operators need to
    // know about misconfiguration immediately.
    let config = AppConfig::from_env().map_err(|e| {
        eprintln!("fatal: invalid configuration: {}", e);
        e
    })?;
    tracing::info!(
        label_pass = %config.label_pass,
        label_warn = %config.label_warn,
        label_fail = %config.label_fail,
        allowed_repos = config.allowed_repos.len(),
        events = ?config.event_allowlist,
        "argus-github-app starting up"
    );

    // The install token is read from the env on every webhook,
    // not cached at startup — GitHub App installers rotate
    // it. We just log whether it's set so operators can spot
    // the misconfiguration in the boot logs.
    match std::env::var("ARGUS_APP_INSTALL_TOKEN") {
        Ok(t) if !t.is_empty() => tracing::info!("ARGUS_APP_INSTALL_TOKEN: present (length {})", t.len()),
        _ => tracing::warn!("ARGUS_APP_INSTALL_TOKEN not set; webhooks will fail until it is"),
    }

    let state = AppState::new(config);
    // axum's default body limit is 2 MiB. The CordonEnforcer
    // accepts up to 10 MiB; we raise the body limit to 11 MiB
    // so the Cordon can see + reject oversize payloads (413),
    // rather than axum itself returning an opaque
    // "length limit exceeded" error.
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/health", get(health_handler))
        .route("/version", get(version_handler))
        .route("/setup", get(setup_handler))
        .route(
            "/webhook",
            post(webhook_handler).layer(DefaultBodyLimit::max(11 * 1024 * 1024)),
        )
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    eprintln!("argus-github-app listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
