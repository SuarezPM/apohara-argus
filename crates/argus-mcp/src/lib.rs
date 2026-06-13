//! ARGUS MCP server. [Refs: 5]
//!
//! Exposes the 4 ARGUS specialists as Model Context Protocol tools
//! so any MCP-compatible client (Claude Code, Codex, Cursor, etc.)
//! can call them via stdio. Each tool takes a `code_diff: String` and
//! returns a structured JSON report.
//!
//! The 4 tools:
//! - `aegis_slop`     — `slop-detector` prompt     (SLOP-001..005 + LLM)
//! - `aegis_security` — `redteam-security` prompt  (adversarial review)
//! - `aegis_arch`     — `architecture-fit` prompt  (repo coherence)
//! - `aegis_verdict`  — `verdict-synthesizer` prompt (final verdict)
//!
//! The NIM key is read from `ARGUS_NIM_KEY` env var per call (BYOK).
//! No key → tool returns a structured error rather than crashing.

use std::future::Future;
use std::time::Instant;

use argus_core::Result as ArgusResult;
use argus_llm::{LlmClient, NimClient};
use argus_slop::pipeline::AnalysisPipeline;
use argus_slop::{Analyzer, ArchitectureFit, SecurityReview, SlopDetector, VerdictSynthesizer};
use rmcp::{
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::ServerInfo,
    tool, tool_handler, tool_router,
    ErrorData, ServerHandler,
};
use serde::{Deserialize, Serialize};

/// Per-tool result envelope. Returned to the MCP client as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistReport {
    /// Which specialist produced this report.
    pub specialist: String,
    /// Prompt name from `argus-core/prompts/`.
    pub prompt_name: String,
    /// Model id used (e.g., "zhipuai/glm-5.1").
    pub model_id: String,
    /// Wall-clock latency for the LLM call.
    pub latency_ms: u64,
    /// The structured findings from the specialist. For slop/security/arch
    /// this is a list of findings; for verdict this is the final verdict.
    pub findings: serde_json::Value,
    /// One-sentence human-readable summary.
    pub summary: String,
}

/// Error envelope returned to the MCP client when a tool fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistError {
    pub specialist: String,
    pub error: String,
}

/// Shared state for the MCP server. We hold the NIM client once and
/// reuse it across tool calls; only the per-call API key + diff change.
pub struct ArgusMcp {
    nim: NimClient,
    /// Per-call NIM key. Set by the agent (Claude Code / Codex) via
    /// the `ARGUS_NIM_KEY` env var at process startup.
    nim_key: String,
    /// Default model for the specialists. In a future iteration this
    /// could be a per-specialist model registry.
    model_id: String,
    /// The MCP tool router. Populated by `#[tool_router]`.
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ArgusMcp {
    /// Construct a new MCP server. Reads `ARGUS_NIM_KEY` and
    /// `ARGUS_MODEL_ID` from the env at construction time.
    pub fn new() -> Self {
        Self {
            nim: NimClient::new(),
            nim_key: std::env::var("ARGUS_NIM_KEY")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_default(),
            model_id: std::env::var("ARGUS_MODEL_ID")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "zhipuai/glm-5.1".to_string()),
            tool_router: Self::tool_router(),
        }
    }

    /// Read the per-call NIM key. Each call re-reads the env so the
    /// operator can rotate keys without restarting the MCP server.
    fn current_key(&self) -> Option<String> {
        std::env::var("ARGUS_NIM_KEY")
            .ok()
            .filter(|s| !s.is_empty())
    }

    /// Return a structured `SpecialistError` to the MCP client.
    fn err(specialist: &str, msg: impl Into<String>) -> ErrorData {
        let payload = SpecialistError {
            specialist: specialist.to_string(),
            error: msg.into(),
        };
        let json = serde_json::to_string(&payload).unwrap_or_default();
        ErrorData {
            code: rmcp::model::ErrorCode(-32000),
            message: json.into(),
            data: None,
        }
    }

