//! argus-agent — the ARGUS agent as documented Rust code (P4 deliverable)
//!
//! This crate encodes the agent's identity, capabilities, decision rules,
//! and routing logic in Rust types. The runtime executors (`argus-verify`,
//! `argus-lens`) implement the actual LLM calls; this crate defines WHAT
//! the agent is and HOW it routes work.
//!
//! **Platzi P4 mapping:** the brief asks for "el agente que tu empresa
//! necesita — con contexto, skills y conexiones a datos reales". This
//! crate is the agent, expressed as code:
//!
//! - [`AgentSpec`] — declarative identity (name, role, capabilities, constraints)
//! - [`DecisionLog`] — append-only audit log of every decision the agent made
//! - [`Orchestrator`] — the routing logic that dispatches a task to the right
//!   specialist agent (slop / security / arch / verdict) and aggregates the result
//! - [`CordonEnforcer`] — runtime guard that ensures the verdict synthesizer
//!   never sees raw code (the Cordon Principle from the academic literature)
//!
//! Every public type is JSON-serializable so the agent's behavior can be
//! logged, audited, and reproduced.

use apohara_argus_core::AgentRole;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// AgentSpec — declarative identity of one specialist agent
// ============================================================================

/// A declarative description of one specialist agent in the ARGUS collective.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    /// SPIFFE-like identity, e.g. `spiffe://apohara.dev/argus/aegis-slop/instance/{uuid}`
    pub spiffe_id: String,
    /// The role of this agent within the collective.
    pub role: AgentRole,
    /// The prompt this agent loads from `apohara-argus-core::prompts`.
    pub prompt_name: String,
    /// Skills this agent has.
    pub capabilities: Vec<String>,
    /// What context this agent needs to do its job.
    pub context_required: ContextRequirement,
    /// Constraints the agent operates under.
    pub constraints: Vec<Constraint>,
    /// Default temperature for the LLM call.
    pub default_temperature: f32,
    /// Default max_tokens for the LLM call.
    pub default_max_tokens: u32,
}

/// What context an agent needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextRequirement {
    /// Just the PR diff. Nothing else.
    DiffOnly,
    /// The diff + a sample of the existing repo (for arch fit).
    DiffPlusRepoSample,
    /// The structured outputs of other agents. No raw code.
    /// This is what the verdict synthesizer receives.
    OtherAgentsOutputs,
}

/// A constraint on the agent's behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Constraint {
    /// The agent must never see raw natural-language code (Cordon Principle).
    NoRawCode,
    /// The agent's temperature must be <= this value (for deterministic output).
    MaxTemperature(f32),
    /// The agent must produce output matching this JSON schema name.
    MustProduceJson(String),
    /// The agent must NOT make final merge decisions.
    NoMergeDecisions,
}

impl AgentSpec {
    /// Build the canonical AgentSpec for the 4 specialists + 2 orchestrators.
    pub fn canonical(role: AgentRole) -> Self {
        match role {
            AgentRole::AegisSlop => aegis_slop_spec(role),
            AgentRole::AegisSecurity => aegis_security_spec(role),
            AgentRole::AegisArch => aegis_arch_spec(role),
            AgentRole::AegisVerdict => aegis_verdict_spec(role),
            AgentRole::AegisScope | AgentRole::AegisLens => aegis_orchestrator_spec(role),
        }
    }

    /// Returns true if the agent operates under the Cordon Principle.
    pub fn is_cordon_enforced(&self) -> bool {
        self.constraints
            .iter()
            .any(|c| matches!(c, Constraint::NoRawCode))
    }

    /// Returns true if this agent requires diff-only context.
    pub fn diff_only(&self) -> bool {
        matches!(self.context_required, ContextRequirement::DiffOnly)
    }
}

/// AegisSlop specialist: AI-generated slop detection. Low temperature
/// (0.1) keeps the JSON output reproducible. Output: `slop_report`.
fn aegis_slop_spec(role: AgentRole) -> AgentSpec {
    AgentSpec {
        spiffe_id: format!(
            "spiffe://apohara.dev/argus/aegis-slop/instance/{}",
            Uuid::new_v4()
        ),
        role,
        prompt_name: "slop-detector".into(),
        capabilities: vec![
            "detect_ai_slop_signals".into(),
            "score_slop_probability".into(),
            "extract_signal_examples".into(),
        ],
        context_required: ContextRequirement::DiffOnly,
        constraints: vec![
            Constraint::MaxTemperature(0.1),
            Constraint::MustProduceJson("slop_report".into()),
        ],
        default_temperature: 0.1,
        default_max_tokens: 1024,
    }
}

