//! argus-github — minimal GitHub API client for ARGUS
//!
//! Just enough to:
//! - Fetch a PR's diff
//! - Post a comment with the verdict
//! - Set a label on the PR
//!
//! Uses the `Authorization: token <PAT>` header. The PAT needs `repo` scope
//! for posting comments and labels.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum GitHubError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("API error: {status} — {message}")]
    Api { status: u16, message: String },
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Missing token")]
    MissingToken,
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

pub type Result<T> = std::result::Result<T, GitHubError>;

/// A GitHub Pull Request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u32,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub head_sha: String,
    pub base_sha: String,
    pub html_url: String,
    pub user: Option<GitHubUser>,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubUser {
    pub login: String,
}

#[derive(Debug, Clone)]
pub struct GitHubClient {
    token: String,
    http: Client,
    base_url: String,
}

impl GitHubClient {
    pub fn new(token: impl Into<String>) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("argus/0.1.0")
            .build()
            .expect("reqwest client");
        Self {
            token: token.into(),
            http,
            base_url: "https://api.github.com".to_string(),
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Parse a PR URL like `https://github.com/owner/repo/pull/42` into parts.
    pub fn parse_pr_url(url: &str) -> Result<(String, String, u32)> {
        let parsed = Url::parse(url).map_err(|e| GitHubError::InvalidUrl(e.to_string()))?;
        let segments: Vec<&str> = parsed
            .path_segments()
            .ok_or_else(|| GitHubError::InvalidUrl("no path".into()))?
            .filter(|s| !s.is_empty())
            .collect();
        if segments.len() < 4 || segments[2] != "pull" {
            return Err(GitHubError::InvalidUrl(format!(
                "expected /owner/repo/pull/N, got {}",
                url
            )));
        }
        let owner = segments[0].to_string();
        let repo = segments[1].to_string();
        let number: u32 = segments[3]
            .parse()
            .map_err(|e: std::num::ParseIntError| GitHubError::InvalidUrl(e.to_string()))?;
        Ok((owner, repo, number))
    }

    /// Fetch a PR by URL.
    pub async fn get_pr(&self, owner: &str, repo: &str, number: u32) -> Result<PullRequest> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, number
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| GitHubError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api {
                status: status.as_u16(),
                message: text,
            });
        }
        resp.json()
            .await
            .map_err(|e| GitHubError::Parse(e.to_string()))
    }

    /// Fetch the diff of a PR (unified diff format).
    pub async fn get_diff(&self, owner: &str, repo: &str, number: u32) -> Result<String> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, number
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github.v3.diff")
            .send()
            .await
            .map_err(|e| GitHubError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api {
                status: status.as_u16(),
                message: text,
            });
        }
        resp.text()
            .await
            .map_err(|e| GitHubError::Parse(e.to_string()))
    }

    /// Post a comment on a PR.
    pub async fn post_comment(
        &self,
        owner: &str,
        repo: &str,
        number: u32,
        body: &str,
    ) -> Result<u64> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments",
            self.base_url, owner, repo, number
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .json(&serde_json::json!({ "body": body }))
            .send()
            .await
            .map_err(|e| GitHubError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api {
                status: status.as_u16(),
                message: text,
            });
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| GitHubError::Parse(e.to_string()))?;
        Ok(v["id"].as_u64().unwrap_or(0))
    }

    /// Set labels on a PR (replaces existing labels).
    pub async fn set_labels(
        &self,
        owner: &str,
        repo: &str,
        number: u32,
        labels: &[&str],
    ) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/labels",
            self.base_url, owner, repo, number
        );
        let resp = self
            .http
            .put(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .json(&serde_json::json!({ "labels": labels }))
            .send()
            .await
            .map_err(|e| GitHubError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api {
                status: status.as_u16(),
                message: text,
            });
        }
        Ok(())
    }

    /// Format an ARGUS verdict as a GitHub-flavored Markdown comment.
    #[allow(clippy::too_many_arguments)]
    pub fn format_verdict_comment(
        pr_ref: &str,
        verdict_status: &str,
        risk_score: f32,
        summary: &str,
        key_findings: &[String],
        action_items: &[String],
        slop_score: f32,
        fit_score: f32,
        security_summary: &str,
    ) -> String {
        let mut s = String::new();
        let emoji = match verdict_status {
            "APPROVED" => "✅",
            "REVIEW_REQUIRED" => "⚠️",
            "HALTED" => "🛑",
            _ => "❓",
        };
        s.push_str(&format!(
            "## {} ARGUS Review — `{}`\n\n",
            emoji, verdict_status
        ));
        s.push_str(&format!("**PR:** `{}`\n", pr_ref));
        s.push_str(&format!("**Risk score:** {:.2} / 1.00\n\n", risk_score));
        s.push_str(&format!("{}\n\n", summary));
        s.push_str("### Scores\n\n");
        s.push_str("| Metric | Score |\n|---|---|\n");
        s.push_str(&format!("| AI slop score | {:.2} |\n", slop_score));
        s.push_str(&format!("| Architecture fit | {:.2} |\n", fit_score));
        s.push_str(&format!("| Security | {} |\n\n", security_summary));
        if !key_findings.is_empty() {
            s.push_str("### Key findings\n\n");
            for f in key_findings {
                s.push_str(&format!("- {}\n", f));
            }
            s.push('\n');
        }
        if !action_items.is_empty() {
            s.push_str("### Action items\n\n");
            for a in action_items {
                s.push_str(&format!("- {}\n", a));
            }
            s.push('\n');
        }
        s.push_str("---\n");
        s.push_str("*This comment was produced by [ARGUS](https://argus.apohara.dev) — ");
        s.push_str("the AI slop defense layer. BYOK, signed, offline-verifiable.*\n");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pr_url() {
        let (o, r, n) =
            GitHubClient::parse_pr_url("https://github.com/SuarezPM/apohara-argus/pull/42")
                .unwrap();
        assert_eq!(o, "SuarezPM");
        assert_eq!(r, "apohara-argus");
        assert_eq!(n, 42);
    }

    #[test]
    fn rejects_invalid_pr_url() {
        assert!(GitHubClient::parse_pr_url("https://github.com/owner/repo/issues/1").is_err());
        assert!(GitHubClient::parse_pr_url("not-a-url").is_err());
    }

    #[test]
    fn verdict_comment_format() {
        let s = GitHubClient::format_verdict_comment(
            "owner/repo#42",
            "HALTED",
            0.92,
            "Critical security issue: hardcoded AWS key.",
            &["Hardcoded AWS key detected at line 2".into()],
            &["Move credentials to env vars".into()],
            0.4,
            0.2,
            "1 CRITICAL finding",
        );
        assert!(s.contains("🛑"));
        assert!(s.contains("HALTED"));
        assert!(s.contains("AWS"));
        assert!(s.contains("argus.apohara.dev"));
    }
}
