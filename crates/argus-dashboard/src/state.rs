//! Dashboard state — the data model for the ARGUS cohort view.
//!
//! A **Cohort** is a named grouping of layers that map to one of the four
//! specialist outputs the verify worker produces (slop, security, arch,
//! verdict). The "cohort view" UX is inspired by CodeRabbit's
//! "Change Stack" pattern: each cohort is a storyline, layers are
//! navigable findings inside it.
//!
//! These types are intentionally framework-free: no axum, no askama.
//! They're plain serde types so they can be serialized to JSON for
//! future htmx endpoints or stored verbatim in the audit log.

use argus_verify::VerifyWorker;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// A cohort is a named grouping of layers (a "storyline" of a PR change).
/// Inspired by CodeRabbit's "Change Stack" pattern.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Cohort {
    pub id: String,
    pub name: String, // e.g., "Aegis Slop", "Aegis Security", "Aegis Arch", "Aegis Verdict"
    pub icon: String, // emoji or unicode
    pub layers: Vec<Layer>,
}

/// A layer is one finding within a cohort.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Layer {
    pub id: String,
    pub summary: String, // "AWS key in commit"
    pub file: String,
    pub line_start: u32,
    pub line_end: u32,
    pub severity: String,   // "info" | "warning" | "error" | "critical"
    pub diff_range: String, // a snippet of the diff
}

/// The full dashboard state, holding all cohorts for a PR review.
///
/// `premium` is initialized from the `ARGUS_PREMIUM` env var on
/// construction; it gates the 5 enterprise routes (org dashboards, custom
/// policy packs, SIEM export). See [`premium_from_env`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardState {
    pub pr_url: String,
    pub pr_title: String,
    pub cohorts: Vec<Cohort>,
    pub premium: bool,
}

impl DashboardState {
    /// Build an empty state for a given PR. Cohorts are added with
    /// [`Self::add_cohort`]. The `premium` flag is read from the
    /// `ARGUS_PREMIUM` env var (true / 1 enables, anything else is off).
    pub fn from_review(pr_url: String, pr_title: String) -> Self {
        Self {
            pr_url,
            pr_title,
            cohorts: Vec::new(),
            premium: premium_from_env(),
        }
    }

    /// Append a cohort. No de-duplication; if the same id is added twice
    /// the template will render both sections (intentional: callers may
    /// want to extend a cohort in-place via `cohort_mut` first).
    pub fn add_cohort(&mut self, cohort: Cohort) {
        self.cohorts.push(cohort);
    }

    /// Look up a cohort by id.
    pub fn cohort(&self, id: &str) -> Option<&Cohort> {
        self.cohorts.iter().find(|c| c.id == id)
    }

    /// Total layer count across all cohorts.
    pub fn total_layers(&self) -> usize {
        self.cohorts.iter().map(|c| c.layers.len()).sum()
    }
}

/// Read the `ARGUS_PREMIUM` env var. Returns `true` only for the
/// canonical opt-ins `"true"` and `"1"`. Anything else (including unset
/// and empty) is `false`. This is the single source of truth for the
/// open-core gate; keep new entry points (tests, scripts, docs) going
/// through this function instead of re-deriving the rule.
pub fn premium_from_env() -> bool {
    std::env::var("ARGUS_PREMIUM")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

/// The axum state shared by every route handler in the dashboard binary.
///
/// The 5 premium routes ([`crate::premium`]) read `premium` to decide
/// between a 402 JSON body and a stub HTML page. The remaining fields
/// are the existing dashboard wiring (worker, NIM model, briefing path)
/// unchanged from the pre-gate binary.
#[derive(Clone)]
pub struct AppState {
    pub worker: Arc<VerifyWorker>,
    pub nim_model: String,
    pub briefings_path: PathBuf,
    pub premium: bool,
}

impl AppState {
    /// Convenience constructor that wires `premium` from
    /// [`premium_from_env`]. The other fields are passed through.
    pub fn with_premium_from_env(
        worker: Arc<VerifyWorker>,
        nim_model: String,
        briefings_path: PathBuf,
    ) -> Self {
        Self {
            worker,
            nim_model,
            briefings_path,
            premium: premium_from_env(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: &str) -> Layer {
        Layer {
            id: id.into(),
            summary: format!("finding {}", id),
            file: "src/lib.rs".into(),
            line_start: 1,
            line_end: 2,
            severity: "info".into(),
            diff_range: "+x".into(),
        }
    }

    fn cohort(id: &str, n: usize) -> Cohort {
        Cohort {
            id: id.into(),
            name: format!("Cohort {}", id),
            icon: "x".into(),
            layers: (0..n).map(|i| layer(&format!("{}-{}", id, i))).collect(),
        }
    }

    #[test]
    fn cohort_lookup_returns_some_for_known_id() {
        let mut s = DashboardState::from_review("u".into(), "t".into());
        s.add_cohort(cohort("slop", 2));
        s.add_cohort(cohort("sec", 1));
        assert_eq!(s.cohort("slop").unwrap().layers.len(), 2);
        assert_eq!(s.cohort("sec").unwrap().layers.len(), 1);
    }

    #[test]
    fn cohort_lookup_returns_none_for_unknown_id() {
        let s = DashboardState::default();
        assert!(s.cohort("nope").is_none());
    }

    #[test]
    fn total_layers_sums_across_cohorts() {
        let mut s = DashboardState::default();
        s.add_cohort(cohort("a", 3));
        s.add_cohort(cohort("b", 0));
        s.add_cohort(cohort("c", 5));
        assert_eq!(s.total_layers(), 8);
    }

    #[test]
    fn from_review_starts_empty() {
        let s = DashboardState::from_review("https://x".into(), "T".into());
        assert_eq!(s.pr_url, "https://x");
        assert_eq!(s.pr_title, "T");
        assert!(s.cohorts.is_empty());
        assert_eq!(s.total_layers(), 0);
    }

    #[test]
    fn default_dashboard_state_has_premium_false() {
        let s = DashboardState::default();
        assert!(!s.premium);
    }

    #[test]
    fn premium_from_env_is_callable() {
        // The function must compile and return a bool. We don't mutate
        // the env here (test-isolation nightmare); the integration tests
        // in `tests/premium_gate.rs` exercise the actual gate via
        // `AppState { premium: ..., .. }` instead.
        let _v: bool = premium_from_env();
    }
}
