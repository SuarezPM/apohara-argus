//! AnalysisPipeline — orchestrates the 4 analyzers in parallel.

use super::architecture::ArchReport;
use super::deterministic::{run_deterministic_rules, SlopSignal};
use super::security::SecurityReport;
use super::slop_detector::SlopReport;
use super::verdict::VerdictSynthesizer;
use super::Analyzer;
use argus_core::{RiskScore, Verdict, VerdictStatus};
use argus_llm::LlmClient;
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineOutput {
    pub slop: Option<SlopReport>,
    pub security: Option<SecurityReport>,
    pub architecture: Option<ArchReport>,
    pub verdict: Verdict,
    pub total_tokens: u32,
    pub total_latency_ms: u64,
}

pub struct AnalysisPipeline {
    slop: super::slop_detector::SlopDetector,
    security: super::security::SecurityReview,
    arch: super::architecture::ArchitectureFit,
    verdict: VerdictSynthesizer,
}

impl AnalysisPipeline {
    pub fn new() -> Self {
        Self {
            slop: super::slop_detector::SlopDetector::new(),
            security: super::security::SecurityReview::new(),
            arch: super::architecture::ArchitectureFit::new(),
            verdict: VerdictSynthesizer::new(),
        }
    }

    /// Run all 4 analyzers in parallel. If any fails, fall back to a
    /// REVIEW_REQUIRED verdict (safer than APPROVED).
    pub async fn run(
        &self,
        client: &dyn LlmClient,
        pr_ref: &str,
        diff: &str,
        context: Option<&str>,
        api_key: &str,
    ) -> PipelineOutput {
        let start = std::time::Instant::now();

        // Phase 1: Deterministic pre-flight (free, < 100ms, no API calls).
        // Catches mechanical slop (oversized fns, swallowed errors, unwraps)
        // before we burn LLM tokens on semantic analysis.
        let deterministic_signals: Vec<SlopSignal> = run_deterministic_rules(diff);

        let slop_fut = self.slop.run(client, diff, context, api_key);
        let sec_fut = self.security.run(client, diff, context, api_key);
        let arch_fut = self.arch.run(client, diff, context, api_key);

        let (slop_res, sec_res, arch_res) = tokio::join!(slop_fut, sec_fut, arch_fut);

        let slop_report = slop_res.ok();
        let sec_report = sec_res.ok();
        let arch_report = arch_res.ok();

        // Heuristic verdict: if any failed, default to REVIEW_REQUIRED.
        let verdict = self.synthesize(pr_ref, &slop_report, &sec_report, &arch_report);
        let total_tokens = 0; // could be summed from LlmClient usage
        let total_latency_ms = start.elapsed().as_millis() as u64;

        PipelineOutput {
            slop: slop_report,
            security: sec_report,
            architecture: arch_report,
            verdict,
            total_tokens,
            total_latency_ms,
        }
    }

    fn synthesize(
        &self,
        _pr_ref: &str,
        slop: &Option<SlopReport>,
        sec: &Option<SecurityReport>,
        arch: &Option<ArchReport>,
    ) -> Verdict {
        // If any failed, default to REVIEW_REQUIRED.
        if slop.is_none() || sec.is_none() || arch.is_none() {
            return self.verdict.to_verdict(
                VerdictStatus::ReviewRequired,
                0.5,
                "One or more analyzers failed; defaulting to REVIEW_REQUIRED.".into(),
                vec!["Incomplete analysis — review manually.".into()],
                vec!["Re-run after checking analyzer failures.".into()],
                "Defensive default: missing analyzer output.".into(),
            );
        }
        let slop = slop.as_ref().unwrap();
        let sec = sec.as_ref().unwrap();
        let arch = arch.as_ref().unwrap();

        // Decision logic from the prompt
        let status = if matches!(
            sec.highest_severity,
            super::security::SecuritySeverity::Critical | super::security::SecuritySeverity::High
        ) {
            VerdictStatus::Halted
        } else if slop.slop_score > 0.7 && arch.fit_score > 0.5 {
            VerdictStatus::Halted
        } else if slop.slop_score > 0.85 || arch.fit_score > 0.7 {
            VerdictStatus::Halted
        } else if slop.slop_score > 0.5 || arch.fit_score > 0.5 {
            VerdictStatus::ReviewRequired
        } else {
            VerdictStatus::Approved
        };

        // Aggregate risk score (weighted average)
        let risk = (slop.slop_score * 0.4
            + arch.fit_score * 0.4
            + (sec.findings.len() as f32 * 0.05).min(0.2))
        .clamp(0.0, 1.0);

        let summary = format!(
            "Slop score: {:.2}, Arch fit: {:.2}, Security: {}. Decision: {:?}.",
            slop.slop_score, arch.fit_score, sec.summary, status
        );
        let key_findings = vec![
            format!(
                "AI slop signals: {} (score {:.2})",
                slop.signals_detected.len(),
                slop.slop_score
            ),
            format!("Architecture fit: {:.2} — {}", arch.fit_score, arch.verdict),
            format!(
                "Security: {} findings, highest {:?}",
                sec.findings.len(),
                sec.highest_severity
            ),
        ];
        let action_items = sec
            .findings
            .iter()
            .take(3)
            .map(|f| f.recommendation.clone())
            .collect();

        self.verdict.to_verdict(
            status,
            risk,
            summary,
            key_findings,
            action_items,
            format!(
                "Heuristic synthesis: slop={:.2} arch={:.2} sec={:?}",
                slop.slop_score, arch.fit_score, sec.highest_severity
            ),
        )
    }
}