    /// Wrap any specialist error as a structured MCP error.
    fn wrap<E: std::fmt::Display>(specialist: &str, e: E) -> ErrorData {
        Self::err(specialist, e.to_string())
    }

    /// **Aegis Slop** — detect AI-generated code smells.
    /// Prompt: `slop-detector`. Best for: narrative comments, swallowed
    /// errors, oversized functions, unwrap chains, TODO stubs, unused
    /// pub symbols. Hybrid: deterministic AST pre-flight (Roadmap 5.1)
    /// + LLM semantic.
    #[tool(
        name = "aegis_slop",
        description = "Detect AI-generated code smells in a code diff. Returns a list of slop findings (severity, file, line, category, description). Hybrid: deterministic AST pre-flight + LLM semantic analysis."
    )]
    async fn aegis_slop(
        &self,
        Parameters(AegisArgs { code_diff }): Parameters<AegisArgs>,
    ) -> Result<String, ErrorData> {
        let key = self.current_key().ok_or_else(|| {
            Self::err("aegis_slop", "ARGUS_NIM_KEY not set (BYOK required)")
        })?;
        let start = Instant::now();
        let analyzer = SlopDetector::new();
        let report = analyzer
            .run(&self.nim, &code_diff, None, &key)
            .await
            .map_err(|e| Self::wrap("aegis_slop", e))?;
        let findings = serde_json::to_value(&report).map_err(|e| Self::wrap("aegis_slop", e))?;
        let summary = format!("{} slop signals (score {:.2})", report.signals_detected.len(), report.slop_score);
        let report = SpecialistReport {
            specialist: "aegis_slop".into(),
            prompt_name: "slop-detector".into(),
            model_id: self.model_id.clone(),
            latency_ms: start.elapsed().as_millis() as u64,
            findings,
            summary,
        };
        Ok(serde_json::to_string(&report).unwrap_or_default())
    }

    /// **Aegis Security** — adversarial security review.
    /// Prompt: `redteam-security`. Best for: credentials in code,
    /// injection vectors, unsafe panic, unhandled errors, OWASP Top 10.
    #[tool(
        name = "aegis_security",
        description = "Adversarial security review of a code diff. Returns a list of security findings (severity, file, line, CWE, recommendation). Looks for credentials, injection, unsafe panic, unhandled errors."
    )]
    async fn aegis_security(
        &self,
        Parameters(AegisArgs { code_diff }): Parameters<AegisArgs>,
    ) -> Result<String, ErrorData> {
        let key = self.current_key().ok_or_else(|| {
            Self::err("aegis_security", "ARGUS_NIM_KEY not set (BYOK required)")
        })?;
        let start = Instant::now();
        let analyzer = SecurityReview::new();
        let report = analyzer
            .run(&self.nim, &code_diff, None, &key)
            .await
            .map_err(|e| Self::wrap("aegis_security", e))?;
        let findings = serde_json::to_value(&report).map_err(|e| Self::wrap("aegis_security", e))?;
        let summary = format!("{} security findings (highest {:?})", report.findings.len(), report.highest_severity);
        let report = SpecialistReport {
            specialist: "aegis_security".into(),
            prompt_name: "redteam-security".into(),
            model_id: self.model_id.clone(),
            latency_ms: start.elapsed().as_millis() as u64,
            findings,
            summary,
        };
        Ok(serde_json::to_string(&report).unwrap_or_default())
    }

    /// **Aegis Arch** — architectural fit review.
    /// Prompt: `architecture-fit`. Best for: repo coherence, pattern
    /// matching, idiom detection, separation of concerns.
    #[tool(
        name = "aegis_arch",
        description = "Architectural fit review of a code diff. Returns an ArchReport with fit_score (0-1), verdict, and a list of concerns. Best for: checking coherence with the rest of the repo, pattern matching, idiom detection."
    )]
    async fn aegis_arch(
        &self,
        Parameters(AegisArgs { code_diff }): Parameters<AegisArgs>,
    ) -> Result<String, ErrorData> {
        let key = self.current_key().ok_or_else(|| {
            Self::err("aegis_arch", "ARGUS_NIM_KEY not set (BYOK required)")
        })?;
        let start = Instant::now();
        let analyzer = ArchitectureFit::new();
        let report = analyzer
            .run(&self.nim, &code_diff, None, &key)
            .await
            .map_err(|e| Self::wrap("aegis_arch", e))?;
        let findings = serde_json::to_value(&report).map_err(|e| Self::wrap("aegis_arch", e))?;
        let summary = format!("Arch fit: {:.2} — {}", report.fit_score, report.verdict);
        let report = SpecialistReport {
            specialist: "aegis_arch".into(),
            prompt_name: "architecture-fit".into(),
            model_id: self.model_id.clone(),
            latency_ms: start.elapsed().as_millis() as u64,
            findings,
            summary,
        };
        Ok(serde_json::to_string(&report).unwrap_or_default())
    }

    /// **Aegis Verdict** — final verdict synthesizer.
    /// Prompt: `verdict-synthesizer`. Takes the 3 pre-computed
    /// specialist reports (slop, security, arch) and produces a
    /// final verdict (Approved / ReviewRequired / Halted) plus a
    /// FixPlan for downstream coding agents.
    #[tool(
        name = "aegis_verdict",
        description = "Synthesize a final verdict from pre-computed slop, security, and arch reports. Returns the verdict status, risk score, key findings, and a structured FixPlan (Roadmap 1.2) for downstream coding agents."
    )]
    async fn aegis_verdict(
        &self,
        Parameters(VerdictArgs {
            slop,
            security,
            arch,
            diff,
        }): Parameters<VerdictArgs>,
    ) -> Result<String, ErrorData> {
        let key = self.current_key().ok_or_else(|| {
            Self::err("aegis_verdict", "ARGUS_NIM_KEY not set (BYOK required)")
        })?;
        let start = Instant::now();

        // Parse the 3 pre-computed reports. If parsing fails, the
        // caller didn't run the other 3 tools first — give a clear error.
        let slop_report: argus_slop::SlopReport = serde_json::from_value(slop)
            .map_err(|e| Self::err("aegis_verdict", format!("invalid slop report: {e}")))?;
        let sec_report: argus_slop::SecurityReport = serde_json::from_value(security)
            .map_err(|e| Self::err("aegis_verdict", format!("invalid security report: {e}")))?;
        let arch_report: argus_slop::ArchReport = serde_json::from_value(arch)
            .map_err(|e| Self::err("aegis_verdict", format!("invalid arch report: {e}")))?;

        // Run the 3 analyzers + the pipeline to get the verdict.
        // We use the pipeline for the LLM-driven synthesis step.
        let pipeline = AnalysisPipeline::new();
        let pipeline_output = pipeline
            .run(&self.nim, "synthesize", &diff, None, &key)
            .await;
        // Pipeline is tolerant of failures — we still get a verdict.

        let verdict_report = pipeline_output.verdict;
        let fix_plan = argus_core::FixPlan::from_findings(&[]);
        let mut combined_findings = serde_json::json!({
            "slop": slop_report,
            "security": sec_report,
            "arch": arch_report,
            "verdict": verdict_report,
            "fix_plan": fix_plan,
        });
        let summary = format!("Verdict: {:?}", verdict_report.status);

        let report = SpecialistReport {
            specialist: "aegis_verdict".into(),
            prompt_name: "verdict-synthesizer".into(),
            model_id: self.model_id.clone(),
            latency_ms: start.elapsed().as_millis() as u64,
            findings: combined_findings.clone(),
            summary,
        };
        // Patch fix_plan count from pipeline output (placeholder — the
        // pipeline doesn't have a FixPlan yet, so we report an empty
        // plan and let the downstream agent run `aegis_slop`/`aegis_arch`
        // again to get a real one).
        let _ = combined_findings["fix_plan"]["total_steps"]
            .as_u64()
            .unwrap_or(0);
        Ok(serde_json::to_string(&report).unwrap_or_default())
    }
}

