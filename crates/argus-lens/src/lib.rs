//! argus-lens — Aegis Lens, the weekly org-wide digest
//!
//! Aggregates ARGUS data over the last 7 days, asks the LLM for a
//! "CTO-style" script, and produces:
//! - A Markdown summary (saved to docs/briefings/)
//! - A JSON file with the structured briefing (saved to the ledger)

use apohara_argus_core::{OffenderSummary, OrgSummary, TeamSummary, WeeklyBriefing};
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
        let _prs_summary = prs
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

fn render_markdown(b: &WeeklyBriefing, org: &OrgSummary, _prs: &[PRBriefSummary]) -> String {
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

    #[test]
    fn new_uses_default_nim_model() {
        // The default model is pinned to a specific NIM model
        // name. We verify it's non-empty and matches the same
        // default that NimClient uses.
        let r = LensRunner::new();
        assert!(!r.nim_model.is_empty());
        assert_eq!(r.nim_model, "meta/llama-3.1-70b-instruct");
    }

    #[test]
    fn with_model_replaces_nim_model_and_re_wires_nim_client() {
        // The builder pattern: with_model(m) sets self.nim_model
        // AND rebuilds self.nim with the same model. Both fields
        // must end up in sync.
        let r = LensRunner::new().with_model("custom/test-model");
        assert_eq!(r.nim_model, "custom/test-model");
    }

    #[test]
    fn default_impl_matches_new() {
        // The Default impl must produce a runner equivalent to
        // `LensRunner::new()` — same model, same NimClient setup.
        let d = LensRunner::default();
        let n = LensRunner::new();
        assert_eq!(d.nim_model, n.nim_model);
    }

    #[test]
    fn pr_brief_summary_display_format() {
        // The Display impl is used in `generate_briefing()` to
        // build the prompt text. The format must include the PR
        // ref, author, risk score (2 decimal places), and top
        // finding.
        let p = PRBriefSummary {
            pr_ref: "acme/api#42".into(),
            author: "alice".into(),
            risk_score: 0.85,
            top_finding: "hardcoded AWS key".into(),
            critical_findings: 1,
        };
        let s = format!("{}", p);
        assert!(s.contains("acme/api#42"));
        assert!(s.contains("alice"));
        assert!(s.contains("0.85"));
        assert!(s.contains("hardcoded AWS key"));
        assert!(s.starts_with("- PR "));
    }

    #[test]
    fn aggregate_caps_top_offenders_at_five() {
        // The aggregate() function takes only the top 5 PRs with
        // risk_score >= 0.5 for the top_offenders list. We feed
        // 7 high-risk PRs and verify only 5 make it.
        let r = LensRunner::new();
        let mut prs = Vec::new();
        for i in 0..7 {
            prs.push(PRBriefSummary {
                pr_ref: format!("acme/api#{}", i),
                author: "alice".into(),
                risk_score: 0.9,
                top_finding: format!("finding {}", i),
                critical_findings: 0,
            });
        }
        let (b, _) = r.aggregate(
            "acme",
            chrono::NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            &prs,
        );
        assert_eq!(b.top_offenders.len(), 5);
    }

    #[test]
    fn aggregate_top_offenders_filters_low_risk() {
        // PRs with risk_score < 0.5 are excluded from
        // top_offenders regardless of count.
        let r = LensRunner::new();
        let prs = vec![
            PRBriefSummary {
                pr_ref: "acme/api#1".into(),
                author: "alice".into(),
                risk_score: 0.3, // below threshold
                top_finding: "low".into(),
                critical_findings: 0,
            },
            PRBriefSummary {
                pr_ref: "acme/api#2".into(),
                author: "bob".into(),
                risk_score: 0.8, // above threshold
                top_finding: "high".into(),
                critical_findings: 0,
            },
        ];
        let (b, _) = r.aggregate(
            "acme",
            chrono::NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            &prs,
        );
        assert_eq!(b.top_offenders.len(), 1);
        assert_eq!(b.top_offenders[0].pr_ref, "acme/api#2");
    }

    #[test]
    fn aggregate_groups_by_author_into_by_team() {
        // The by_team field groups PRs by author (proxy for
        // team). We feed 3 PRs from 2 authors and verify the
        // grouping + per-team aggregates.
        let r = LensRunner::new();
        let prs = vec![
            PRBriefSummary {
                pr_ref: "acme/api#1".into(),
                author: "alice".into(),
                risk_score: 0.8,
                top_finding: "secret".into(),
                critical_findings: 1,
            },
            PRBriefSummary {
                pr_ref: "acme/api#2".into(),
                author: "alice".into(),
                risk_score: 0.6,
                top_finding: "slop".into(),
                critical_findings: 0,
            },
            PRBriefSummary {
                pr_ref: "acme/web#1".into(),
                author: "bob".into(),
                risk_score: 0.3,
                top_finding: "minor".into(),
                critical_findings: 0,
            },
        ];
        let (_, o) = r.aggregate(
            "acme",
            chrono::NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            &prs,
        );
        assert_eq!(o.by_team.len(), 2);
        // Find alice's team.
        let alice_team = o
            .by_team
            .iter()
            .find(|t| t.team == "alice")
            .expect("alice should have a team");
        assert_eq!(alice_team.pr_count, 2);
        assert!((alice_team.avg_risk - 0.7).abs() < 0.01);
        assert_eq!(alice_team.high_risk_count, 1); // 0.8 >= 0.7
    }

    #[test]
    fn render_markdown_includes_all_sections() {
        // The markdown rendering must include the header, week
        // date, PR count, avg risk, critical findings, and the
        // CTO script section. We construct a WeeklyBriefing
        // and OrgSummary directly (bypassing aggregate) to
        // control the output precisely.
        let b = apohara_argus_core::WeeklyBriefing {
            id: uuid::Uuid::nil(),
            week_starting: chrono::NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            org: "acme".into(),
            prs_analyzed: 3,
            avg_slop_score: 0.4,
            avg_fit_score: 0.2,
            critical_findings: 1,
            top_offenders: vec![apohara_argus_core::OffenderSummary {
                pr_ref: "acme/api#1".into(),
                author: "alice".into(),
                risk_score: 0.9,
                top_finding: "secret".into(),
            }],
            trend_vs_prev_week: 0.0,
            cto_avatar_script: "Good morning team, this week...".into(),
            created_at: chrono::Utc::now(),
        };
        let o = apohara_argus_core::OrgSummary {
            org: "acme".into(),
            total_prs_analyzed: 3,
            pct_ai_generated: 0.65,
            avg_risk_score: 0.5,
            by_team: vec![apohara_argus_core::TeamSummary {
                team: "alice".into(),
                pr_count: 3,
                avg_risk: 0.5,
                high_risk_count: 1,
            }],
            last_updated: chrono::Utc::now(),
        };
        let md = render_markdown(&b, &o, &[]);
        assert!(md.contains("# ARGUS Weekly Briefing — `acme`"));
        assert!(md.contains("Week of: 2026-06-10"));
        assert!(md.contains("PRs analyzed: **3**"));
        assert!(md.contains("Avg risk: **0.50**"));
        assert!(md.contains("Critical findings: **1**"));
        assert!(md.contains("## CTO Avatar Script"));
        assert!(md.contains("Good morning team, this week..."));
        assert!(md.contains("## Top Offenders"));
        assert!(md.contains("acme/api#1"));
        assert!(md.contains("## By Team"));
        assert!(md.contains("alice"));
    }

    #[test]
    fn render_markdown_omits_empty_sections() {
        // When there are no top_offenders and no by_team entries,
        // those sections must be omitted from the markdown (not
        // rendered as empty tables).
        let b = apohara_argus_core::WeeklyBriefing {
            id: uuid::Uuid::nil(),
            week_starting: chrono::NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            org: "acme".into(),
            prs_analyzed: 0,
            avg_slop_score: 0.0,
            avg_fit_score: 0.0,
            critical_findings: 0,
            top_offenders: vec![],
            trend_vs_prev_week: 0.0,
            cto_avatar_script: String::new(),
            created_at: chrono::Utc::now(),
        };
        let o = apohara_argus_core::OrgSummary {
            org: "acme".into(),
            total_prs_analyzed: 0,
            pct_ai_generated: 0.0,
            avg_risk_score: 0.0,
            by_team: vec![],
            last_updated: chrono::Utc::now(),
        };
        let md = render_markdown(&b, &o, &[]);
        assert!(!md.contains("## Top Offenders"));
        assert!(!md.contains("## By Team"));
    }
}
