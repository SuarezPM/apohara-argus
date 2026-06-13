//! HTTP routes for the audit-export endpoint. [Refs: 2.2]
//!
//! This module hosts:
//! - `GET /audit/export?from=&to=` — NDJSON stream of Article 12 audit events
//!   with a manifest footer (Roadmap 2.2).
//! - `GET /.well-known/agent-card.json` — A2A AgentCard describing our 4
//!   specialists (Roadmap 3.2). Hand-rolled JSON, no `a2a-rust` crate —
//!   it's too immature (24 downloads, single author).
//! - `POST /a2a/message` — minimal A2A message echo handler. Real A2A
//!   protocol compliance is deferred; this is the discoverable surface
//!   for orchestrators that want to call our specialists.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::audit_store::InMemoryAuditStore;

// =====================================================================
// 2.2 — Audit export (NDJSON)
// =====================================================================

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

// =====================================================================
// 3.2 — A2A AgentCard (opt-in, hand-rolled)
// =====================================================================

/// A single skill offered by an ARGUS specialist. The shape is a strict
/// subset of the A2A v1.0 `AgentSkill` schema — orchestrators that
/// speak A2A can read it without a custom parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Free-form tags for filtering (e.g., `["security", "rust"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// The A2A `AgentCard` for ARGUS. Discovers the 4 specialists
/// (slop, security, arch, verdict) plus the weekly digest
/// (lens). Locks us into Google's open protocol without adding
/// the `a2a-rust` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    pub name: String,
    pub description: String,
    pub url: String,
    pub version: String,
    /// A2A protocol version this card conforms to.
    pub protocol_version: String,
    pub skills: Vec<AgentSkill>,
    /// Authentication requirement: `None` for open, `Bearer` for
    /// our BYOK flow. The orchestrator must pass `X-LLM-Key` to
    /// actually invoke us.
    #[serde(default)]
    pub authentication: Option<AuthScheme>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthScheme {
    pub schemes: Vec<String>,
    pub description: String,
}

/// The A2A `message/send` shape (minimal subset).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AMessage {
    /// The role sending the message (`"user"` or `"agent"`).
    pub role: String,
    /// Free-form parts — we only inspect `parts[0].text` for
    /// routing hints; everything else is opaque to us.
    pub parts: Vec<A2APart>,
    /// Optional explicit target specialist (`"slop"`, `"security"`,
    /// `"arch"`, `"verdict"`, `"lens"`). If absent, the orchestrator
    /// chose for us and we just ack.
    #[serde(default)]
    pub target_skill: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2APart {
    /// The kind of part. We only handle `"text"` today; other kinds
    /// are passed through opaquely.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
}

/// Build the static AgentCard for this server. The URL is templated
/// from the request — see `agent_card_handler` for the live version.
pub fn build_agent_card(base_url: &str) -> AgentCard {
    AgentCard {
        name: "ARGUS".to_string(),
        description: "AI slop defense layer for code review. 4 specialists (slop, security, arch, verdict) plus a weekly digest (lens). EU AI Act Article 12 audit trail built-in. BYOK — your NIM key, your code.".to_string(),
        url: format!("{}/.well-known/agent-card.json", base_url.trim_end_matches('/')),
        version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: "0.3".to_string(),
        skills: vec![
            AgentSkill {
                id: "slop".to_string(),
                name: "Aegis Slop".to_string(),
                description: "Detects AI-generated code smells: narrative comments, swallowed errors, oversized functions, unused public symbols, TODO stubs. Hybrid: deterministic AST pre-flight + LLM semantic.".to_string(),
                tags: vec!["slop".to_string(), "rust".to_string(), "ast".to_string()],
            },
            AgentSkill {
                id: "security".to_string(),
                name: "Aegis Security".to_string(),
                description: "Adversarial security review: credentials, injection, unsafe panic, unhandled errors.".to_string(),
                tags: vec!["security".to_string(), "owasp".to_string()],
            },
            AgentSkill {
                id: "arch".to_string(),
                name: "Aegis Arch".to_string(),
                description: "Architectural fit review: coherence with the existing repo, patterns, idioms.".to_string(),
                tags: vec!["architecture".to_string()],
            },
            AgentSkill {
                id: "verdict".to_string(),
                name: "Aegis Verdict".to_string(),
                description: "Synthesizes the other 3 outputs into a final verdict (Approved / ReviewRequired / Halted) with a structured hand-off plan for downstream coding agents.".to_string(),
                tags: vec!["synthesis".to_string()],
            },
            AgentSkill {
                id: "lens".to_string(),
                name: "Aegis Lens".to_string(),
                description: "Weekly digest: aggregate findings, top offenders, team themes, executive briefing.".to_string(),
                tags: vec!["lens".to_string(), "weekly".to_string()],
            },
        ],
        authentication: Some(AuthScheme {
            schemes: vec!["Bearer".to_string()],
            description: "BYOK: pass your NVIDIA NIM key as `Bearer` in the `Authorization` header, or as the `X-LLM-Key` header.".to_string(),
        }),
    }
}

