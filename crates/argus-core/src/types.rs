//! Domain types for ARGUS.

use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single finding emitted by one of the analyzers (slop, security, arch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PRFinding {
    pub id: Uuid,
    pub agent: AgentRole,
    pub severity: FindingSeverity,
    pub file: String,
    pub line: Option<u32>,
    pub category: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// A risk score in [0.0, 1.0].
/// Higher = more risk.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RiskScore(pub f32);

impl RiskScore {
    pub fn new(v: f32) -> Self { Self(v.clamp(0.0, 1.0)) }
    pub fn as_f32(self) -> f32 { self.0 }
    pub fn is_high(self) -> bool { self.0 >= 0.7 }
    pub fn is_critical(self) -> bool { self.0 >= 0.85 }
}

/// The final verdict emitted by the verdict-synthesizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub status: VerdictStatus,
    pub risk_score: RiskScore,
    pub summary: String,
    pub key_findings: Vec<String>,
    pub action_items: Vec<String>,
    pub reasoning: String,
    pub issued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerdictStatus {
    Approved,
    ReviewRequired,
    Halted,
}

/// A complete PR review (output of Aegis Verify).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PRReview {
    pub id: Uuid,
    pub pr_ref: String,
    pub pr_commit_hash: String,
    pub verdict: Verdict,
    pub findings: Vec<PRFinding>,
    pub agent_chain: Vec<AgentAction>,
    pub created_at: DateTime<Utc>,
    pub ledger_signature: String,
    pub prev_ledger_hash: String,
}

/// One action taken by one agent (logged to the audit chain).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub agent: AgentRole,
    pub spiffe_id: String,
    pub action: String,
    pub timestamp: DateTime<Utc>,
    pub ed25519_sig: String,
    pub payload_hash: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AgentRole {
    /// Scopes the PR, recruits the other agents.
    AegisScope,
    /// Adversarial security review.
    AegisSecurity,
    /// AI slop detector.
    AegisSlop,
    /// Architecture fit checker.
    AegisArch,
    /// Final verdict synthesizer.
    AegisVerdict,
    /// Weekly org-wide digest.
    AegisLens,
}

impl AgentRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AegisScope => "aegis-scope",
            Self::AegisSecurity => "aegis-security",
            Self::AegisSlop => "aegis-slop",
            Self::AegisArch => "aegis-arch",
            Self::AegisVerdict => "aegis-verdict",
            Self::AegisLens => "aegis-lens",
        }
    }
}

/// An entry in the signed audit ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub id: Uuid,
    pub kind: LedgerEntryKind,
    pub timestamp: DateTime<Utc>,
    pub actor: String,
    pub payload: serde_json::Value,
    pub prev_hash: String,
    pub hash: String,
    pub ed25519_sig: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerEntryKind {
    PrOpened,
    PrAnalyzed,
    PrVerdict,
    CommitBlocked,
    CommitWarned,
    CommitAllowed,
    WeeklyBriefing,
}

/// Weekly org-wide digest (output of Aegis Lens).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyBriefing {
    pub id: Uuid,
    pub week_starting: chrono::NaiveDate,
    pub org: String,
    pub prs_analyzed: u32,
    pub avg_slop_score: f32,
    pub avg_fit_score: f32,
    pub critical_findings: u32,
    pub top_offenders: Vec<OffenderSummary>,
    pub trend_vs_prev_week: f32,
    pub cto_avatar_script: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffenderSummary {
    pub pr_ref: String,
    pub author: String,
    pub risk_score: f32,
    pub top_finding: String,
}

/// Org-wide summary (GET /api/org/summary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgSummary {
    pub org: String,
    pub total_prs_analyzed: u32,
    pub pct_ai_generated: f32,
    pub avg_risk_score: f32,
    pub by_team: Vec<TeamSummary>,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSummary {
    pub team: String,
    pub pr_count: u32,
    pub avg_risk: f32,
    pub high_risk_count: u32,
}

// =====================================================================
// EU AI Act Article 12 audit log types (Roadmap 2.1)
//
// `AuditEvent` is the canonical record of a single LLM call that produced
// a decision (verdict, classification, tool invocation, etc.). It carries
// the 7 EU AI Act Article 12 required fields plus 4 cost/observability
// fields and 2 hash-chain fields, totalling 13 fields.
//
// GDPR: the cleartext prompt is NEVER stored. Only a BLAKE3 fingerprint
// is kept, so Article 12 logs are GDPR-safe by construction.
// =====================================================================

/// Serde helpers for `[u8; 32]` fields. JSON has no native fixed-size byte
/// array, so we render as lowercase hex. This is the format the audit
/// pipeline, NDJSON exporter, and external auditors all expect.
pub mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(de)?;
        let v = hex::decode(&s).map_err(serde::de::Error::custom)?;
        v.try_into().map_err(|_| serde::de::Error::custom("expected 32-byte hex string"))
    }
}