/// AegisSecurity specialist: red-team security review. Zero
/// temperature (deterministic for repeatable findings). Output:
/// `security_report`.
fn aegis_security_spec(role: AgentRole) -> AgentSpec {
    AgentSpec {
        spiffe_id: format!(
            "spiffe://apohara.dev/argus/aegis-security/instance/{}",
            Uuid::new_v4()
        ),
        role,
        prompt_name: "redteam-security".into(),
        capabilities: vec![
            "detect_hardcoded_secrets".into(),
            "detect_command_injection".into(),
            "detect_sql_injection".into(),
            "detect_path_traversal".into(),
            "detect_unsafe_deserialization".into(),
            "detect_crypto_misuse".into(),
            "detect_sensitive_data_in_logs".into(),
        ],
        context_required: ContextRequirement::DiffOnly,
        constraints: vec![
            Constraint::MaxTemperature(0.0),
            Constraint::MustProduceJson("security_report".into()),
        ],
        default_temperature: 0.0,
        default_max_tokens: 1536,
    }
}

/// AegisArch specialist: architecture-fit review. Needs diff +
/// repo sample (to detect naming + helper-reuse patterns).
/// Output: `arch_report`.
fn aegis_arch_spec(role: AgentRole) -> AgentSpec {
    AgentSpec {
        spiffe_id: format!(
            "spiffe://apohara.dev/argus/aegis-arch/instance/{}",
            Uuid::new_v4()
        ),
        role,
        prompt_name: "architecture-fit".into(),
        capabilities: vec![
            "evaluate_naming_conventions".into(),
            "detect_error_handling_consistency".into(),
            "detect_helper_reuse_opportunities".into(),
            "detect_logging_convention_consistency".into(),
            "score_architecture_fit".into(),
        ],
        context_required: ContextRequirement::DiffPlusRepoSample,
        constraints: vec![
            Constraint::MaxTemperature(0.2),
            Constraint::MustProduceJson("arch_report".into()),
        ],
        default_temperature: 0.2,
        default_max_tokens: 1280,
    }
}

/// AegisVerdict specialist: synthesizes the 3 specialist outputs
/// into an approve / warn / halt decision. The `NoRawCode`
/// constraint enforces the Cordon Principle — the verdict
/// synthesizer must NEVER see raw diff text, only the
/// `RedactedSpecialistReport` from each upstream specialist.
fn aegis_verdict_spec(role: AgentRole) -> AgentSpec {
    AgentSpec {
        spiffe_id: format!(
            "spiffe://apohara.dev/argus/aegis-verdict/instance/{}",
            Uuid::new_v4()
        ),
        role,
        prompt_name: "verdict-synthesizer".into(),
        capabilities: vec![
            "synthesize_verdict_from_3_outputs".into(),
            "decide_approve_warn_halt".into(),
            "generate_action_items".into(),
        ],
        context_required: ContextRequirement::OtherAgentsOutputs,
        constraints: vec![
            Constraint::NoRawCode,
            Constraint::MaxTemperature(0.3),
            Constraint::NoMergeDecisions,
        ],
        default_temperature: 0.3,
        default_max_tokens: 1024,
    }
}

/// Orchestrator role (AegisScope or AegisLens). Orchestrators
/// don't run their own LLM call — they route the diff through
/// the 3 specialists and synthesize the result. Hence
/// `default_max_tokens: 0` (no completion) and an empty
/// `prompt_name` (no prompt to load).
fn aegis_orchestrator_spec(role: AgentRole) -> AgentSpec {
    AgentSpec {
        spiffe_id: format!(
            "spiffe://apohara.dev/argus/aegis-{}/instance/{}",
            role.as_str(),
            Uuid::new_v4()
        ),
        role,
        prompt_name: String::new(),
        capabilities: vec!["orchestrate".into()],
        context_required: ContextRequirement::DiffOnly,
        constraints: vec![Constraint::NoMergeDecisions],
        default_temperature: 0.0,
        default_max_tokens: 0,
    }
}

// ============================================================================
// DecisionLog — append-only audit log of every decision
// ============================================================================

