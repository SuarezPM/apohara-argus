//! Integration tests for the graceful-shutdown signal handler. [Refs: 6.1]
//!
//! These tests spawn an axum server inside the test process, deliver a
//! signal to the process, and assert that the server's `JoinHandle`
//! returns `Ok(())` within 5 seconds — confirming that
//! `axum::serve(...).with_graceful_shutdown(shutdown_signal())` actually
//! drains in response to SIGINT/SIGTERM.
//!
//! All three tests are serialized via a global `Mutex` because tokio
//! installs a single process-wide signal handler, so concurrent test
//! runs would race over the same signal stream.

use argus_verify::shutdown_signal;
use axum::{routing::get, Json, Router};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;

/// Serializes the signal-driven tests (see module docs).
static SERIAL: Mutex<()> = Mutex::new(());

/// Spin up a bare-bones axum server on a random localhost port, wired
/// to the production `shutdown_signal`. Returns the bound address and
/// the `JoinHandle` of the server task.
async fn spawn_test_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Router::new().route(
        "/health",
        get(|| async { Json(serde_json::json!({"ok": true})) }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind random localhost port");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await;
    });
    (addr, handle)
}

#[tokio::test]
async fn sigterm_triggers_graceful_shutdown() {
    let _guard = SERIAL.lock().expect("SERIAL mutex poisoned");

    let (addr, handle) = spawn_test_server().await;

    // Give the server task a beat to install its signal handlers and
    // start accepting connections before we fire the signal.
    tokio::time::sleep(Duration::from_millis(50)).await;

    kill(Pid::this(), Signal::SIGTERM).expect("send SIGTERM to self");

    match timeout(Duration::from_secs(5), handle).await {
        Ok(Ok(())) => {} // graceful shutdown completed cleanly
        Ok(Err(e)) => panic!("server task panicked on SIGTERM: {e}"),
        Err(_) => {
            panic!("graceful shutdown did not complete within 5s after SIGTERM (addr={addr})")
        }
    }
}

#[tokio::test]
async fn sigint_triggers_graceful_shutdown() {
    let _guard = SERIAL.lock().expect("SERIAL mutex poisoned");

    let (addr, handle) = spawn_test_server().await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    kill(Pid::this(), Signal::SIGINT).expect("send SIGINT to self");

    match timeout(Duration::from_secs(5), handle).await {
        Ok(Ok(())) => {} // graceful shutdown completed cleanly
        Ok(Err(e)) => panic!("server task panicked on SIGINT: {e}"),
        Err(_) => panic!("graceful shutdown did not complete within 5s after SIGINT (addr={addr})"),
    }
}

/// Regression gate: the un-shielded form
/// ```text
///     axum::serve(...).await?
/// ```
/// (with no `.with_graceful_shutdown(...)` chained on within a small
/// window) MUST NOT appear in any production source file under
/// `crates/*/src/**`. Future code that wires up another axum server
/// (argus-agent, argus-lens, etc.) is required to link the same
/// `shutdown_signal` — this test makes regressions visible.
#[test]
fn no_unshielded_axum_serve_in_workspace() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = root
        .parent()
        .and_then(Path::parent)
        .expect("locate workspace root from CARGO_MANIFEST_DIR");

    let mut sources: Vec<PathBuf> = Vec::new();
    collect_rs_sources(&workspace_root.join("crates"), &mut sources);
    // Exclude the test file itself from the gate — it is the place
    // that legitimately constructs a server for verification.
    sources.retain(|p| p != &root.join("tests").join("shutdown.rs"));

    let mut offenders: Vec<String> = Vec::new();
    for path in &sources {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains("axum::serve(") {
                continue;
            }
            // Look ahead 6 lines (a generous window that easily
            // covers a single `.with_graceful_shutdown(...)` chain).
            let window_end = (i + 6).min(lines.len());
            let window = lines[i..window_end].join("\n");
            if window.contains(".with_graceful_shutdown") {
                continue;
            }
            offenders.push(format!("{}:{}  |  {}", path.display(), i + 1, line.trim()));
        }
    }

    assert!(
        offenders.is_empty(),
        "Un-shielded `axum::serve(...)` calls found. Every axum server \
         must be wrapped with `.with_graceful_shutdown(shutdown_signal())` \
         (or equivalent). Offenders:\n  {}",
        offenders.join("\n  ")
    );
}

fn collect_rs_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Recurse only into `src/` and `tests/` subtrees of each
            // crate; we don't need target/ or examples/.
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == "src" || name == "tests" {
                    collect_rs_sources(&path, out);
                }
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