/// 64-byte Ed25519 signature, hex-encoded for JSON portability.
pub mod hex_signature {
    use ed25519_dalek::Signature;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(sig: &Signature, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&hex::encode(sig.to_bytes()))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Signature, D::Error> {
        let s = String::deserialize(de)?;
        let v = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let bytes: [u8; 64] = v
            .try_into()
            .map_err(|_| serde::de::Error::custom("expected 64-byte hex signature"))?;
        Ok(Signature::from_bytes(&bytes))
    }
}

/// A single tool call made during an LLM turn (e.g. `read_file`, `shell`).
/// We hash inputs and outputs instead of storing cleartext — the audit log
/// stays GDPR-safe even when the tool touches user data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCallRecord {
    pub tool_name: String,
    #[serde(with = "hex_bytes")]
    pub input_hash: [u8; 32],
    #[serde(with = "hex_bytes")]
    pub output_hash: [u8; 32],
    pub latency_ms: u64,
}

/// The final structured decision that an LLM turn produced.
/// Verdict is a closed enum-ish string: "allow" | "warn" | "block".
/// `rationale` is brief — full reasoning lives in the upstream `Verdict`
/// struct's `reasoning` field, which is not duplicated here to keep the
/// audit record compact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionArtifact {
    pub verdict: String,
    pub findings_count: u32,
    pub rationale: String,
}

