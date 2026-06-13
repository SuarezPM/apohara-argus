//! VerdictSynthesizer — emits the final verdict from the 3 analyzer outputs.

use super::{extract_json, Analyzer, SlopError};
use argus_core::{RiskScore, Verdict, VerdictStatus};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesizerInput {
    pub pr_ref: String,
    pub pr_diff: String,
    pub slop_report: serde_json::Value,
    pub security_report: serde_json::Value,
    pub architecture_report: serde_json::Value,
}

pub struct VerdictSynthesizer;

impl VerdictSynthesizer {
    pub fn new() -> Self {
        Self
    }

    /// Build a Verdict from a synthesizer response.
    pub fn to_verdict(
        &self,
        status: VerdictStatus,
        risk: f32,
        summary: String,
        key_findings: Vec<String>,
        action_items: Vec<String>,
        reasoning: String,
    ) -> Verdict {
        Verdict {
            status,
            risk_score: RiskScore::new(risk),
            summary,
            key_findings,
            action_items,
            reasoning,
            issued_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SynthesizerResponse {
    verdict: String, // APPROVED | REVIEW_REQUIRED | HALTED
    risk_score: f32,
    summary: String,
    key_findings: Vec<String>,
    action_items: Vec<String>,
    reasoning: String,
}

#[async_trait]
impl Analyzer for VerdictSynthesizer {
    type Output = Verdict;

    fn name(&self) -> &'static str {
        "aegis-verdict"
    }
    fn prompt_name(&self) -> &'static str {
        "verdict-synthesizer"
    }

    fn build_user_message(&self, _diff: &str, _context: Option<&str>) -> String {
        // VerdictSynthesizer uses a custom message built in `run` instead,
        // because it needs the 3 prior outputs serialized.
        String::new()
    }

    fn parse_response(&self, raw: &str) -> Result<Verdict, SlopError> {
        let json_str = extract_json(raw);
        let resp: SynthesizerResponse =
            serde_json::from_str(&json_str).map_err(|e| SlopError::Parse(e.to_string()))?;
        let status = match resp.verdict.as_str() {
            "APPROVED" => VerdictStatus::Approved,
            "HALTED" => VerdictStatus::Halted,
            _ => VerdictStatus::ReviewRequired,
        };
        Ok(self.to_verdict(
            status,
            resp.risk_score,
            resp.summary,
            resp.key_findings,
            resp.action_items,
            resp.reasoning,
        ))
    }
}
