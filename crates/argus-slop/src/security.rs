//! SecurityReview — adversarial security analysis of a PR diff.

use super::{extract_json, Analyzer, SlopError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReport {
    pub highest_severity: SecuritySeverity,
    pub findings: Vec<SecurityFinding>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum SecuritySeverity {
    None,
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub severity: SecuritySeverity,
    pub file: String,
    pub line: Option<u32>,
    pub category: String,
    pub quote: String,
    pub description: String,
    pub recommendation: String,
}

#[derive(Default)]
pub struct SecurityReview;

impl SecurityReview {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Analyzer for SecurityReview {
    type Output = SecurityReport;

    fn name(&self) -> &'static str {
        "aegis-security"
    }
    fn prompt_name(&self) -> &'static str {
        "redteam-security"
    }

    fn build_user_message(&self, diff: &str, _context: Option<&str>) -> String {
        format!(
            "Adversarially review the following PR diff for security issues. \
             Return ONLY valid JSON.\n\n```diff\n{}\n```",
            diff
        )
    }

    fn parse_response(&self, raw: &str) -> Result<SecurityReport, SlopError> {
        let json_str = extract_json(raw);
        serde_json::from_str(&json_str).map_err(|e| SlopError::Parse(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for SecurityReview. Cover the public Analyzer trait
    //! surface, the SecuritySeverity ordering (Critical is the highest
    //! and None is the lowest, used by the verdict synthesizer to
    //! escalate), and the serde contracts for SecurityReport and
    //! SecurityFinding.
    use super::*;

    #[test]
    fn name_and_prompt_name_are_static() {
        // The 4 specialist names are load-bearing: the prompt library
        // looks them up by these exact strings.
        assert_eq!(SecurityReview::new().name(), "aegis-security");
        assert_eq!(SecurityReview::new().prompt_name(), "redteam-security");
    }

    #[test]
    fn new_equals_default() {
        let from_new = SecurityReview::new();
        let from_default = SecurityReview;
        assert_eq!(from_new.name(), from_default.name());
    }

    #[test]
    fn build_user_message_includes_diff() {
        let msg = SecurityReview::new().build_user_message("+ let x = 1;", Some("ctx"));
        assert!(msg.contains("+ let x = 1;"));
    }

    #[test]
    fn build_user_message_handles_missing_context() {
        // The context is intentionally ignored (`_context`) in this
        // analyzer — the redteam prompt is self-contained.
        let msg = SecurityReview::new().build_user_message("diff", None);
        assert!(msg.contains("diff"));
    }

    #[test]
    fn parse_response_strips_json_fence() {
        let raw =
            "```json\n{\"highest_severity\":\"high\",\"findings\":[],\"summary\":\"risky\"}\n```";
        let r = SecurityReview::new().parse_response(raw).unwrap();
        assert_eq!(r.highest_severity, SecuritySeverity::High);
    }

    #[test]
    fn parse_response_handles_bare_json() {
        let raw = r#"{"highest_severity":"low","findings":[],"summary":"ok"}"#;
        let r = SecurityReview::new().parse_response(raw).unwrap();
        assert_eq!(r.highest_severity, SecuritySeverity::Low);
    }

    #[test]
    fn parse_response_invalid_returns_parse_error() {
        let err = SecurityReview::new().parse_response("garbage").unwrap_err();
        assert!(matches!(err, SlopError::Parse(_)));
    }

    #[test]
    fn security_severity_ordering() {
        // The verdict synthesizer escalates to HALTED on
        // Critical | High (pipeline.rs). This ordering must hold.
        assert!(SecuritySeverity::None < SecuritySeverity::Info);
        assert!(SecuritySeverity::Info < SecuritySeverity::Low);
        assert!(SecuritySeverity::Low < SecuritySeverity::Medium);
        assert!(SecuritySeverity::Medium < SecuritySeverity::High);
        assert!(SecuritySeverity::High < SecuritySeverity::Critical);
    }

    #[test]
    fn security_severity_serde_lowercase() {
        // The model is told to return JSON with lowercase severity
        // strings; the deserializer must accept them and the
        // serializer must emit the same form for round-tripping.
        for (s, expected) in [
            (SecuritySeverity::None, "none"),
            (SecuritySeverity::Info, "info"),
            (SecuritySeverity::Low, "low"),
            (SecuritySeverity::Medium, "medium"),
            (SecuritySeverity::High, "high"),
            (SecuritySeverity::Critical, "critical"),
        ] {
            assert_eq!(
                serde_json::to_string(&s).unwrap(),
                format!("\"{expected}\"")
            );
        }
    }

    #[test]
    fn security_finding_serde_round_trip() {
        let f = SecurityFinding {
            severity: SecuritySeverity::Medium,
            file: "src/auth.rs".to_string(),
            line: Some(123),
            category: "xss".to_string(),
            quote: "innerHTML = user_input".to_string(),
            description: "unescaped user input".to_string(),
            recommendation: "use textContent".to_string(),
        };
        let j = serde_json::to_string(&f).unwrap();
        let back: SecurityFinding = serde_json::from_str(&j).unwrap();
        assert_eq!(back.severity, SecuritySeverity::Medium);
        assert_eq!(back.line, Some(123));
        assert_eq!(back.category, "xss");
    }
}
