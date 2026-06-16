//! argus-guard — Aegis Guard, the pre-commit AI slop check.
//!
//! Used as a `pre-commit` hook. Reads a diff from disk (or stdin), runs the
//! 3 slop/security/arch analyzers in parallel, and emits a decision:
//!
//! - score < 0.3       → ALLOW (exit 0)
//! - 0.3 ≤ score < 0.7 → WARN  (exit 0, prints warnings)
//! - score ≥ 0.7       → BLOCK (exit 1, prints blocker)
//!
//! The LLM call is BYOK: the user provides their NVIDIA NIM key via
//! --nim-key or ARGUS_NIM_KEY env var.

use apohara_argus_core::VerdictStatus;
use argus_llm::NimClient;
use argus_slop::pipeline::AnalysisPipeline;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Allow,
    Warn,
    Block,
}

impl Decision {
    pub fn from_risk(risk: f32) -> Self {
        if risk >= 0.7 {
            Decision::Block
        } else if risk >= 0.3 {
            Decision::Warn
        } else {
            Decision::Allow
        }
    }
    pub fn exit_code(self) -> i32 {
        match self {
            Decision::Allow | Decision::Warn => 0,
            Decision::Block => 1,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "ALLOW",
            Decision::Warn => "WARN",
            Decision::Block => "BLOCK",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardOutput {
    pub decision: Decision,
    pub risk_score: f32,
    pub verdict_status: VerdictStatus,
    pub summary: String,
    pub key_findings: Vec<String>,
    pub action_items: Vec<String>,
    pub slop_score: f32,
    pub fit_score: f32,
    pub security_summary: String,
}

pub struct GuardRunner {
    pub nim_key: String,
    pub nim_model: Option<String>,
}

impl GuardRunner {
    pub fn new(nim_key: impl Into<String>) -> Self {
        Self {
            nim_key: nim_key.into(),
            nim_model: None,
        }
    }

    pub fn with_model(mut self, m: impl Into<String>) -> Self {
        self.nim_model = Some(m.into());
        self
    }

    /// Read a diff from a file path or stdin.
    pub fn read_diff(path: Option<&PathBuf>) -> anyhow::Result<String> {
        match path {
            Some(p) => Ok(std::fs::read_to_string(p)?),
            None => {
                let mut buf = String::new();
                use std::io::Read;
                std::io::stdin().read_to_string(&mut buf)?;
                Ok(buf)
            }
        }
    }

    /// Run the analysis pipeline on a diff and return a guard output.
    pub async fn run(&self, diff: &str) -> anyhow::Result<GuardOutput> {
        let mut client = NimClient::new();
        if let Some(m) = &self.nim_model {
            client = client.with_model(m.clone());
        }
        let pipeline = AnalysisPipeline::new();
        // The pipeline returns PipelineOutput directly; failures inside an
        // individual analyzer are captured as None in the output.
        let out = pipeline
            .run(&client, "local/pre-commit", diff, None, &self.nim_key)
            .await;

        let risk = out.verdict.risk_score.as_f32();
        let decision = Decision::from_risk(risk);
        let slop_score = out.slop.as_ref().map(|s| s.slop_score).unwrap_or(0.5);
        let fit_score = out
            .architecture
            .as_ref()
            .map(|a| a.fit_score)
            .unwrap_or(0.5);
        let sec_sum = out
            .security
            .as_ref()
            .map(|s| {
                format!(
                    "{} findings, highest {:?}",
                    s.findings.len(),
                    s.highest_severity
                )
            })
            .unwrap_or_else(|| "no security report".into());

        // If all 3 analyzers failed (the defensive default), we treat as BLOCK
        // regardless of the risk score from the synthesizer.
        let final_decision =
            if out.slop.is_none() && out.security.is_none() && out.architecture.is_none() {
                Decision::Block
            } else {
                decision
            };

        Ok(GuardOutput {
            decision: final_decision,
            risk_score: risk,
            verdict_status: out.verdict.status,
            summary: out.verdict.summary,
            key_findings: out.verdict.key_findings,
            action_items: out.verdict.action_items,
            slop_score,
            fit_score,
            security_summary: sec_sum,
        })
    }
}

impl GuardOutput {
    /// Render the output as a pretty terminal report.
    pub fn render_terminal(&self) -> String {
        let mut s = String::new();
        let icon = match self.decision {
            Decision::Allow => "✅",
            Decision::Warn => "⚠️ ",
            Decision::Block => "🛑",
        };
        s.push_str(&format!(
            "\n{} ARGUS Guard: {}\n",
            icon,
            self.decision.as_str()
        ));
        s.push_str(&format!("   Risk score: {:.2} / 1.00\n", self.risk_score));
        s.push_str(&format!("   Status: {:?}\n", self.verdict_status));
        s.push_str(&format!(
            "   Slop: {:.2}  |  Arch fit: {:.2}  |  Sec: {}\n",
            self.slop_score, self.fit_score, self.security_summary
        ));
        s.push_str(&format!("\n   {}\n", self.summary));
        if !self.key_findings.is_empty() {
            s.push_str("\n   Findings:\n");
            for f in &self.key_findings {
                s.push_str(&format!("   - {}\n", f));
            }
        }
        if !self.action_items.is_empty() {
            s.push_str("\n   Action items:\n");
            for a in &self.action_items {
                s.push_str(&format!("   - {}\n", a));
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_thresholds() {
        assert_eq!(Decision::from_risk(0.1), Decision::Allow);
        assert_eq!(Decision::from_risk(0.3), Decision::Warn);
        assert_eq!(Decision::from_risk(0.69), Decision::Warn);
        assert_eq!(Decision::from_risk(0.7), Decision::Block);
        assert_eq!(Decision::from_risk(0.95), Decision::Block);
    }

    #[test]
    fn exit_codes() {
        assert_eq!(Decision::Allow.exit_code(), 0);
        assert_eq!(Decision::Warn.exit_code(), 0);
        assert_eq!(Decision::Block.exit_code(), 1);
    }

    #[test]
    fn as_str_returns_correct_strings() {
        assert_eq!(Decision::Allow.as_str(), "ALLOW");
        assert_eq!(Decision::Warn.as_str(), "WARN");
        assert_eq!(Decision::Block.as_str(), "BLOCK");
    }

    #[test]
    fn new_stores_nim_key_and_default_model() {
        let r = GuardRunner::new("test-key");
        assert_eq!(r.nim_key, "test-key");
        assert!(r.nim_model.is_none());
    }

    #[test]
    fn with_model_sets_nim_model() {
        let r = GuardRunner::new("key").with_model("custom/model");
        assert_eq!(r.nim_model.as_deref(), Some("custom/model"));
    }

    #[test]
    fn read_diff_from_file_path() {
        // The file-path branch of read_diff() reads the diff from
        // disk. We create a temp file with a canned diff and
        // verify the contents are returned verbatim.
        let dir = std::env::temp_dir().join("argus-guard-read-diff-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let diff_path = dir.join("test.diff");
        let diff_content = "+ let x = 1;\n- let x = 2;\n";
        std::fs::write(&diff_path, diff_content).unwrap();
        let result = GuardRunner::read_diff(Some(&diff_path)).expect("read ok");
        assert_eq!(result, diff_content);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_diff_from_nonexistent_file_errors() {
        // A non-existent file path must propagate the io::Error
        // as an anyhow error. The function does not silently
        // fall back to stdin when the path is Some(_).
        let path = std::env::temp_dir().join("argus-guard-nonexistent-xyz.diff");
        let _ = std::fs::remove_file(&path);
        let result = GuardRunner::read_diff(Some(&path));
        assert!(result.is_err());
    }

    #[test]
    fn render_terminal_allow_decision() {
        // The terminal report must include the decision string,
        // risk score, status, and summary. The icon is
        // decision-specific (✅/⚠️/🛑).
        let out = GuardOutput {
            decision: Decision::Allow,
            risk_score: 0.1,
            verdict_status: VerdictStatus::Approved,
            summary: "all clean".into(),
            key_findings: vec![],
            action_items: vec![],
            slop_score: 0.1,
            fit_score: 0.9,
            security_summary: "no findings".into(),
        };
        let s = out.render_terminal();
        assert!(s.contains("ALLOW"));
        assert!(s.contains("0.10"));
        assert!(s.contains("all clean"));
        assert!(s.contains("✅"));
        // No findings/action items sections when empty.
        assert!(!s.contains("Findings:"));
        assert!(!s.contains("Action items:"));
    }

    #[test]
    fn render_terminal_warn_decision() {
        let out = GuardOutput {
            decision: Decision::Warn,
            risk_score: 0.5,
            verdict_status: VerdictStatus::ReviewRequired,
            summary: "needs review".into(),
            key_findings: vec!["verbose comments".into()],
            action_items: vec!["simplify naming".into()],
            slop_score: 0.5,
            fit_score: 0.5,
            security_summary: "2 findings".into(),
        };
        let s = out.render_terminal();
        assert!(s.contains("WARN"));
        assert!(s.contains("0.50"));
        assert!(s.contains("⚠️"));
        assert!(s.contains("verbose comments"));
        assert!(s.contains("simplify naming"));
        assert!(s.contains("Findings:"));
        assert!(s.contains("Action items:"));
    }

    #[test]
    fn render_terminal_block_decision() {
        let out = GuardOutput {
            decision: Decision::Block,
            risk_score: 0.85,
            verdict_status: VerdictStatus::Halted,
            summary: "critical: hardcoded secret".into(),
            key_findings: vec!["AWS key in diff".into()],
            action_items: vec!["rotate the key".into()],
            slop_score: 0.1,
            fit_score: 0.9,
            security_summary: "1 critical finding".into(),
        };
        let s = out.render_terminal();
        assert!(s.contains("BLOCK"));
        assert!(s.contains("0.85"));
        assert!(s.contains("🛑"));
        assert!(s.contains("AWS key in diff"));
    }

    #[test]
    fn guard_output_serde_roundtrip() {
        // The GuardOutput struct must round-trip through serde
        // JSON without losing any field. This pins the public
        // schema for any downstream consumer that reads the
        // guard output from a file or stdout pipe.
        let original = GuardOutput {
            decision: Decision::Warn,
            risk_score: 0.42,
            verdict_status: VerdictStatus::ReviewRequired,
            summary: "needs attention".into(),
            key_findings: vec!["finding 1".into(), "finding 2".into()],
            action_items: vec!["fix 1".into()],
            slop_score: 0.5,
            fit_score: 0.7,
            security_summary: "no critical issues".into(),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: GuardOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.decision, original.decision);
        assert!((restored.risk_score - original.risk_score).abs() < 1e-6);
        assert_eq!(restored.verdict_status, original.verdict_status);
        assert_eq!(restored.summary, original.summary);
        assert_eq!(restored.key_findings, original.key_findings);
        assert_eq!(restored.action_items, original.action_items);
        assert_eq!(restored.security_summary, original.security_summary);
    }
}
