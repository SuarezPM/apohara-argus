//! Integration tests for the `ARGUS_PREMIUM` open-core gate.
//!
//! These tests drive the 5 enterprise routes through the real axum
//! router (no subprocess, no reqwest) via `tower::ServiceExt::oneshot`.
//! The `premium` flag is set directly on the `AppState` so the tests
//! are deterministic and don't need to mutate the process env var.
//!
//! Spec: P.4 (argus-silver-roadmap). The 6 named tests below are the
//! contract — when the gate is removed or relaxed, the 3 "off" cases
//! are the ones that will start to fail first.

use argus_dashboard::premium::routes;
use argus_dashboard::state::AppState;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

/// Build a deterministic `AppState` for the gate tests. The worker and
/// briefing path are placeholders; the gate short-circuits before
/// either is touched, so they are never read.
fn test_state(premium: bool) -> AppState {
    AppState {
        worker: Arc::new(argus_verify::VerifyWorker::new("test")),
        nim_model: "test-model".to_string(),
        briefings_path: PathBuf::from("/tmp/argus-dashboard-test"),
        premium,
    }
}

/// Build the premium subrouter wired to a `premium == premium_on` state.
fn app_with(premium: bool) -> axum::Router {
    routes().with_state(test_state(premium))
}

/// Send a GET to the given URI on the router and return the response.
async fn get(app: axum::Router, uri: &str) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("build request"),
    )
    .await
    .expect("in-process response")
}

// ============================================================================
// Premium OFF (the default; no env var) -> 402 Payment Required
// ============================================================================

#[tokio::test]
async fn premium_off_returns_402_on_org_dashboard() {
    let resp = get(app_with(false), "/org/acme/dashboard").await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("application/json"),
        "premium-off body must be JSON, got Content-Type: {ct}"
    );
}

#[tokio::test]
async fn premium_off_returns_402_on_policy_packs() {
    let resp = get(app_with(false), "/policy-packs").await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

#[tokio::test]
async fn premium_off_returns_402_on_splunk_export() {
    let resp = get(app_with(false), "/audit-log/export.splunk").await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// ============================================================================
// Premium ON (ARGUS_PREMIUM=true in the env) -> 200 OK with stub body
// ============================================================================

#[tokio::test]
async fn premium_on_returns_200_on_org_dashboard() {
    let resp = get(app_with(true), "/org/acme/dashboard").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn premium_on_returns_200_on_policy_packs() {
    let resp = get(app_with(true), "/policy-packs").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn premium_on_returns_200_on_splunk_export() {
    let resp = get(app_with(true), "/audit-log/export.splunk").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ============================================================================
// Body shape — the 402 body is the machine-readable contract for the SDK
// ============================================================================

#[tokio::test]
async fn premium_off_body_has_three_required_fields() {
    // Not in the original 6-test spec, but a thin extra: locks the
    // JSON body shape so a future refactor of premium_required_response
    // cannot silently drop a field without breaking a test.
    let resp = get(app_with(false), "/policy-packs").await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
    let body = axum::body::to_bytes(resp.into_body(), 4096)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("body is valid JSON");
    assert_eq!(v["error"], "premium_required");
    assert_eq!(v["tier"], "Enterprise");
    assert_eq!(v["url"], "/pricing");
}

#[tokio::test]
async fn premium_off_gates_all_three_siem_formats() {
    // The spec lists 3 SIEM routes (Splunk / Datadog / Elastic). The
    // 6-test spec only spot-checks Splunk; this extra asserts the
    // other two also gate, so a future copy-paste that forgets one
    // route fails here.
    for path in [
        "/audit-log/export.splunk",
        "/audit-log/export.datadog",
        "/audit-log/export.elastic",
    ] {
        let resp = get(app_with(false), path).await;
        assert_eq!(
            resp.status(),
            StatusCode::PAYMENT_REQUIRED,
            "premium-off should 402 on {path}"
        );
    }
}

#[tokio::test]
async fn premium_on_admits_all_three_siem_formats() {
    for path in [
        "/audit-log/export.splunk",
        "/audit-log/export.datadog",
        "/audit-log/export.elastic",
    ] {
        let resp = get(app_with(true), path).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "premium-on should 200 on {path}"
        );
    }
}
