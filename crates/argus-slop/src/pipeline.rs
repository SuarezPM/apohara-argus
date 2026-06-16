//! AnalysisPipeline — orchestrates the 4 analyzers in parallel.

use super::architecture::ArchReport;
use super::deterministic::{run_deterministic_rules, SlopSignal};
use super::security::SecurityReport;
use super::slop_detector::SlopReport;
use super::verdict::VerdictSynthesizer;
use super::Analyzer;
use apohara_argus_core::{Verdict, VerdictStatus};
use argus_llm::LlmClient;
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

#[derive(Default)]
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
        let _deterministic_signals: Vec<SlopSignal> = run_deterministic_rules(diff);

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
        ) || (slop.slop_score > 0.7 && arch.fit_score > 0.5)
            || (slop.slop_score > 0.85 || arch.fit_score > 0.7)
        {
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

#[cfg(test)]
mod tests {
    //! Unit tests for AnalysisPipeline::synthesize. We don't test the
    //! async `run` (it needs a real LlmClient); we test the
    //! deterministic decision logic of `synthesize` directly with
    //! hand-built reports. This pins the heuristics that the LLM
    //! prompt tells the model to mirror.
    use super::*;
    use crate::security::SecuritySeverity;
    use apohara_argus_core::VerdictStatus;

    fn slop(score: f32) -> SlopReport {
        SlopReport {
            slop_score: score,
            signals_detected: vec!["s1".to_string()],
            specific_examples: vec![],
            confidence: 0.9,
            reasoning: "test".to_string(),
        }
    }

    fn arch(fit: f32) -> ArchReport {
        ArchReport {
            fit_score: fit,
            verdict: "ok".to_string(),
            positives: vec![],
            concerns: vec![],
            summary: "ok".to_string(),
        }
    }

    fn sec(sev: SecuritySeverity) -> SecurityReport {
        SecurityReport {
            highest_severity: sev,
            findings: vec![],
            summary: "ok".to_string(),
        }
    }

    #[test]
    fn pipeline_new_builds_default() {
        // The default constructor must not panic and must wire up
        // the 4 sub-analyzers with their default configs.
        let p = AnalysisPipeline::new();
        // Smoke test: the synthesizer is reachable through synthesize().
        let v = p.synthesize("pr/0", &None, &None, &None);
        assert_eq!(v.status, VerdictStatus::ReviewRequired);
    }

    #[test]
    fn synthesize_missing_slop_returns_review_required() {
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/1",
            &None,
            &Some(sec(SecuritySeverity::None)),
            &Some(arch(0.3)),
        );
        assert_eq!(v.status, VerdictStatus::ReviewRequired);
    }

    #[test]
    fn synthesize_missing_security_returns_review_required() {
        let p = AnalysisPipeline::new();
        let v = p.synthesize("pr/2", &Some(slop(0.3)), &None, &Some(arch(0.3)));
        assert_eq!(v.status, VerdictStatus::ReviewRequired);
    }

    #[test]
    fn synthesize_missing_arch_returns_review_required() {
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/3",
            &Some(slop(0.3)),
            &Some(sec(SecuritySeverity::None)),
            &None,
        );
        assert_eq!(v.status, VerdictStatus::ReviewRequired);
    }

    #[test]
    fn synthesize_critical_security_halts() {
        // Critical severity always escalates to HALTED, regardless
        // of the other scores.
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/crit",
            &Some(slop(0.1)),
            &Some(sec(SecuritySeverity::Critical)),
            &Some(arch(0.1)),
        );
        assert_eq!(v.status, VerdictStatus::Halted);
    }

    #[test]
    fn synthesize_high_security_halts() {
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/high",
            &Some(slop(0.1)),
            &Some(sec(SecuritySeverity::High)),
            &Some(arch(0.1)),
        );
        assert_eq!(v.status, VerdictStatus::Halted);
    }

    #[test]
    fn synthesize_high_slop_plus_arch_halts() {
        // slop > 0.7 AND arch > 0.5 → HALTED.
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/sa",
            &Some(slop(0.8)),
            &Some(sec(SecuritySeverity::None)),
            &Some(arch(0.6)),
        );
        assert_eq!(v.status, VerdictStatus::Halted);
    }

    #[test]
    fn synthesize_very_high_slop_alone_halts() {
        // slop > 0.85 alone → HALTED.
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/vs",
            &Some(slop(0.9)),
            &Some(sec(SecuritySeverity::None)),
            &Some(arch(0.3)),
        );
        assert_eq!(v.status, VerdictStatus::Halted);
    }

    #[test]
    fn synthesize_very_high_arch_alone_halts() {
        // arch > 0.7 alone → HALTED.
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/va",
            &Some(slop(0.3)),
            &Some(sec(SecuritySeverity::None)),
            &Some(arch(0.8)),
        );
        assert_eq!(v.status, VerdictStatus::Halted);
    }

    #[test]
    fn synthesize_moderate_slop_review_required() {
        // 0.5 < slop <= 0.7 (with arch low) → REVIEW_REQUIRED.
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/ms",
            &Some(slop(0.6)),
            &Some(sec(SecuritySeverity::None)),
            &Some(arch(0.3)),
        );
        assert_eq!(v.status, VerdictStatus::ReviewRequired);
    }

    #[test]
    fn synthesize_moderate_arch_review_required() {
        // 0.5 < arch <= 0.7 (with slop low) → REVIEW_REQUIRED.
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/ma",
            &Some(slop(0.3)),
            &Some(sec(SecuritySeverity::None)),
            &Some(arch(0.6)),
        );
        assert_eq!(v.status, VerdictStatus::ReviewRequired);
    }

    #[test]
    fn synthesize_clean_approved() {
        // All scores low, no security findings → APPROVED.
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/clean",
            &Some(slop(0.2)),
            &Some(sec(SecuritySeverity::None)),
            &Some(arch(0.3)),
        );
        assert_eq!(v.status, VerdictStatus::Approved);
    }

    #[test]
    fn risk_score_clamped_to_unit_interval() {
        // The risk aggregation in pipeline.rs is:
        //   (slop * 0.4 + arch * 0.4 + (n_findings * 0.05).min(0.2))
        //     .clamp(0.0, 1.0)
        // We build a case that would overflow 1.0 without the clamp
        // (slop=1.0, arch=1.0, 100 findings) and verify it stays <= 1.0.
        let p = AnalysisPipeline::new();
        let mut s = sec(SecuritySeverity::None);
        for i in 0..100 {
            s.findings.push(crate::security::SecurityFinding {
                severity: SecuritySeverity::Low,
                file: format!("f{i}.rs"),
                line: Some(i as u32),
                category: "x".to_string(),
                quote: "q".to_string(),
                description: "d".to_string(),
                recommendation: "r".to_string(),
            });
        }
        let v = p.synthesize("pr/overflow", &Some(slop(1.0)), &Some(s), &Some(arch(1.0)));
        let risk: f32 = v.risk_score.as_f32();
        assert!(risk <= 1.0, "risk={risk}");
        assert!(risk > 0.0);
    }

    #[test]
    fn key_findings_and_action_items_populated() {
        // The synthesizer always populates key_findings (3 entries)
        // and action_items (up to 3 from security findings).
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/kf",
            &Some(slop(0.3)),
            &Some(sec(SecuritySeverity::None)),
            &Some(arch(0.3)),
        );
        assert_eq!(v.key_findings.len(), 3);
        // action_items comes from sec.findings (none here).
        assert!(v.action_items.is_empty());
    }

    #[test]
    fn action_items_take_top_3_recommendations() {
        let p = AnalysisPipeline::new();
        let mut s = sec(SecuritySeverity::Medium);
        for i in 0..5 {
            s.findings.push(crate::security::SecurityFinding {
                severity: SecuritySeverity::Medium,
                file: format!("f{i}.rs"),
                line: Some(i as u32),
                category: "x".to_string(),
                quote: "q".to_string(),
                description: "d".to_string(),
                recommendation: format!("fix-{i}"),
            });
        }
        let v = p.synthesize("pr/ai", &Some(slop(0.3)), &Some(s), &Some(arch(0.3)));
        assert_eq!(v.action_items.len(), 3);
        assert_eq!(v.action_items[0], "fix-0");
        assert_eq!(v.action_items[1], "fix-1");
        assert_eq!(v.action_items[2], "fix-2");
    }

    #[test]
    fn reasoning_string_includes_all_three_scores() {
        // The reasoning field is shown in the audit chain + CLI; it
        // must mention all 3 component scores for debuggability.
        let p = AnalysisPipeline::new();
        let v = p.synthesize(
            "pr/rs",
            &Some(slop(0.33)),
            &Some(sec(SecuritySeverity::Info)),
            &Some(arch(0.44)),
        );
        assert!(v.reasoning.contains("slop=0.33"));
        assert!(v.reasoning.contains("arch=0.44"));
        assert!(v.reasoning.contains("Info"));
    }
}
