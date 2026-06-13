//! Integration tests for the audit-export endpoint. [Refs: 2.2]
//!
//! These tests exercise the real `audit_export_handler` mounted on an
//! axum `Router`, end-to-end. They do NOT call `/analyze` (which
//! requires `GITHUB_TOKEN` and live network access); instead they
//! populate the `InMemoryAuditStore` directly and verify the wire
//! shape of the NDJSON response.

use argus_core::{AuditEvent, DecisionArtifact};
use argus_verify::{audit_export_handler, InMemoryAuditStore};
use axum::{
    body::to_bytes,
    http::{Request, StatusCode as AxStatus},
    routing::get,
    Router,
};
use chrono::{DateTime, TimeZone, Utc};
use ed25519_dalek::{Signer, SigningKey};
use tower::ServiceExt;
use uuid::Uuid;

fn sample_event(ts: DateTime<Utc>, model: &str, idx: u8) -> AuditEvent {
    let key = SigningKey::generate(&mut rand::rngs::OsRng);
    let mut prompt = [0u8; 32];
    prompt[0] = idx;
    let mut ev = AuditEvent {
        audit_id: Uuid::new_v4(),
        timestamp: ts,
        model_id: model.into(),
        prompt_template_version: "v1".into(),
        prompt_fingerprint: prompt,
        response_fingerprint: [0u8; 32],
        temperature: 0.7,
        tool_calls: vec![],
        input_tokens: 10,
        output_tokens: 5,
        estimated_cost_usd: 0.0,
        decision: DecisionArtifact {
            verdict: "warn".into(),
            findings_count: 1,
            rationale: "test".into(),
        },
        prev_hash: [0u8; 32],
        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
    };
    let mut canonical = ev.clone();
    canonical.signature = ed25519_dalek::Signature::from_bytes(&[0u8; 64]);
    let bytes = serde_json::to_vec(&canonical).unwrap();
    ev.signature = key.sign(&bytes);
    ev
}

fn ts(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap()
}

fn make_router(store: InMemoryAuditStore) -> Router {
    Router::new()
        .route("/audit/export", get(audit_export_handler))
        .with_state(store)
}

async fn append_one(store: &InMemoryAuditStore, ts: DateTime<Utc>, model: &str, idx: u8) -> Uuid {
    let ev = sample_event(ts, model, idx);
    let id = ev.audit_id;
    store.append(ev).await;
    id
}

#[tokio::test]
async fn export_on_empty_store_yields_only_manifest_line() {
    let store = InMemoryAuditStore::new();
    let app = make_router(store);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/audit/export")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), AxStatus::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
        "application/x-ndjson"
    );
    let body_bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let body = String::from_utf8(body_bytes.to_vec()).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("# manifest: "));
    let manifest_json = lines[0].trim_start_matches("# manifest: ");
    let v: serde_json::Value = serde_json::from_str(manifest_json).unwrap();
    assert_eq!(v["count"], 0);
    assert_eq!(v["b3_hash"], "");
    assert!(v["first_at"].is_null());
    assert!(v["last_at"].is_null());
}

#[tokio::test]
async fn export_on_populated_store_yields_one_line_per_event_plus_manifest() {
    let store = InMemoryAuditStore::new();
    let id1 = append_one(&store, ts(2026, 6, 12, 10), "m1", 1).await;
    let id2 = append_one(&store, ts(2026, 6, 12, 11), "m2", 2).await;
    let id3 = append_one(&store, ts(2026, 6, 12, 12), "m3", 3).await;
    let app = make_router(store);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/audit/export")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), AxStatus::OK);
    let body_bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let body = String::from_utf8(body_bytes.to_vec()).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 4, "3 events + 1 manifest line");
    let expected_ids = [id1.to_string(), id2.to_string(), id3.to_string()];
    for (i, line) in lines[..3].iter().enumerate() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["audit_id"].as_str().unwrap(), expected_ids[i]);
    }
    let m_line = lines[3];
    assert!(m_line.starts_with("# manifest: "));
    let m_json = m_line.trim_start_matches("# manifest: ");
    let m: serde_json::Value = serde_json::from_str(m_json).unwrap();
    assert_eq!(m["count"], 3);
    assert_eq!(m["b3_hash"].as_str().unwrap().len(), 64);
}

#[tokio::test]
async fn export_filters_by_query_window() {
    let store = InMemoryAuditStore::new();
    append_one(&store, ts(2026, 1, 1, 0), "old", 1).await;
    let in1 = append_one(&store, ts(2026, 6, 12, 10), "in", 2).await;
    let in2 = append_one(&store, ts(2026, 6, 12, 11), "in", 3).await;
    append_one(&store, ts(2027, 1, 1, 0), "future", 4).await;
    let app = make_router(store);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/audit/export?from=2026-06-12T00:00:00Z&to=2026-06-12T23:59:59Z")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), AxStatus::OK);
    let body_bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let body = String::from_utf8(body_bytes.to_vec()).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(!body.contains("\"old\""));
    assert!(!body.contains("\"future\""));
    let v0: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let v1: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(v0["audit_id"].as_str().unwrap(), in1.to_string());
    assert_eq!(v1["audit_id"].as_str().unwrap(), in2.to_string());
    let m_json = lines[2].trim_start_matches("# manifest: ");
    let m: serde_json::Value = serde_json::from_str(m_json).unwrap();
    assert_eq!(m["count"], 2);
}
