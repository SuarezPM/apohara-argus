//! argus-core — shared types, errors, config, prompt loading
//!
//! This crate is the foundation. Every other ARGUS crate depends on it.
//! It contains:
//! - Domain types (PRFinding, RiskScore, Verdict, PRReview, AgentAction)
//! - Error types
//! - The Argus Prompt Library loader (the 4 .md files)
//! - Configuration (env vars, defaults)

pub mod config;
pub mod errors;
pub mod prompts;
pub mod types;

pub use config::Config;
pub use errors::{ArgusError, Result};
pub use prompts::{Prompt, PromptLibrary, PromptMetadata};
pub use types::{
    AgentAction, AgentRole, AuditEvent, DataClass, DecisionArtifact, FindingSeverity, FixPlan,
    FixStep, FixStepKind, LedgerEntry, LedgerEntryKind, Manifest, OffenderSummary, OrgSummary,
    PRFinding, PRReview, RiskScore, TeamSummary, ToolCallRecord, Verdict, VerdictStatus,
    WeeklyBriefing,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
