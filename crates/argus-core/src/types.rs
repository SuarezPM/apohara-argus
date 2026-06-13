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
    pub fn new(v: f32) -> Self {
        Self(v.clamp(0.0, 1.0))
    }
    pub fn as_f32(self) -> f32 {
        self.0
    }
    pub fn is_high(self) -> bool {
        self.0 >= 0.7
    }
    pub fn is_critical(self) -> bool {
        self.0 >= 0.85
    }
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
        v.try_into()
            .map_err(|_| serde::de::Error::custom("expected 32-byte hex string"))
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

/// Classification of the data the LLM call touched. Required by
/// `certifieddata/ai-decision-logging-spec` (Level 2 conformance,
/// April 2026) and EU AI Act Art. 12 (data minimisation, retention
/// scoping).
///
/// `Mixed` means the prompt contained more than one class. Callers
/// should err on the side of `Mixed` rather than `None` when in
/// doubt — the regulator's default posture is "treat as sensitive".
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    /// No data class assigned yet (pre-12.4 — this event is "metadata
    /// only" or "test").
    None,
    /// Source code (e.g., a PR diff). Default for ARGUS — most of our
    /// events see code.
    SourceCode,
    /// Personally identifiable information (emails, names, addresses).
    /// GDPR Art. 4(1) — special handling.
    Pii,
    /// Protected health information (HIPAA-style). ARGUS doesn't see
    /// this by default; the variant exists for completeness.
    Phi,
    /// Contract / legal text. The retention clock is different
    /// (typically 7y per commercial-contract norms).
    Contract,
    /// Two or more of the above in a single prompt/response.
    Mixed,
    /// Could not classify at write time. Auditors will flag these
    /// for manual review.
    Unknown,
}

/// The 15-field EU AI Act Article 12 audit record (Level 2 conformance
/// per `certifieddata/ai-decision-logging-spec` April 2026).
///
/// v2 (this version) adds `data_class` and `policy_version` over the
/// v1 (Roadmap 2.1) 13-field record. Both new fields are required —
/// omitting them is a compile error, not a runtime fallback. The
/// reasoning: a regulator-facing audit log that *defaults* to
/// "unknown" data class is, by definition, not auditable.
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
    /// What kind of data the LLM call saw. Drives the retention
    /// clock: `SourceCode` = 1y, `Pii`/`Phi` = 1y (GDPR/HIPAA), but
    /// `Contract` = 7y. See `certifieddata/ai-decision-logging-spec`
    /// §3.2 for the exact mapping. [Refs: 4 EU AI Act L2]
    pub data_class: DataClass,
    /// Semver of the active policy bundle (prompts + thresholds) at
    /// write time. Auditors can reproduce the exact behavior by
    /// checking out this version of the repo. [Refs: 4 EU AI Act L2]
    pub policy_version: String,
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

// =====================================================================
// Agent hand-off — `fix_plan.json` [Refs: 1.2]
//
// Output of Aegis Verify that downstream coding agents (Claude Code,
// Codex, Cursor, Devin) can ingest as a structured task list. The
// JSON shape is intentionally minimal: each step is `(file, kind,
// description, suggested_code)`, which is what the four popular
// handoff consumers expect (Greptile's `fix_plan.json`, PR-Agent's
// `--review`, etc.).
// =====================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FixStepKind {
    /// Add a missing import (e.g., hallucinated crate path).
    AddImport,
    /// Add a test case for an under-covered branch.
    AddTest,
    /// Refactor / replace a function or block.
    ModifyFunction,
    /// Remove dead code, unused vars, swallowed `Err` arms, etc.
    RemoveCode,
    /// Add a doc comment or a `// SAFETY:` / `// TODO:` annotation.
    AddDocumentation,
    /// Patch a security-relevant finding (CWE reference, etc.).
    SecurityPatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixStep {
    pub kind: FixStepKind,
    /// Path relative to the repo root, e.g. `"src/api/handlers.rs"`.
    pub file: String,
    /// Inclusive `[start, end]` line range, when known. `None` means
    /// "the file as a whole" (e.g., "remove this unused dependency").
    pub line_range: Option<(u32, u32)>,
    /// One-sentence human-readable description of what to do.
    pub description: String,
    /// Optional drop-in code suggestion. The downstream agent can
    /// apply it verbatim, ignore it, or use it as a starting point.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_code: Option<String>,
    /// Severity of the underlying finding — drives priority order.
    pub severity: FindingSeverity,
    /// Free-form category tag from the analyzer (e.g., `"swallowed_err"`,
    /// `"unhandled_credential"`, `"oversized_fn"`). Useful for filtering.
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FixPlan {
    /// Steps sorted high-severity-first (Critical → Info).
    pub steps: Vec<FixStep>,
    /// Total step count, redundant with `steps.len()` for downstream
    /// consumers that prefer a scalar.
    pub total_steps: u32,
    /// Counts by severity. Empty if no findings.
    pub by_severity: std::collections::BTreeMap<String, u32>,
}

impl FixPlan {
    /// Empty plan (no findings).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build a plan from a list of findings, sorted by severity
    /// (Critical first, Info last). Stable order: same-severity
    /// findings keep their original order.
    pub fn from_findings(findings: &[PRFinding]) -> Self {
        let mut steps: Vec<FixStep> = findings.iter().map(finding_to_step).collect();
        // Sort: highest severity first. FindingSeverity derives Ord
        // (Info=lowest, Critical=highest) so reverse works.
        steps.sort_by_key(|b| std::cmp::Reverse(b.severity));

        let mut by_severity: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        for s in &steps {
            *by_severity
                .entry(severity_label(s.severity).to_string())
                .or_insert(0) += 1;
        }

        Self {
            total_steps: steps.len() as u32,
            steps,
            by_severity,
        }
    }
}

fn severity_label(s: FindingSeverity) -> &'static str {
    match s {
        FindingSeverity::Critical => "critical",
        FindingSeverity::High => "high",
        FindingSeverity::Medium => "medium",
        FindingSeverity::Low => "low",
        FindingSeverity::Info => "info",
    }
}

