//! Per-role model registry for ARGUS's NVIDIA NIM client.
//!
//! Different agent roles (slop, security, arch, verdict, lens) have
//! different quality / cost / latency needs. This registry maps each
//! `ModelRole` to its default NIM model and reads per-role env overrides.
//!
//! Resolution order (highest priority first):
//! 1. Programmatic override via `ModelRegistry::with_override`
//! 2. Environment variable (`ARGUS_MODEL_<ROLE>`)
//! 3. Hardcoded default for the role
//!
//! Refs: supremum-roadmap §4.1.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Distinct agent roles that may call the LLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    /// Signal-based slop detection — fast, high-throughput.
    Slop,
    /// Deep security review.
    Security,
    /// Architectural fit review.
    Arch,
    /// Final verdict synthesis — needs structured JSON compliance.
    Verdict,
    /// Weekly briefing generation — simple text.
    Lens,
}

impl ModelRole {
    /// Env var name for overriding the default model for this role.
    pub fn env_var(&self) -> &'static str {
        match self {
            ModelRole::Slop => "ARGUS_MODEL_SLOP",
            ModelRole::Security => "ARGUS_MODEL_SECURITY",
            ModelRole::Arch => "ARGUS_MODEL_ARCH",
            ModelRole::Verdict => "ARGUS_MODEL_VERDICT",
            ModelRole::Lens => "ARGUS_MODEL_LENS",
        }
    }

    /// Hardcoded default model for this role (NIM free tier, June 2026).
    pub fn default_model(&self) -> &'static str {
        match self {
            ModelRole::Slop => "deepseek-ai/deepseek-v4-flash",
            ModelRole::Security => "nvidia/nemotron-3-super-120b",
            ModelRole::Arch => "nvidia/nemotron-3-super-120b",
            ModelRole::Verdict => "zhipuai/glm-5.1",
            ModelRole::Lens => "meta/llama-3.1-70b-instruct",
        }
    }

    /// All roles, for iteration / regression tests.
    pub const ALL: [ModelRole; 5] = [
        ModelRole::Slop,
        ModelRole::Security,
        ModelRole::Arch,
        ModelRole::Verdict,
        ModelRole::Lens,
    ];
}

/// Registry of model assignments. Holds programmatic overrides;
/// env vars and defaults live in `ModelRole`.
#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    pub overrides: HashMap<ModelRole, String>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a programmatic override (highest priority).
    pub fn with_override(mut self, role: ModelRole, model: impl Into<String>) -> Self {
        self.overrides.insert(role, model.into());
        self
    }

    /// Convenience: look up the default model for a role using a fresh registry.
    pub fn default_for_role(role: ModelRole) -> String {
        Self::new().select_for_role(role)
    }

    /// Resolve the model for a role. Checks programmatic override first,
    /// then env var, then the role's hardcoded default.
    pub fn select_for_role(&self, role: ModelRole) -> String {
        if let Some(m) = self.overrides.get(&role) {
            return m.clone();
        }
        if let Ok(m) = std::env::var(role.env_var()) {
            if !m.is_empty() {
                return m;
            }
        }
        role.default_model().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env-mutating tests so they don't race with each other.
    // Use `unwrap_or_else(|e| e.into_inner())` so a poison from a previous
    // failure doesn't cascade into PoisonError panics.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn test_role_to_model_id_happy() {
        let _g = env_lock();
        // Ensure no leftover override from a previous test.
        std::env::remove_var(ModelRole::Slop.env_var());
        let reg = ModelRegistry::new();
        assert_eq!(
            reg.select_for_role(ModelRole::Slop),
            "deepseek-ai/deepseek-v4-flash"
        );
    }

    #[test]
    fn test_env_override() {
        let _g = env_lock();
        std::env::set_var("ARGUS_MODEL_SLOP", "custom/test-model");
        let reg = ModelRegistry::new();
        let m = reg.select_for_role(ModelRole::Slop);
        std::env::remove_var("ARGUS_MODEL_SLOP");
        assert_eq!(m, "custom/test-model");
    }

    #[test]
    fn test_all_roles_mapped() {
        let _g = env_lock();
        for role in ModelRole::ALL {
            std::env::remove_var(role.env_var());
            let m = ModelRegistry::new().select_for_role(role);
            assert!(!m.is_empty(), "role {:?} should have a non-empty model", role);
        }
    }

    #[test]
    fn test_programmatic_override_beats_env() {
        let _g = env_lock();
        std::env::set_var("ARGUS_MODEL_VERDICT", "env-model");
        let reg = ModelRegistry::new().with_override(ModelRole::Verdict, "prog-model");
        let m = reg.select_for_role(ModelRole::Verdict);
        std::env::remove_var("ARGUS_MODEL_VERDICT");
        assert_eq!(m, "prog-model");
    }
}
