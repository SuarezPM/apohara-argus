//! VerdictSynthesizer — emits the final verdict from the 3 analyzer outputs.

use super::{extract_json, Analyzer, SlopError};
use apohara_argus_core::{RiskScore, Verdict, VerdictStatus};
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

#[derive(Default)]
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

#[cfg(test)]
mod tests {
    //! Unit tests for VerdictSynthesizer. Cover the public Analyzer
    //! trait surface + the verdict-string → VerdictStatus mapping
    //! (APPROVED and HALTED and anything-else = REVIEW_REQUIRED) + the
    //! SynthesizerInput contract used by the CordonEnforcer (the
    //! synthesizer never sees raw diff text, only redacted reports).
    use super::*;
    use apohara_argus_core::VerdictStatus;

    #[test]
    fn name_and_prompt_name_are_static() {
        assert_eq!(VerdictSynthesizer::new().name(), "aegis-verdict");
        assert_eq!(
            VerdictSynthesizer::new().prompt_name(),
            "verdict-synthesizer"
        );
    }

    #[test]
    fn new_equals_default() {
        let from_new = VerdictSynthesizer::new();
        let from_default = VerdictSynthesizer;
        assert_eq!(from_new.name(), from_default.name());
    }

    #[test]
    fn build_user_message_is_empty() {
        // The verdict synthesizer uses a custom message built in `run`
        // (it needs the 3 prior outputs serialized as JSON, not the
        // raw diff). The trait method returns an empty string by
        // design — the harness in `run` overrides this.
        let msg = VerdictSynthesizer::new().build_user_message("diff", Some("ctx"));
        assert!(msg.is_empty());
    }

    #[test]
    fn parse_response_approved() {
        let raw = r#"{"verdict":"APPROVED","risk_score":0.2,"summary":"ok","key_findings":[],"action_items":[],"reasoning":"clean"}"#;
        let v = VerdictSynthesizer::new().parse_response(raw).unwrap();
        assert_eq!(v.status, VerdictStatus::Approved);
    }

    #[test]
    fn parse_response_halted() {
        let raw = r#"{"verdict":"HALTED","risk_score":0.95,"summary":"bad","key_findings":["k"],"action_items":[],"reasoning":"critical"}"#;
        let v = VerdictSynthesizer::new().parse_response(raw).unwrap();
        assert_eq!(v.status, VerdictStatus::Halted);
        let risk: f32 = v.risk_score.as_f32();
        assert!(risk > 0.9);
    }

    #[test]
    fn parse_response_unknown_string_maps_to_review_required() {
        // Defensive default: anything that isn't APPROVED or HALTED
        // becomes REVIEW_REQUIRED (safer than APPROVED). This matches
        // the pipeline.rs synthesize() fallback for missing reports.
        let raw = r#"{"verdict":"WAT","risk_score":0.5,"summary":"","key_findings":[],"action_items":[],"reasoning":""}"#;
        let v = VerdictSynthesizer::new().parse_response(raw).unwrap();
        assert_eq!(v.status, VerdictStatus::ReviewRequired);
    }

    #[test]
    fn parse_response_strips_json_fence() {
        let raw = "```json\n{\"verdict\":\"APPROVED\",\"risk_score\":0.1,\"summary\":\"\",\"key_findings\":[],\"action_items\":[],\"reasoning\":\"\"}\n```";
        let v = VerdictSynthesizer::new().parse_response(raw).unwrap();
        assert_eq!(v.status, VerdictStatus::Approved);
    }

    #[test]
    fn parse_response_invalid_returns_parse_error() {
        let err = VerdictSynthesizer::new()
            .parse_response("garbage")
            .unwrap_err();
        assert!(matches!(err, SlopError::Parse(_)));
    }

    #[test]
    fn to_verdict_builds_full_struct() {
        let v = VerdictSynthesizer::new().to_verdict(
            VerdictStatus::Approved,
            0.3,
            "summary".to_string(),
            vec!["k1".to_string(), "k2".to_string()],
            vec!["a1".to_string()],
            "reasoning".to_string(),
        );
        assert_eq!(v.status, VerdictStatus::Approved);
        assert_eq!(v.summary, "summary");
        assert_eq!(v.key_findings.len(), 2);
        assert_eq!(v.action_items.len(), 1);
        assert_eq!(v.reasoning, "reasoning");
    }

    #[test]
    fn to_verdict_handles_high_risk() {
        // The risk_score is clamped 0..=1 by the synthesizer
        // caller (pipeline.rs synthesize uses .clamp(0.0, 1.0)).
        // to_verdict itself does not clamp; we document that
        // by passing 1.0 and verifying the value passes through.
        let v = VerdictSynthesizer::new().to_verdict(
            VerdictStatus::Halted,
            1.0,
            "high".to_string(),
            vec![],
            vec![],
            "x".to_string(),
        );
        let risk: f32 = v.risk_score.as_f32();
        assert!((risk - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn synthesizer_input_serde_round_trip() {
        // The CordonEnforcer guarantees that the diff field is
        // already secret-redacted before it reaches this struct.
        // We round-trip the shape to ensure serde doesn't break
        // the contract.
        let input = SynthesizerInput {
            pr_ref: "pr/123".to_string(),
            pr_diff: "redacted-by-cordon".to_string(),
            slop_report: serde_json::json!({"slop_score": 0.5}),
            security_report: serde_json::json!({"highest_severity": "low"}),
            architecture_report: serde_json::json!({"fit_score": 0.7}),
        };
        let j = serde_json::to_string(&input).unwrap();
        let back: SynthesizerInput = serde_json::from_str(&j).unwrap();
        assert_eq!(back.pr_ref, "pr/123");
        assert_eq!(back.pr_diff, "redacted-by-cordon");
        assert_eq!(back.slop_report["slop_score"], 0.5);
    }

    #[test]
    fn synthesizer_response_private_serde_contract() {
        // SynthesizerResponse is private; we test its serde contract
        // through parse_response (above) and by verifying the
        // structural fields survive a round trip when the JSON is
        // exactly the expected shape.
        let raw = r#"{"verdict":"APPROVED","risk_score":0.0,"summary":"","key_findings":[],"action_items":[],"reasoning":""}"#;
        let v = VerdictSynthesizer::new().parse_response(raw).unwrap();
        assert_eq!(v.status, VerdictStatus::Approved);
        let risk: f32 = v.risk_score.as_f32();
        assert!(risk.abs() < f32::EPSILON);
    }
}
