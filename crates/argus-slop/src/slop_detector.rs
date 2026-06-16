//! SlopDetector — flags AI-generated code signals in a PR diff.

use super::{extract_json, Analyzer, SlopError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlopReport {
    pub slop_score: f32,
    pub signals_detected: Vec<String>,
    pub specific_examples: Vec<SlopExample>,
    pub confidence: f32,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlopExample {
    pub file: String,
    pub line: Option<u32>,
    pub quote: String,
    pub reason: String,
}

#[derive(Default)]
pub struct SlopDetector;

impl SlopDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Analyzer for SlopDetector {
    type Output = SlopReport;

    fn name(&self) -> &'static str {
        "aegis-slop"
    }
    fn prompt_name(&self) -> &'static str {
        "slop-detector"
    }

    fn build_user_message(&self, diff: &str, _context: Option<&str>) -> String {
        format!(
            "Analyze the following PR diff for AI-generated code signals. \
             Return ONLY valid JSON.\n\n```diff\n{}\n```",
            diff
        )
    }

    fn parse_response(&self, raw: &str) -> Result<SlopReport, SlopError> {
        let json_str = extract_json(raw);
        serde_json::from_str(&json_str).map_err(|e| SlopError::Parse(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for SlopDetector. Cover the public Analyzer trait
    //! surface + the SlopReport and SlopExample serde contracts.
    use super::*;

    #[test]
    fn name_and_prompt_name_are_static() {
        assert_eq!(SlopDetector::new().name(), "aegis-slop");
        assert_eq!(SlopDetector::new().prompt_name(), "slop-detector");
    }

    #[test]
    fn new_equals_default() {
        let from_new = SlopDetector::new();
        let from_default = SlopDetector;
        assert_eq!(from_new.name(), from_default.name());
    }

    #[test]
    fn build_user_message_includes_diff() {
        let msg = SlopDetector::new().build_user_message("+ let x = 1;", None);
        assert!(msg.contains("+ let x = 1;"));
    }

    #[test]
    fn build_user_message_handles_context_ignored() {
        // Like SecurityReview, this analyzer ignores the context
        // parameter (the slop prompt is self-contained).
        let msg = SlopDetector::new().build_user_message("diff", Some("ctx"));
        assert!(msg.contains("diff"));
    }

    #[test]
    fn parse_response_strips_json_fence() {
        let raw = "```json\n{\"slop_score\":0.6,\"signals_detected\":[\"s1\"],\"specific_examples\":[],\"confidence\":0.8,\"reasoning\":\"r\"}\n```";
        let r = SlopDetector::new().parse_response(raw).unwrap();
        assert!((r.slop_score - 0.6).abs() < f32::EPSILON);
        assert_eq!(r.signals_detected, vec!["s1".to_string()]);
    }

    #[test]
    fn parse_response_handles_bare_json() {
        let raw = r#"{"slop_score":0.2,"signals_detected":[],"specific_examples":[],"confidence":0.95,"reasoning":"clean"}"#;
        let r = SlopDetector::new().parse_response(raw).unwrap();
        assert!((r.slop_score - 0.2).abs() < f32::EPSILON);
        assert!((r.confidence - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_response_invalid_returns_parse_error() {
        let err = SlopDetector::new().parse_response("not json").unwrap_err();
        assert!(matches!(err, SlopError::Parse(_)));
    }

    #[test]
    fn slop_report_serde_round_trip() {
        let report = SlopReport {
            slop_score: 0.75,
            signals_detected: vec!["verbose".to_string(), "redundant".to_string()],
            specific_examples: vec![SlopExample {
                file: "src/main.rs".to_string(),
                line: Some(10),
                quote: "// TODO: improve this".to_string(),
                reason: "commented-out code".to_string(),
            }],
            confidence: 0.9,
            reasoning: "multiple slop signals".to_string(),
        };
        let j = serde_json::to_string(&report).unwrap();
        let back: SlopReport = serde_json::from_str(&j).unwrap();
        assert_eq!(back.signals_detected.len(), 2);
        assert_eq!(back.specific_examples.len(), 1);
        assert_eq!(back.specific_examples[0].line, Some(10));
    }

    #[test]
    fn slop_example_serde_round_trip() {
        let e = SlopExample {
            file: "lib.rs".to_string(),
            line: None, // some signals don't have an exact line
            quote: "let result = fetch(url).await?;".to_string(),
            reason: "generic error handling".to_string(),
        };
        let j = serde_json::to_string(&e).unwrap();
        let back: SlopExample = serde_json::from_str(&j).unwrap();
        assert!(back.line.is_none());
    }
}
