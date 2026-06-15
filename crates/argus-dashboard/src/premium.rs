//! The 5 enterprise (premium) dashboard routes.
//!
//! These surfaces are paid-tier only. When `ARGUS_PREMIUM` is unset (or
//! set to anything other than the canonical `"true"` / `"1"`) every
//! route in this module returns **HTTP 402 Payment Required** with a
//! fixed JSON body pointing the visitor to `/pricing`.
//!
//! The actual feature implementations (multi-tenant org dashboards,
//! custom policy packs, Splunk / Datadog / Elastic SIEM export) are
//! post-P.4. For now the premium-on branches return a stub HTML page
//! with a roadmap note. The deliverable for P.4 is the gate, not the
//! features behind it.
//!
//! Why 5 routes and not 1? The dashboard premium surface is a *family*
//! of features, not a single endpoint. Org dashboards and policy packs
//! are user-facing views; the 3 SIEM exports are machine-facing data
//! feeds in different wire formats. Each is its own route so the gate
//! can be lifted per-feature in the future without re-architecting.
//!
//! Why 402 and not 401 / 403? 402 is the canonical "payment required"
//! status and signals the exact remediation (subscribe to the
//! Enterprise tier). 401 would be wrong because no credentials are
//! missing; 403 would be wrong because access is gated by tier, not by
//! identity. The JSON body's `tier` and `url` fields give a machine-
//! readable remediation.

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::state::AppState;

/// The canonical 402 body. Kept as a const so the bytes-on-the-wire
/// stay stable; if the schema ever changes, bump a `premium_required_v2`
/// and keep this one for back-compat clients.
const PREMIUM_REQUIRED_JSON: &str =
    r#"{"error":"premium_required","tier":"Enterprise","url":"/pricing"}"#;

/// Build the 402 response. Content-Type is set explicitly so the
/// caller (curl, a Python script, an SDK) can branch on it without
/// sniffing the body.
pub fn premium_required_response() -> Response {
    (
        StatusCode::PAYMENT_REQUIRED,
        [(header::CONTENT_TYPE, "application/json")],
        PREMIUM_REQUIRED_JSON,
    )
        .into_response()
}

