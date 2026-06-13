//! HTTP routes for the audit-export endpoint. [Refs: 2.2]
//!
//! Today this module hosts the `GET /audit/export?from=&to=` NDJSON
//! stream and its handler. Future audit-adjacent routes (per-PR
//! drilldown, retention summary, etc.) belong in this file too.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::audit_store::InMemoryAuditStore;

/// Query string for `GET /audit/export`.
///
/// Both bounds are optional. When omitted:
/// - `from` defaults to `now - 365 days` (full year of audit history)
/// - `to`   defaults to `now` (live tail)
///
/// Bounds are inclusive on both ends. The handler treats them as
/// `event.timestamp ∈ [from, to]`.
#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    pub from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub to: Option<DateTime<Utc>>,
}

/// NDJSON body: one JSON-encoded `AuditEvent` per line, followed by a
/// single manifest footer line prefixed with `# manifest: `.
///
/// `application/x-ndjson` is the de-facto standard MIME type for
/// newline-delimited JSON. Auditors can stream the response line by
/// line and re-derive the manifest hash without holding the whole
/// body in memory.
pub async fn audit_export_handler(
    State(store): State<InMemoryAuditStore>,
    Query(q): Query<ExportQuery>,
) -> impl IntoResponse {
    let now = Utc::now();
    let from = q.from.unwrap_or_else(|| now - Duration::days(365));
    let to = q.to.unwrap_or(now);

    let events = store.query_range(from, to).await;
    let manifest = store.manifest_for(&events).await;

    let mut body: Vec<u8> = Vec::with_capacity(events.len() * 320 + 512);
    for event in &events {
        let line = serde_json::to_string(event).expect("AuditEvent must serialize");
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    let manifest_line = format!(
        "# manifest: {}\n",
        serde_json::to_string(&manifest).expect("Manifest must serialize")
    );
    body.extend_from_slice(manifest_line.as_bytes());

    (
        StatusCode::OK,
        [("content-type", "application/x-ndjson")],
        body,
    )
}