/// `GET /.well-known/agent-card.json` — A2A discovery. The URL
/// advertised in the card is templated from the request's `Host`
/// header so the orchestrator can reach the agent back.
pub async fn agent_card_handler(headers: axum::http::HeaderMap) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("argus.local");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let base = format!("{}://{}", scheme, host);
    let card = build_agent_card(&base);
    (StatusCode::OK, Json(card))
}

/// `POST /a2a/message` — minimal A2A message handler. We don't
/// fully implement the protocol yet; we ack with the target skill
/// name (or "default" if unspecified) so orchestrators can confirm
/// connectivity. The real work happens via `/analyze` on the PR
/// review pipeline.
pub async fn a2a_message_handler(Json(msg): Json<A2AMessage>) -> impl IntoResponse {
    let skill = msg.target_skill.as_deref().unwrap_or("default");
    let text_excerpt: String = msg
        .parts
        .iter()
        .find_map(|p| p.text.clone())
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect();
    tracing::info!(
        target_skill = %skill,
        parts = msg.parts.len(),
        text_chars = text_excerpt.chars().count(),
        "A2A message received"
    );
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "received",
            "target_skill": skill,
            "echo_text_excerpt": text_excerpt,
            "note": "ARGUS specialist routing is via POST /analyze; this endpoint confirms connectivity. Full A2A protocol support is roadmap 5 (MCP server)."
        })),
    )
}

/// Marker for the 404 fallback when A2A is disabled. Used by `main.rs`
/// to merge the A2A sub-router conditionally. Must be `pub` (not
/// `pub(crate)`) so the binary entry point can import it.
#[allow(dead_code)]
pub async fn a2a_disabled_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": "A2A surface disabled",
            "hint": "Set ARGUS_A2A_DISABLED=false (or unset) to enable the A2A AgentCard and /a2a/message endpoints."
        })),
    )
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_card_lists_five_skills() {
        let card = build_agent_card("http://localhost:8080");
        assert_eq!(card.name, "ARGUS");
        assert_eq!(card.protocol_version, "0.3");
        assert_eq!(card.skills.len(), 5);
        let skill_ids: Vec<&str> = card.skills.iter().map(|s| s.id.as_str()).collect();
        assert!(skill_ids.contains(&"slop"));
        assert!(skill_ids.contains(&"security"));
        assert!(skill_ids.contains(&"arch"));
        assert!(skill_ids.contains(&"verdict"));
        assert!(skill_ids.contains(&"lens"));
    }

    #[test]
    fn agent_card_url_strips_trailing_slash() {
        let card = build_agent_card("http://localhost:8080/");
        assert_eq!(
            card.url,
            "http://localhost:8080/.well-known/agent-card.json"
        );
    }

    #[test]
    fn agent_card_serializes_to_well_known_url() {
        let card = build_agent_card("https://argus.example.com");
        let json = serde_json::to_value(&card).unwrap();
        assert_eq!(
            json["url"],
            "https://argus.example.com/.well-known/agent-card.json"
        );
        assert!(json["authentication"]["schemes"].is_array());
    }

    #[test]
    fn a2a_message_deserializes_with_target_skill() {
        let raw = serde_json::json!({
            "role": "user",
            "parts": [{ "type": "text", "text": "review PR #42" }],
            "target_skill": "verdict"
        });
        let msg: A2AMessage = serde_json::from_value(raw).unwrap();
        assert_eq!(msg.role, "user");
        assert_eq!(msg.target_skill.as_deref(), Some("verdict"));
        assert_eq!(msg.parts.len(), 1);
        assert_eq!(msg.parts[0].text.as_deref(), Some("review PR #42"));
    }

    #[test]
    fn a2a_message_handles_missing_target_skill() {
        let raw = serde_json::json!({
            "role": "user",
            "parts": []
        });
        let msg: A2AMessage = serde_json::from_value(raw).unwrap();
        assert!(msg.target_skill.is_none());
        assert_eq!(msg.parts.len(), 0);
    }
}