/// One entry in the agent's decision log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionLogEntry {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub spiffe_id: String,
    pub decision_type: DecisionType,
    pub input_summary: String,
    pub output_summary: String,
    pub reasoning: String,
    /// Hash of the previous entry. Empty for the genesis entry.
    pub prev_hash: String,
    /// Hash of this entry.
    pub entry_hash: String,
}

/// The kind of decision being logged.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionType {
    /// The orchestrator routed a task to a specialist.
    TaskRouted,
    /// A specialist agent produced output.
    SpecialistOutput,
    /// The verdict synthesizer emitted a final verdict.
    VerdictEmitted,
    /// A Cordon Principle violation was blocked.
    CordonViolationBlocked,
    /// The agent defaulted to HALTED due to an internal error.
    DefensiveHalt,
}

/// An append-only decision log. Maintains a simple BLAKE3 hash chain.
#[derive(Debug, Default)]
pub struct DecisionLog {
    entries: Vec<DecisionLogEntry>,
}

impl DecisionLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a new entry. The `prev_hash` is taken from the current tail.
    pub fn append(
        &mut self,
        spiffe_id: impl Into<String>,
        decision_type: DecisionType,
        input_summary: impl Into<String>,
        output_summary: impl Into<String>,
        reasoning: impl Into<String>,
    ) -> &DecisionLogEntry {
        let prev_hash = self
            .entries
            .last()
            .map(|e| e.entry_hash.clone())
            .unwrap_or_default();
        let input_s: String = input_summary.into();
        let output_s: String = output_summary.into();
        // Simple BLAKE3-based hash of (prev_hash + input + output)
        let mut hasher = blake3::Hasher::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(input_s.as_bytes());
        hasher.update(b"|".as_ref());
        hasher.update(output_s.as_bytes());
        let entry_hash = format!("blake3:{}", hasher.finalize().to_hex());
        let entry = DecisionLogEntry {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            spiffe_id: spiffe_id.into(),
            decision_type,
            input_summary: input_s,
            output_summary: output_s,
            reasoning: reasoning.into(),
            prev_hash,
            entry_hash,
        };
        self.entries.push(entry);
        self.entries.last().unwrap()
    }

    pub fn entries(&self) -> &[DecisionLogEntry] {
        &self.entries
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ============================================================================
// CordonEnforcer — runtime guard for the Cordon Principle
// ============================================================================

#[derive(Error, Debug)]
pub enum CordonError {
    #[error(
        "Cordon Principle violation: a synthesis agent would have access to raw code. Blocked."
    )]
    RawCodeLeak,
    #[error("Agent spec is invalid: {0}")]
    InvalidSpec(String),
}

/// Enforces the Cordon Principle at runtime: a synthesizer agent (the verdict
/// synthesizer) must NEVER receive raw code. It only receives the structured
/// outputs of the other 3 specialists.
#[derive(Debug, Default)]
pub struct CordonEnforcer;

impl CordonEnforcer {
    pub fn new() -> Self {
        Self
    }

    /// Verify that the context about to be sent to a synthesizer agent is safe.
    pub fn verify_safe_to_synthesize(
        &self,
        context_type: &ContextRequirement,
    ) -> Result<(), CordonError> {
        match context_type {
            ContextRequirement::OtherAgentsOutputs => Ok(()),
            ContextRequirement::DiffOnly | ContextRequirement::DiffPlusRepoSample => {
                Err(CordonError::RawCodeLeak)
            }
        }
    }

    /// Scan a JSON value for raw code (heuristic: lines starting with "+ " or "- ").
    pub fn verify_no_raw_code_in_json(&self, value: &serde_json::Value) -> Result<(), CordonError> {
        Self::scan_for_raw_code(value)
    }