/// Render the stub page served when `premium == true` for a route whose
/// real implementation is post-P.4. The title is injected by the
/// caller (e.g. "Org dashboard: acme", "Custom policy packs").
fn stub_with_title(title: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>ARGUS — {title} (Enterprise)</title>
  <style>
    body {{ font-family: system-ui, sans-serif; max-width: 720px; margin: 60px auto; padding: 0 24px; color: #222; line-height: 1.6; }}
    h1 {{ color: #111; }}
    .tier {{ display: inline-block; background: #f78166; color: #0e1116; padding: 2px 10px; border-radius: 100px; font-size: 12px; font-weight: 700; text-transform: uppercase; margin-left: 8px; }}
    code {{ background: #f6f6f6; padding: 2px 6px; border-radius: 3px; }}
    .box {{ background: #fafafa; border-left: 4px solid #f78166; padding: 12px 16px; margin: 20px 0; border-radius: 4px; }}
  </style>
</head>
<body>
  <h1>{title} <span class="tier">Enterprise</span></h1>
  <div class="box">
    This Enterprise-tier surface is gated behind
    <code>ARGUS_PREMIUM=true</code>. The stub response confirms the
    gate is wired correctly. The real implementation ships in
    <strong>v0.5.0</strong>. See <a href="/pricing">/pricing</a> for
    the three tiers and <a href="https://github.com/SuarezPM/apohara-argus/blob/main/docs/pricing.md">docs/pricing.md</a>
    for the Enterprise line items.
  </div>
  <p><a href="/">Back to ARGUS</a></p>
</body>
</html>"##
    )
}

/// `GET /org/:org_id/dashboard` — multi-tenant org view.
///
/// Premium-on: renders the stub page. Premium-off: 402.
pub async fn org_dashboard(State(state): State<AppState>, Path(org_id): Path<String>) -> Response {
    if !state.premium {
        return premium_required_response();
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        stub_with_title(&format!("Org dashboard: {}", org_id)),
    )
        .into_response()
}

/// `GET /policy-packs` — custom policy packs.
///
/// Premium-on: renders the stub page. Premium-off: 402.
pub async fn policy_packs(State(state): State<AppState>) -> Response {
    if !state.premium {
        return premium_required_response();
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        stub_with_title("Custom policy packs"),
    )
        .into_response()
}

/// `GET /audit-log/export.splunk` — Splunk HEC export.
///
/// Premium-on: stub text/plain body (the real Splunk HEC envelope is
/// post-P.4). Premium-off: 402.
pub async fn export_splunk(State(state): State<AppState>) -> Response {
    if !state.premium {
        return premium_required_response();
    }
    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        "# Splunk HEC export — coming in v0.5.0\n# See docs/pricing.md for the Enterprise tier.\n",
    )
        .into_response()
}

/// `GET /audit-log/export.datadog` — Datadog Logs export.
///
/// Premium-on: stub text/plain body. Premium-off: 402.
pub async fn export_datadog(State(state): State<AppState>) -> Response {
    if !state.premium {
        return premium_required_response();
    }
    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        "# Datadog Logs export — coming in v0.5.0\n# See docs/pricing.md for the Enterprise tier.\n",
    )
        .into_response()
}

/// `GET /audit-log/export.elastic` — Elastic Stack export.
///
/// Premium-on: stub text/plain body. Premium-off: 402.
pub async fn export_elastic(State(state): State<AppState>) -> Response {
    if !state.premium {
        return premium_required_response();
    }
    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        "# Elastic Stack export — coming in v0.5.0\n# See docs/pricing.md for the Enterprise tier.\n",
    )
        .into_response()
}

/// Build the premium subrouter with the 5 gated routes. The subrouter
/// is `Router<AppState>` so `main.rs` can merge it into the main app
/// router (which carries the same `AppState`) without a type cast.
///
/// Exposed at the lib root as `argus_dashboard::premium::routes` so the
/// integration tests in `tests/premium_gate.rs` can construct a
/// premium-only router with a controlled `premium` flag and exercise
/// the gate in-process.
pub fn routes() -> Router<AppState> {
    Router::new()
        // axum 0.8 changed the path-segment capture syntax from
        // `:capture` (axum 0.7) to `{capture}` (matchit 0.8).
        // The handler still receives the value via
        // `Path(org_id): Path<String>` — the variable name is
        // what binds to the `{org_id}` capture group.
        .route("/org/{org_id}/dashboard", get(org_dashboard))
        .route("/policy-packs", get(policy_packs))
        .route("/audit-log/export.splunk", get(export_splunk))
        .route("/audit-log/export.datadog", get(export_datadog))
        .route("/audit-log/export.elastic", get(export_elastic))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use std::path::PathBuf;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state(premium: bool) -> AppState {
        // The gate short-circuits before the worker is ever invoked, so
        // the worker / path placeholders are safe. `with_premium_from_env`
        // is bypassed here on purpose: tests need a deterministic value.
        AppState {
            worker: Arc::new(argus_verify::VerifyWorker::new("test")),
            nim_model: "test-model".to_string(),
            briefings_path: PathBuf::from("/tmp/argus-dashboard-test"),
            premium,
        }
    }

    /// Build a premium-only router with the given `premium` flag.
    fn router(premium: bool) -> Router {
        routes().with_state(test_state(premium))
    }

    async fn get(app: Router, uri: &str) -> Response {
        app.oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request build"),
        )
        .await
        .expect("response")
    }

    #[tokio::test]
    async fn off_returns_402_on_org_dashboard() {
        let resp = get(router(false), "/org/acme/dashboard").await;
        assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.starts_with("application/json"), "got content-type: {ct}");
    }

    #[tokio::test]
    async fn off_returns_402_on_policy_packs() {
        let resp = get(router(false), "/policy-packs").await;
        assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
    }

    #[tokio::test]
    async fn off_returns_402_on_splunk_export() {
        let resp = get(router(false), "/audit-log/export.splunk").await;
        assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
    }

    #[tokio::test]
    async fn on_returns_200_on_org_dashboard() {
        let resp = get(router(true), "/org/acme/dashboard").await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn on_returns_200_on_policy_packs() {
        let resp = get(router(true), "/policy-packs").await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn on_returns_200_on_splunk_export() {
        let resp = get(router(true), "/audit-log/export.splunk").await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn premium_required_body_has_three_fields() {
        let resp = get(router(false), "/policy-packs").await;
        let body = axum::body::to_bytes(resp.into_body(), 4096)
            .await
            .expect("body bytes");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("valid json");
        assert_eq!(v["error"], "premium_required");
        assert_eq!(v["tier"], "Enterprise");
        assert!(
            v["url"].as_str().is_some_and(|s| !s.is_empty()),
            "url field must be non-empty"
        );
    }
}
