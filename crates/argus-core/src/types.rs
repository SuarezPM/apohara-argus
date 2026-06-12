//! Domain types for ARGUS.

use chrono::{DateTime, Utc};
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
}