    fn scan_for_raw_code(value: &serde_json::Value) -> Result<(), CordonError> {
        match value {
            serde_json::Value::String(s) => {
                if s.lines()
                    .any(|l| l.starts_with("+ ") || l.starts_with("- "))
                    && s.contains('\n')
                {
                    return Err(CordonError::RawCodeLeak);
                }
                Ok(())
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    Self::scan_for_raw_code(v)?;
                }
                Ok(())
            }
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    let kl = k.to_lowercase();
                    if (kl.contains("raw") && kl.contains("code")) || kl == "raw_diff" {
                        return Err(CordonError::RawCodeLeak);
                    }
                    Self::scan_for_raw_code(v)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

// ============================================================================
// Orchestrator — the agent's routing logic
// ============================================================================

/// The orchestrator knows which specialist agent should handle which task.
#[derive(Debug, Serialize, Deserialize)]
pub struct Orchestrator {
    pub name: String,
    pub specialists: Vec<AgentSpec>,
    /// The decision log (append-only, signed via BLAKE3 hash chain).
    /// Not Clone (mutable in place), but Serialize/Deserialize for state dumps.
    #[serde(skip)]
    pub log: DecisionLog,
    /// Cordon enforcer for safe synthesis.
    #[serde(skip)]
    pub cordon: CordonEnforcer,
}

impl Orchestrator {
    /// Build the canonical ARGUS orchestrator with the 4 specialist agents.
    pub fn canonical() -> Self {
        let specialists = vec![
            AgentSpec::canonical(AgentRole::AegisSlop),
            AgentSpec::canonical(AgentRole::AegisSecurity),
            AgentSpec::canonical(AgentRole::AegisArch),
            AgentSpec::canonical(AgentRole::AegisVerdict),
        ];
        Self {
            name: "ARGUS Aegis Orchestrator".into(),
            specialists,
            log: DecisionLog::new(),
            cordon: CordonEnforcer::new(),
        }
    }

    /// Find the specialist that should handle a given role.
    pub fn find_specialist(&self, role: AgentRole) -> Option<&AgentSpec> {
        self.specialists.iter().find(|s| s.role == role)
    }

    /// Build the routing plan for a full PR review: 3 parallel specialists
    /// (slop, security, arch) feeding into 1 synthesizer (verdict).
    pub fn routing_plan_for_pr_review(&self) -> Vec<AgentRole> {
        vec![
            AgentRole::AegisSlop,
            AgentRole::AegisSecurity,
            AgentRole::AegisArch,
            AgentRole::AegisVerdict,
        ]
    }

    /// Build the routing plan for a weekly digest: just the Lens role.
    pub fn routing_plan_for_weekly_digest(&self) -> Vec<AgentRole> {
        vec![AgentRole::AegisLens]
    }

    /// Validate that a routing plan respects the Cordon Principle.
    pub fn validate_routing_plan_safety(&self, plan: &[AgentRole]) -> Result<(), CordonError> {
        for (i, role) in plan.iter().enumerate() {
            if let Some(spec) = self.find_specialist(*role) {
                if spec.is_cordon_enforced() && i == 0 {
                    return Err(CordonError::InvalidSpec(format!(
                        "Cordon-enforced agent {:?} cannot be first in routing plan (it needs other outputs first)",
                        role
                    )));
                }
            }
        }
        Ok(())
    }

    /// Record a routing decision in the audit log.
    pub fn record_routing(
        &mut self,
        specialist_role: AgentRole,
        task_summary: impl Into<String>,
        reasoning: impl Into<String>,
    ) -> &DecisionLogEntry {
        let spiffe_id = self
            .find_specialist(specialist_role)
            .map(|s| s.spiffe_id.clone())
            .unwrap_or_else(|| "unknown".into());
        self.log.append(
            spiffe_id,
            DecisionType::TaskRouted,
            task_summary,
            format!("routed to {:?}", specialist_role),
            reasoning,
        )
    }

    /// Record that a Cordon violation was blocked.
    pub fn record_cordon_block(&mut self, attempted_role: AgentRole) -> &DecisionLogEntry {
        let spiffe_id = self
            .find_specialist(attempted_role)
            .map(|s| s.spiffe_id.clone())
            .unwrap_or_else(|| "unknown".into());
        self.log.append(
            spiffe_id,
            DecisionType::CordonViolationBlocked,
            "attempted to pass raw code to synthesizer",
            "blocked by CordonEnforcer",
            "Cordon Principle: synthesis agents must not access raw code",
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use apohara_argus_core::{RiskScore, Verdict, VerdictStatus};

    #[test]
    fn slop_spec_has_cordon_compatible_constraints() {
        let s = AgentSpec::canonical(AgentRole::AegisSlop);
        assert!(!s.is_cordon_enforced());
        assert!(s.diff_only());
        assert_eq!(s.prompt_name, "slop-detector");
    }

    #[test]
    fn verdict_spec_is_cordon_enforced() {
        let s = AgentSpec::canonical(AgentRole::AegisVerdict);
        assert!(
            s.is_cordon_enforced(),
            "verdict synthesizer must never see raw code"
        );
        assert!(!s.diff_only());
        assert!(matches!(
            s.context_required,
            ContextRequirement::OtherAgentsOutputs
        ));
    }

    #[test]
    fn security_spec_has_low_temperature() {
        let s = AgentSpec::canonical(AgentRole::AegisSecurity);
        assert!(!s.is_cordon_enforced());
        assert!(s.default_temperature <= 0.0 + 0.001);
    }

    #[test]
    fn routing_plan_respects_cordon() {
        let orch = Orchestrator::canonical();
        let plan = orch.routing_plan_for_pr_review();
        assert!(orch.validate_routing_plan_safety(&plan).is_ok());
    }

    #[test]
    fn routing_plan_rejects_cordon_first() {
        let orch = Orchestrator::canonical();
        let plan = vec![AgentRole::AegisVerdict, AgentRole::AegisSlop];
        assert!(orch.validate_routing_plan_safety(&plan).is_err());
    }

    #[test]
    fn cordon_blocks_diff_context() {
        let e = CordonEnforcer::new();
        assert!(e
            .verify_safe_to_synthesize(&ContextRequirement::DiffOnly)
            .is_err());
        assert!(e
            .verify_safe_to_synthesize(&ContextRequirement::OtherAgentsOutputs)
            .is_ok());
    }

    #[test]
    fn cordon_detects_raw_diff_in_json() {
        let e = CordonEnforcer::new();
        let bad = serde_json::json!({
            "raw_code": "+ print('hello')\n- print('bye')"
        });
        assert!(e.verify_no_raw_code_in_json(&bad).is_err());

        let good = serde_json::json!({
            "verdict": "HALTED",
            "risk_score": 0.9,
            "findings": []
        });
        assert!(e.verify_no_raw_code_in_json(&good).is_ok());
    }

    #[test]
    fn cordon_detects_raw_diff_field() {
        let e = CordonEnforcer::new();
        let bad = serde_json::json!({
            "raw_diff": "+ added line\n- removed line"
        });
        assert!(e.verify_no_raw_code_in_json(&bad).is_err());
    }

    #[test]
    fn decision_log_chains_correctly() {
        let mut log = DecisionLog::new();
        let prev_hash_after_first = {
            let e1 = log.append(
                "aegis-1",
                DecisionType::TaskRouted,
                "task A",
                "routed",
                "reason A",
            );
            assert_eq!(e1.prev_hash, "");
            e1.entry_hash.clone()
        };
        let e2 = log.append(
            "aegis-2",
            DecisionType::SpecialistOutput,
            "task B",
            "output",
            "reason B",
        );
        assert_eq!(e2.prev_hash, prev_hash_after_first);
        assert_ne!(prev_hash_after_first, e2.entry_hash);
    }

    #[test]
    fn orchestrator_records_routing_in_log() {
        let mut orch = Orchestrator::canonical();
        assert_eq!(orch.log.len(), 0);
        orch.record_routing(
            AgentRole::AegisSlop,
            "review PR #42",
            "first step in PR review",
        );
        assert_eq!(orch.log.len(), 1);
        assert_eq!(
            orch.log.entries()[0].decision_type,
            DecisionType::TaskRouted
        );
    }

    #[test]
    fn verdict_emitted_in_log() {
        let mut orch = Orchestrator::canonical();
        let _verdict = Verdict {
            status: VerdictStatus::Halted,
            risk_score: RiskScore::new(0.9),
            summary: "Critical security issue".into(),
            key_findings: vec!["Hardcoded AWS key".into()],
            action_items: vec!["Move to env vars".into()],
            reasoning: "Test".into(),
            issued_at: Utc::now(),
        };
        orch.log.append(
            "aegis-verdict",
            DecisionType::VerdictEmitted,
            "3 slop reports",
            "HALTED with risk 0.9",
            "Cordon-compliant synthesis",
        );
        assert_eq!(orch.log.len(), 1);
        assert!(orch.log.entries()[0].output_summary.contains("HALTED"));
    }

    #[test]
    fn all_4_specialists_present_in_canonical() {
        let orch = Orchestrator::canonical();
        assert!(orch.find_specialist(AgentRole::AegisSlop).is_some());
        assert!(orch.find_specialist(AgentRole::AegisSecurity).is_some());
        assert!(orch.find_specialist(AgentRole::AegisArch).is_some());
        assert!(orch.find_specialist(AgentRole::AegisVerdict).is_some());
    }
}
