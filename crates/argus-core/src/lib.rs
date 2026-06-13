//! argus-core — shared types, errors, config, prompt loading
//!
//! This crate is the foundation. Every other ARGUS crate depends on it.
//! It contains:
//! - Domain types (PRFinding, RiskScore, Verdict, PRReview, AgentAction)
//! - Error types
//! - The Argus Prompt Library loader (the 4 .md files)
//! - Configuration (env vars, defaults)

pub mod errors;
pub mod prompts;
pub mod types;
pub mod config;

pub use errors::{ArgusError, Result};
pub use types::{
    PRFinding, FindingSeverity, RiskScore, Verdict, VerdictStatus,
    PRReview, AgentAction, AgentRole, LedgerEntry, LedgerEntryKind,
    OrgSummary, WeeklyBriefing, OffenderSummary, TeamSummary,
    AuditEvent, DecisionArtifact, ToolCallRecord, Manifest, DataClass,
    FixPlan, FixStep, FixStepKind,
};
pub use prompts::{Prompt, PromptLibrary, PromptMetadata};
pub use config::Config;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
