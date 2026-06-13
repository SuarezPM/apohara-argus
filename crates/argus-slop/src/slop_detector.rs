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
