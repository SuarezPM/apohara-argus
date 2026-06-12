//! Graceful-shutdown signal handler for the argus-verify HTTP server.
//! [Refs: 6.1]
//!
//! Resolves when the process receives SIGINT (Ctrl+C) or SIGTERM,
//! whichever fires first. Designed to be passed to
//! `axum::serve(...).with_graceful_shutdown(shutdown_signal())` so that
//! in-flight requests are drained before the server returns.
//!
//! Exposed as a public item on the crate root (via `pub mod shutdown;`
//! in `lib.rs`) so that integration tests in `tests/shutdown.rs` can
//! drive the same future that production uses.

/// Resolves when the process receives SIGINT (Ctrl+C) or SIGTERM,
/// whichever fires first.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => { tracing::info!("SIGINT received, draining in-flight requests..."); }
        _ = terminate => { tracing::info!("SIGTERM received, draining in-flight requests..."); }
    }
}
