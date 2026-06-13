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
