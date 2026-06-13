//! The argus-dashboard library — the SSR landing page and the public
//! HTTP surface for the open-core gate.
//!
//! The binary target in `src/main.rs` wires the dashboard server; the
//! integration tests in `tests/premium_gate.rs` exercise the premium
//! gate against the same `routes()` builder the binary uses.
//!
//! Public surface:
//! - `state::{AppState, DashboardState, Cohort, Layer, premium_from_env}`
//! - `premium::{routes, premium_required_response}`
//!
//! Everything else is internal to the binary.

pub mod premium;
pub mod state;
pub mod templates;