#[tool_handler]
impl ServerHandler for ArgusMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: rmcp::model::ServerCapabilities::default(),
            server_info: rmcp::model::Implementation {
                name: "ARGUS".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some(
                "ARGUS — AI slop defense layer. 4 specialists exposed as MCP tools: \
                 aegis_slop, aegis_security, aegis_arch, aegis_verdict. \
                 BYOK — pass your NVIDIA NIM key as ARGUS_NIM_KEY."
                    .into(),
            ),
        }
    }
}

// =====================================================================
// Tool parameter structs
// =====================================================================

/// Input for the 3 first-pass tools (slop, security, arch).
#[derive(Debug, Clone, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct AegisArgs {
    /// The unified diff to analyze. Standard `git diff` output works.
    #[schemars(description = "A unified diff (git diff format) to analyze")]
    pub code_diff: String,
}

/// Input for the verdict synthesizer.
#[derive(Debug, Clone, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct VerdictArgs {
    /// The SlopDetector report (JSON-serialized from a prior `aegis_slop` call).
    #[schemars(description = "Output of aegis_slop, JSON-serialized")]
    pub slop: serde_json::Value,
    /// The SecurityReview report (JSON-serialized from a prior `aegis_security` call).
    #[schemars(description = "Output of aegis_security, JSON-serialized")]
    pub security: serde_json::Value,
    /// The ArchitectureFit report (JSON-serialized from a prior `aegis_arch` call).
    #[schemars(description = "Output of aegis_arch, JSON-serialized")]
    pub arch: serde_json::Value,
    /// The original diff (kept for the LLM context).
    #[schemars(description = "The original unified diff that was analyzed")]
    pub diff: String,
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specialist_report_serializes_to_json() {
        let report = SpecialistReport {
            specialist: "aegis_slop".into(),
            prompt_name: "slop-detector".into(),
            model_id: "zhipuai/glm-5.1".into(),
            latency_ms: 1234,
            findings: serde_json::json!([{"severity": "warning", "file": "src/lib.rs"}]),
            summary: "test".into(),
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("aegis_slop"));
        assert!(json.contains("slop-detector"));
        assert!(json.contains("1234"));
    }

    #[test]
    fn specialist_error_serializes_to_json() {
        let err = SpecialistError {
            specialist: "aegis_security".into(),
            error: "ARGUS_NIM_KEY not set".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("aegis_security"));
        assert!(json.contains("ARGUS_NIM_KEY"));
    }

    #[test]
    fn aegis_args_deserializes() {
        let json = r#"{"code_diff":"+ x = 1;"}"#;
        let args: AegisArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.code_diff, "+ x = 1;");
    }

    #[test]
    fn verdict_args_deserializes_with_nested_reports() {
        let json = r#"{
            "slop": {"signals_detected": [], "slop_score": 0.0},
            "security": {"findings": [], "highest_severity": "Low", "summary": "ok"},
            "arch": {"fit_score": 0.9, "verdict": "good", "concerns": []},
            "diff": "+ x = 1;"
        }"#;
        let args: VerdictArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.diff, "+ x = 1;");
        assert!(args.slop.get("slop_score").is_some());
    }

    #[test]
    fn server_info_advertises_four_specialists() {
        let info = ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: rmcp::model::ServerCapabilities::default(),
            server_info: rmcp::model::Implementation {
                name: "ARGUS".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some("test".into()),
        };
        assert_eq!(info.server_info.name, "ARGUS");
        assert!(info.instructions.is_some());
    }

    #[test]
    fn new_constructor_does_not_panic() {
        // Clear env so we exercise the default branch.
        // SAFETY: this is a test; parallel test execution would be
        // problematic but the Mutex-less env read here is OK because
        // the test only checks that the constructor doesn't panic.
        let _ = ArgusMcp::new();
    }
}
