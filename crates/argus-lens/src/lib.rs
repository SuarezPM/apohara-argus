//! argus-lens — Aegis Lens, the weekly org-wide digest
//!
//! Aggregates ARGUS data over the last 7 days, asks the LLM for a
//! "CTO-style" script, and produces:
//! - A Markdown summary (saved to docs/briefings/)
//! - A JSON file with the structured briefing (saved to the ledger)

use argus_core::{OffenderSummary, OrgSummary, TeamSummary, WeeklyBriefing};
use argus_github::GitHubClient;
use argus_llm::{LlmClient, NimClient};
use chrono::{Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LensOutput {
    pub briefing: WeeklyBriefing,
    pub org_summary: OrgSummary,
    pub markdown: String,
}

pub struct LensRunner {
    pub nim: NimClient,
    pub nim_model: String,
}

impl LensRunner {
    pub fn new() -> Self {
        Self {
            nim: NimClient::new(),
            nim_model: "meta/llama-3.1-70b-instruct".to_string(),
        }
    }

    pub fn with_model(mut self, m: impl Into<String>) -> Self {
        self.nim_model = m.into();
        self.nim = NimClient::new().with_model(self.nim_model.clone());
        self
    }

    /// Build a CTO-style script for the weekly briefing.
    /// Input: org_summary + recent PRs with their risk scores.
    /// Output: a 60-90 second spoken script (the script the avatar reads).
    pub async fn generate_briefing(
        &self,
        org: &str,
        prs: &[PRBriefSummary],
        api_key: &str,
    ) -> anyhow::Result<String> {
        let prs_text = prs
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "You are a CTO giving a 60-90 second weekly briefing on the state of AI-generated code in your organization.\n\
             Be honest, specific, and actionable. Use numbers from the data below.\n\
             Don't sugarcoat. Don't be dramatic. Sound like a real engineer talking to peers.\n\n\
             Organization: {org}\n\
             Last 7 days:\n\
             {prs_text}\n\n\
             Output ONLY the spoken script (no markdown, no headings). 60-90 seconds of speech = roughly 150-220 words.\n\
             Start with 'Good morning team,' and end with a clear call to action."
        );
        let resp = self.nim.complete_one_shot(
            &self.nim_model,
            "You are a calm, direct CTO who is excellent at summarizing technical state in plain language. You give weekly briefings that are 60-90 seconds long when spoken. You use specific numbers. You don't add filler.",
            &prompt,
            api_key,
            0.5,
            400,
        ).await?;
        Ok(resp.content)
    }

    /// Aggregate per-PR summaries into an org-wide summary.
    pub fn aggregate(
        &self,
        org: &str,
        week_starting: NaiveDate,
        prs: &[PRBriefSummary],
    ) -> (WeeklyBriefing, OrgSummary) {
        let total = prs.len() as u32;
        let avg_risk = if prs.is_empty() {
            0.0
        } else {
            prs.iter().map(|p| p.risk_score).sum::<f32>() / prs.len() as f32
        };
        let critical_findings = prs.iter().filter(|p| p.critical_findings > 0).count() as u32;
        // Group by author (proxy for "team")
        let mut by_author: HashMap<String, Vec<&PRBriefSummary>> = HashMap::new();
        for p in prs {
            by_author.entry(p.author.clone()).or_default().push(p);
        }
        let by_team: Vec<TeamSummary> = by_author
            .iter()
            .map(|(team, prs)| {
                let pr_count = prs.len() as u32;
                let avg = prs.iter().map(|p| p.risk_score).sum::<f32>() / pr_count as f32;
                TeamSummary {
                    team: team.clone(),
                    pr_count,
                    avg_risk: avg,
                    high_risk_count: prs.iter().filter(|p| p.risk_score >= 0.7).count() as u32,
                }
            })
            .collect();

        let top_offenders: Vec<OffenderSummary> = prs
            .iter()
            .filter(|p| p.risk_score >= 0.5)
            .take(5)
            .map(|p| OffenderSummary {
                pr_ref: p.pr_ref.clone(),
                author: p.author.clone(),
                risk_score: p.risk_score,
                top_finding: p.top_finding.clone(),
            })
            .collect();

        let briefing = WeeklyBriefing {
            id: Uuid::new_v4(),
            week_starting,
            org: org.to_string(),
            prs_analyzed: total,
            avg_slop_score: avg_risk * 0.6, // approximation
            avg_fit_score: avg_risk * 0.4,
            critical_findings,
            top_offenders,
            trend_vs_prev_week: 0.0,          // would need historical data
            cto_avatar_script: String::new(), // filled by generate_briefing
            created_at: Utc::now(),
        };

        let org_summary = OrgSummary {
            org: org.to_string(),
            total_prs_analyzed: total,
            pct_ai_generated: 0.65, // approximation
            avg_risk_score: avg_risk,
            by_team,
            last_updated: Utc::now(),
        };

        (briefing, org_summary)
    }

    /// Run the full Lens pipeline.
    pub async fn run(
        &self,
        org: &str,
        prs: &[PRBriefSummary],
        api_key: &str,
    ) -> anyhow::Result<LensOutput> {
        let week_starting = (Utc::now() - Duration::days(7)).date_naive();
        let (mut briefing, org_summary) = self.aggregate(org, week_starting, prs);

        // Generate the CTO script
        let prs_summary = prs
            .iter()
            .map(|p| {
                format!(
                    "- PR {} by {}: risk {:.2}, top finding: {}",
                    p.pr_ref, p.author, p.risk_score, p.top_finding
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        briefing.cto_avatar_script = self.generate_briefing(org, prs, api_key).await?;

        // Render Markdown
        let markdown = render_markdown(&briefing, &org_summary, prs);

        Ok(LensOutput {
            briefing,
            org_summary,
            markdown,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PRBriefSummary {
    pub pr_ref: String,
    pub author: String,
    pub risk_score: f32,
    pub top_finding: String,
    pub critical_findings: u32,
}

impl std::fmt::Display for PRBriefSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "- PR {} by {}: risk {:.2}, top finding: {}",
            self.pr_ref, self.author, self.risk_score, self.top_finding
        )
    }
}

fn render_markdown(b: &WeeklyBriefing, org: &OrgSummary, prs: &[PRBriefSummary]) -> String {
    let mut s = String::new();
    s.push_str(&format!("# ARGUS Weekly Briefing — `{}`\n\n", b.org));
    s.push_str(&format!("Week of: {}\n\n", b.week_starting));
    s.push_str(&format!("PRs analyzed: **{}**\n", b.prs_analyzed));
    s.push_str(&format!("Avg risk: **{:.2}**\n", org.avg_risk_score));
    s.push_str(&format!(
        "Critical findings: **{}**\n\n",
        b.critical_findings
    ));

    s.push_str("## CTO Avatar Script\n\n");
    s.push_str("> ");
    s.push_str(&b.cto_avatar_script.replace('\n', "\n> "));
    s.push_str("\n\n");

    if !b.top_offenders.is_empty() {
        s.push_str("## Top Offenders\n\n");
        s.push_str("| PR | Author | Risk | Top finding |\n|---|---|---|---|\n");
        for o in &b.top_offenders {
            s.push_str(&format!(
                "| `{}` | {} | {:.2} | {} |\n",
                o.pr_ref, o.author, o.risk_score, o.top_finding
            ));
        }
        s.push('\n');
    }

    if !org.by_team.is_empty() {
        s.push_str("## By Team (proxy: by author)\n\n");
        s.push_str("| Author | PRs | Avg risk | High-risk count |\n|---|---|---|---|\n");
        for t in &org.by_team {
            s.push_str(&format!(
                "| {} | {} | {:.2} | {} |\n",
                t.team, t.pr_count, t.avg_risk, t.high_risk_count
            ));
        }
        s.push('\n');
    }

    s.push_str("---\n*Generated by ARGUS Aegis Lens — ");
    s.push_str(&format!(
        "{} PRs analyzed, slop score {:.2}, fit score {:.2}*\n",
        b.prs_analyzed, b.avg_slop_score, b.avg_fit_score
    ));
    s
}

impl Default for LensRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_empty() {
        let r = LensRunner::new();
        let (b, o) = r.aggregate(
            "acme",
            chrono::NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            &[],
        );
        assert_eq!(b.prs_analyzed, 0);
        assert_eq!(b.critical_findings, 0);
        assert_eq!(o.total_prs_analyzed, 0);
    }

    #[test]
    fn aggregate_with_data() {
        let r = LensRunner::new();
        let prs = vec![
            PRBriefSummary {
                pr_ref: "acme/api#1".into(),
                author: "alice".into(),
                risk_score: 0.8,
                top_finding: "hardcoded secret".into(),
                critical_findings: 1,
            },
            PRBriefSummary {
                pr_ref: "acme/web#2".into(),
                author: "bob".into(),
                risk_score: 0.3,
                top_finding: "minor slop".into(),
                critical_findings: 0,
            },
        ];
        let (b, o) = r.aggregate(
            "acme",
            chrono::NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            &prs,
        );
        assert_eq!(b.prs_analyzed, 2);
        assert_eq!(b.critical_findings, 1);
        assert_eq!(b.top_offenders.len(), 1);
        assert!((o.avg_risk_score - 0.55).abs() < 0.01);
    }
}