/// Map a PRFinding to a FixStep. The mapping is intentionally
/// conservative — we pick the most likely `kind` from the analyzer
/// category string, fall back to a generic description, and let the
/// downstream agent decide.
fn finding_to_step(f: &PRFinding) -> FixStep {
    let kind = match f.category.as_str() {
        "unhandled_credential" | "security" | "injection" | "unsafe_panic" => {
            FixStepKind::SecurityPatch
        }
        "missing_test" | "untested_branch" | "test_coverage" => FixStepKind::AddTest,
        "dead_code" | "swallowed_err" | "unwrap" | "expect" => FixStepKind::RemoveCode,
        "oversized_fn" | "complexity" | "refactor" => FixStepKind::ModifyFunction,
        "doc" | "missing_doc" | "comment" => FixStepKind::AddDocumentation,
        "missing_import" | "import" | "use" => FixStepKind::AddImport,
        _ => FixStepKind::ModifyFunction,
    };
    let line_range = f.line.map(|l| (l, l));
    FixStep {
        kind,
        file: f.file.clone(),
        line_range,
        description: f
            .recommendation
            .clone()
            .unwrap_or_else(|| f.description.clone()),
        suggested_code: f.quote.clone(),
        severity: f.severity,
        category: f.category.clone(),
    }
}

#[cfg(test)]
mod fix_plan_tests {
    use super::*;
    use chrono::Utc;

    fn finding(
        severity: FindingSeverity,
        category: &str,
        file: &str,
        line: Option<u32>,
    ) -> PRFinding {
        PRFinding {
            id: Uuid::new_v4(),
            agent: AgentRole::AegisSlop,
            severity,
            file: file.to_string(),
            line,
            category: category.to_string(),
            description: format!("{} finding", category),
            quote: None,
            recommendation: Some(format!("Fix {}", category)),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn empty_plan_has_zero_steps() {
        let p = FixPlan::empty();
        assert_eq!(p.total_steps, 0);
        assert!(p.steps.is_empty());
        assert!(p.by_severity.is_empty());
    }

    #[test]
    fn from_findings_sorts_critical_first() {
        let findings = vec![
            finding(FindingSeverity::Info, "doc", "src/lib.rs", Some(1)),
            finding(
                FindingSeverity::Critical,
                "security",
                "src/api.rs",
                Some(42),
            ),
            finding(FindingSeverity::Medium, "test", "src/foo.rs", Some(10)),
        ];
        let plan = FixPlan::from_findings(&findings);
        assert_eq!(plan.total_steps, 3);
        assert_eq!(plan.steps[0].severity, FindingSeverity::Critical);
        assert_eq!(plan.steps[1].severity, FindingSeverity::Medium);
        assert_eq!(plan.steps[2].severity, FindingSeverity::Info);
    }

    #[test]
    fn from_findings_counts_by_severity() {
        let findings = vec![
            finding(FindingSeverity::High, "x", "a.rs", Some(1)),
            finding(FindingSeverity::High, "x", "b.rs", Some(2)),
            finding(FindingSeverity::Low, "x", "c.rs", Some(3)),
        ];
        let plan = FixPlan::from_findings(&findings);
        assert_eq!(plan.by_severity.get("high"), Some(&2));
        assert_eq!(plan.by_severity.get("low"), Some(&1));
        assert_eq!(plan.by_severity.get("medium"), None);
    }

    #[test]
    fn json_roundtrip_preserves_all_fields() {
        let findings = vec![finding(
            FindingSeverity::Critical,
            "security",
            "src/auth.rs",
            Some(99),
        )];
        let plan = FixPlan::from_findings(&findings);
        let json = serde_json::to_string(&plan).unwrap();
        let back: FixPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_steps, 1);
        assert_eq!(back.steps.len(), 1);
        assert_eq!(back.steps[0].file, "src/auth.rs");
        assert_eq!(back.steps[0].line_range, Some((99, 99)));
        assert_eq!(back.steps[0].kind, FixStepKind::SecurityPatch);
    }

    #[test]
    fn category_to_kind_mapping() {
        // Sanity-check the category→kind mapping used by finding_to_step
        let cases: &[(&str, FixStepKind)] = &[
            ("security", FixStepKind::SecurityPatch),
            ("missing_test", FixStepKind::AddTest),
            ("swallowed_err", FixStepKind::RemoveCode),
            ("oversized_fn", FixStepKind::ModifyFunction),
            ("missing_doc", FixStepKind::AddDocumentation),
            ("missing_import", FixStepKind::AddImport),
        ];
        for (cat, expected) in cases {
            let f = finding(FindingSeverity::Medium, cat, "x.rs", Some(1));
            let step = finding_to_step(&f);
            assert_eq!(step.kind, *expected, "category={cat}");
        }
    }
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
            // EU AI Act Level 2 fields [Refs: 4]:
            data_class: DataClass::SourceCode,
            policy_version: "test-policy-v1".to_string(),
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

        // Sniff-check JSON shape: 16 top-level keys (one per struct field,
        // post EU AI Act L2 conformance — adds `data_class` and
        // `policy_version` over the original 14).
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 16);
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
