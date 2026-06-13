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
