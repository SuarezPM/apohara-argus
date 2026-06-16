//! ArchitectureFit — evaluates whether a PR fits the existing repo's patterns.

use super::{extract_json, Analyzer, SlopError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchReport {
    pub fit_score: f32,
    pub verdict: String,
    pub positives: Vec<String>,
    pub concerns: Vec<ArchConcern>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchConcern {
    pub file: String,
    pub line: Option<u32>,
    pub issue: String,
    pub severity: String,
    pub fix: String,
}

#[derive(Default)]
pub struct ArchitectureFit;

impl ArchitectureFit {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Analyzer for ArchitectureFit {
    type Output = ArchReport;

    fn name(&self) -> &'static str {
        "aegis-arch"
    }
    fn prompt_name(&self) -> &'static str {
        "architecture-fit"
    }

    fn build_user_message(&self, diff: &str, context: Option<&str>) -> String {
        let context = context.unwrap_or("");
        format!(
            "Evaluate whether this PR fits the existing repo architecture.\n\n\
             PR diff:\n```diff\n{}\n```\n\n\
             Repo context (sample of existing files):\n{}\n\n\
             Return ONLY valid JSON.",
            diff, context
        )
    }

    fn parse_response(&self, raw: &str) -> Result<ArchReport, SlopError> {
        let json_str = extract_json(raw);
        serde_json::from_str(&json_str).map_err(|e| SlopError::Parse(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for ArchitectureFit. Cover the public Analyzer trait
    //! surface (name, prompt_name, build_user_message, parse_response)
    //! + serde round-trips for ArchReport/ArchConcern.
    use super::*;

    #[test]
    fn name_and_prompt_name_are_static() {
        // The 4 specialist names are load-bearing: the prompt library
        // looks them up by these exact strings. A typo here would break
        // every analyzer invocation.
        assert_eq!(ArchitectureFit::new().name(), "aegis-arch");
        assert_eq!(ArchitectureFit::new().prompt_name(), "architecture-fit");
    }

    #[test]
    fn new_equals_default() {
        // Both constructors must produce a functionally identical value.
        let from_new = ArchitectureFit::new();
        let from_default = ArchitectureFit;
        assert_eq!(from_new.name(), from_default.name());
        assert_eq!(from_new.prompt_name(), from_default.prompt_name());
    }

    #[test]
    fn build_user_message_includes_diff_and_context() {
        let msg = ArchitectureFit::new().build_user_message("+ let x = 1;", Some("ctx.rs"));
        assert!(msg.contains("+ let x = 1;"), "diff not embedded");
        assert!(msg.contains("ctx.rs"), "context not embedded");
    }

    #[test]
    fn build_user_message_handles_missing_context() {
        // Some callers pass None for context (e.g. for a fresh repo).
        // The prompt must still be well-formed and reference the diff.
        let msg = ArchitectureFit::new().build_user_message("+ let x = 1;", None);
        assert!(msg.contains("+ let x = 1;"));
        assert!(msg.contains("Repo context (sample of existing files):"));
        // No panic + no truncation = OK.
    }

    #[test]
    fn parse_response_strips_json_fence() {
        // LLMs often wrap JSON in ```json ... ```; the analyzer must
        // strip the fence before parsing.
        let raw = "```json\n{\"fit_score\":0.7,\"verdict\":\"ok\",\"positives\":[],\"concerns\":[],\"summary\":\"good\"}\n```";
        let r = ArchitectureFit::new().parse_response(raw).unwrap();
        assert!((r.fit_score - 0.7).abs() < f32::EPSILON);
        assert_eq!(r.verdict, "ok");
    }

    #[test]
    fn parse_response_strips_plain_fence() {
        // Some models emit ``` without the json tag.
        let raw = "```\n{\"fit_score\":0.5,\"verdict\":\"ok\",\"positives\":[],\"concerns\":[],\"summary\":\"\"}\n```";
        let r = ArchitectureFit::new().parse_response(raw).unwrap();
        assert!((r.fit_score - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_response_handles_bare_json() {
        // When the LLM returns clean JSON without fences, parsing must
        // succeed without modification.
        let raw = r#"{"fit_score":0.9,"verdict":"excellent","positives":["good"],"concerns":[],"summary":"solid"}"#;
        let r = ArchitectureFit::new().parse_response(raw).unwrap();
        assert_eq!(r.positives, vec!["good".to_string()]);
    }

    #[test]
    fn parse_response_invalid_returns_parse_error() {
        // Malformed JSON must surface as SlopError::Parse so the
        // pipeline can fall back to a defensive REVIEW_REQUIRED.
        let err = ArchitectureFit::new()
            .parse_response("not json at all")
            .unwrap_err();
        assert!(matches!(err, SlopError::Parse(_)));
    }

    #[test]
    fn arch_report_serde_round_trip() {
        // The report travels as JSON over the audit chain + the
        // web UI; round-trip integrity is non-negotiable.
        let report = ArchReport {
            fit_score: 0.8,
            verdict: "good".to_string(),
            positives: vec!["good naming".to_string()],
            concerns: vec![ArchConcern {
                file: "src/main.rs".to_string(),
                line: Some(42),
                issue: "duplicated logic".to_string(),
                severity: "low".to_string(),
                fix: "extract a helper".to_string(),
            }],
            summary: "looks good".to_string(),
        };
        let j = serde_json::to_string(&report).unwrap();
        let back: ArchReport = serde_json::from_str(&j).unwrap();
        assert_eq!(back.fit_score, report.fit_score);
        assert_eq!(back.concerns.len(), 1);
        assert_eq!(back.concerns[0].line, Some(42));
    }
}