/// The 13-field EU AI Act Article 12 audit record. See module docs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuditEvent {
    /// Unique identifier (UUIDv4) — never reused, ever.
    pub audit_id: Uuid,
    /// ISO 8601 UTC timestamp of when the LLM call completed.
    pub timestamp: DateTime<Utc>,
    /// Provider model id, e.g. "deepseek-ai/deepseek-v4-flash".
    pub model_id: String,
    /// BLAKE3 hex of the prompt template `.md` file. Lets auditors verify
    /// which version of the prompt was used without bloating the record.
    pub prompt_template_version: String,
    /// BLAKE3(prompt_text). The cleartext prompt is NEVER stored (GDPR).
    #[serde(with = "hex_bytes")]
    pub prompt_fingerprint: [u8; 32],
    /// BLAKE3(raw_response). Same privacy posture as the prompt.
    #[serde(with = "hex_bytes")]
    pub response_fingerprint: [u8; 32],
    /// Sampling temperature used (provider-reported or our request value).
    pub temperature: f32,
    /// Tool calls the LLM made during this turn (may be empty).
    pub tool_calls: Vec<ToolCallRecord>,
    /// Estimated input tokens (provider usage if available, else chars/4).
    pub input_tokens: u32,
    /// Estimated output tokens (provider usage if available, else chars/4).
    pub output_tokens: u32,
    /// Estimated cost in USD based on a static pricing table.
    pub estimated_cost_usd: f64,
    /// The decision this LLM call produced.
    pub decision: DecisionArtifact,
    /// BLAKE3 hash of the previous `AuditEvent` in the same session chain.
    #[serde(with = "hex_bytes")]
    pub prev_hash: [u8; 32],
    /// Ed25519 signature over the canonical JSON of this event
    /// (with the signature field zeroed). Verifies the record was
    /// produced by the holder of the audit signing key.
    #[serde(with = "hex_signature")]
    pub signature: Signature,
}
// =====================================================================
// EU AI Act Article 12 — compliance export manifest (Roadmap 2.2)
//
// The `Manifest` is the last line of the NDJSON stream emitted by
// `GET /audit/export`. It lets an auditor verify the body they hold is
// the same one we held at export time: re-compute the BLAKE3 hash from
// each event line and compare against `b3_hash`.
//
// `b3_hash` is computed over the canonical JSON of each event in arrival
// order, with the per-event `signature` field zeroed. Zeroing the
// signature makes the manifest stable across re-signing — signatures are
// volatile in a forward-secure design, and the manifest should reflect
// the *content* of the chain, not its signatures.
// =====================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub count: u32,
    pub first_at: Option<DateTime<Utc>>,
    pub last_at: Option<DateTime<Utc>>,
    pub b3_hash: String,
    pub generated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_score_clamps() {
        assert_eq!(RiskScore::new(-0.5).as_f32(), 0.0);
        assert_eq!(RiskScore::new(1.5).as_f32(), 1.0);
        assert_eq!(RiskScore::new(0.42).as_f32(), 0.42);
    }

    #[test]
    fn risk_score_thresholds() {
        assert!(!RiskScore::new(0.5).is_high());
        assert!(RiskScore::new(0.7).is_high());
        assert!(!RiskScore::new(0.7).is_critical());
        assert!(RiskScore::new(0.85).is_critical());
    }

    #[test]
    fn agent_role_str_roundtrip() {
        for role in [
            AgentRole::AegisScope,
            AgentRole::AegisSecurity,
            AgentRole::AegisSlop,
            AgentRole::AegisArch,
            AgentRole::AegisVerdict,
            AgentRole::AegisLens,
        ] {
            assert!(!role.as_str().is_empty());
        }
    }

    // --- Article 12 audit-event tests (Roadmap 2.1) ---

    fn sample_audit_event() -> AuditEvent {
        AuditEvent {
            audit_id: Uuid::nil(),
            timestamp: DateTime::parse_from_rfc3339("2026-06-12T19:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            model_id: "deepseek-ai/deepseek-v4-flash".into(),
            prompt_template_version: "abc123".into(),
            prompt_fingerprint: [0u8; 32],
            response_fingerprint: [1u8; 32],
            temperature: 0.7,
            tool_calls: vec![],
            input_tokens: 100,
            output_tokens: 50,
            estimated_cost_usd: 0.0,
            decision: DecisionArtifact {
                verdict: "warn".into(),
                findings_count: 2,
                rationale: "Two minor slop patterns detected".into(),
            },
            prev_hash: [2u8; 32],
            signature: Signature::from_bytes(&[0u8; 64]),
        }
    }

    #[test]
    fn audit_event_json_roundtrip_preserves_all_13_fields() {
        let event = sample_audit_event();
        let json = serde_json::to_string(&event).expect("serialize");
        let back: AuditEvent = serde_json::from_str(&json).expect("deserialize");

        // Structural equality across every field — proves the roundtrip is lossless.
        assert_eq!(back.audit_id, event.audit_id);
        assert_eq!(back.timestamp, event.timestamp);
        assert_eq!(back.model_id, event.model_id);
        assert_eq!(back.prompt_template_version, event.prompt_template_version);
        assert_eq!(back.prompt_fingerprint, event.prompt_fingerprint);
        assert_eq!(back.response_fingerprint, event.response_fingerprint);
        assert_eq!(back.temperature, event.temperature);
        assert_eq!(back.tool_calls.len(), event.tool_calls.len());
        assert_eq!(back.input_tokens, event.input_tokens);
        assert_eq!(back.output_tokens, event.output_tokens);
        assert_eq!(back.estimated_cost_usd, event.estimated_cost_usd);
        assert_eq!(back.decision, event.decision);
        assert_eq!(back.prev_hash, event.prev_hash);
        assert_eq!(back.signature.to_bytes(), event.signature.to_bytes());

        // Sniff-check JSON shape: 14 top-level keys (one per struct field).
        // (Spec title says "13 fields" but the field list in MUST DO #2
        // has 14 — we go with the field list as the source of truth.)
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 14);
    }

    #[test]
    fn audit_event_gdpr_prompt_fingerprint_is_32_bytes_not_text() {
        // GDPR: the cleartext prompt must NEVER appear in the JSON.
        // The fingerprint is a [u8; 32] hex string, not the original text.
        let prompt_text = "This prompt contains a user email user@example.com and is secret.";
        let event = AuditEvent {
            prompt_fingerprint: *blake3::hash(prompt_text.as_bytes()).as_bytes(),
            response_fingerprint: *blake3::hash(b"response").as_bytes(),
            ..sample_audit_event()
        };
        let json = serde_json::to_string(&event).unwrap();

        // 1. The cleartext prompt must NOT be in the serialized JSON.
        assert!(
            !json.contains(prompt_text),
            "GDPR violation: cleartext prompt leaked into audit JSON"
        );
        assert!(
            !json.contains("user@example.com"),
            "GDPR violation: PII leaked into audit JSON"
        );

        // 2. The fingerprint must be hex-encoded, exactly 64 chars (32 bytes).
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let fp = v["prompt_fingerprint"].as_str().expect("hex string");
        assert_eq!(fp.len(), 64, "fingerprint must be 32-byte hex (64 chars)");
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));

        // 3. The field is NOT a `String` at the type level — it's [u8; 32].
        let back: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.prompt_fingerprint.len(), 32);
        // The actual bytes are BLAKE3 of the prompt, not the prompt.
        let mut prefix = [0u8; 32];
        let bytes = prompt_text.as_bytes();
        let n = bytes.len().min(32);
        prefix[..n].copy_from_slice(&bytes[..n]);
        assert_ne!(
            back.prompt_fingerprint, prefix,
            "fingerprint must not equal the prompt's leading bytes"
        );
    }

    #[test]
    fn tool_call_record_roundtrips() {
        let tc = ToolCallRecord {
            tool_name: "read_file".into(),
            input_hash: [7u8; 32],
            output_hash: [9u8; 32],
            latency_ms: 42,
        };
        let json = serde_json::to_string(&tc).unwrap();
        let back: ToolCallRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tc);
    }
}
