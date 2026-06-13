//! argus-github-app — GitHub App front door for ARGUS
//!
//! This crate is the binary that runs behind the GitHub App
//! install. It receives webhook deliveries from GitHub, runs
//! the deterministic slop layer on the PR diff, and posts a
//! verdict comment + label back to the PR.
//!
//! ## Architecture
//!
//! ```text
//! GitHub webhook -> /webhook
//!                       |
//!                       v
//!                 signature::verify (HMAC-SHA256, constant-time)
//!                       |
//!                       v
//!                 cordon::CordonEnforcer (size, repo, event, no-SSRF)
//!                       |
//!                       v
//!                 argus_slop::run_deterministic_rules (<100ms)
//!                       |
//!                       v
//!                 argus_github::post_comment + set_labels
//! ```
//!
//! ## Endpoints
//!
//! - `GET  /` — landing page (plain text)
//! - `GET  /health` — liveness probe
//! - `GET  /version` — version + git SHA
//! - `GET  /setup` — GitHub App manifest JSON + install URL
//! - `POST /webhook` — receives GitHub events
//!
//! ## Environment variables
//!
//! Required:
//! - `ARGUS_APP_WEBHOOK_SECRET` — HMAC secret
//! - `ARGUS_APP_INSTALL_TOKEN` — GitHub App installation token
//!
//! Optional:
//! - `PORT` — bind port (default 8080)
//! - `ARGUS_APP_LABEL_PASS` / `ARGUS_APP_LABEL_WARN` / `ARGUS_APP_LABEL_FAIL`
//! - `ARGUS_APP_ALLOWED_REPOS` — comma-separated `owner/repo`
//! - `ARGUS_APP_EVENTS` — comma-separated event names
//! - `ARGUS_NIM_KEY` — BYOK for the LLM layer (optional, deterministic-only default)
//!
//! [Refs: argus-silver-roadmap/P.2]

pub mod app_state;
pub mod cordon;
pub mod setup;
pub mod signature;
pub mod webhook;
