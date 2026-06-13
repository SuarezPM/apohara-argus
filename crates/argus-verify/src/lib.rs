//! argus-verify — Aegis Verify, the PR review worker
//!
//! HTTP server that exposes:
//! - `POST /analyze`        — analyze a PR URL, return verdict
//! - `GET  /health`         — health check
//! - `GET  /audit/export`   — NDJSON stream of Article 12 audit events
//!                            (Roadmap 2.2), with a manifest footer.
//!
//! The user provides their NIM key in the `X-LLM-Key` header (BYOK).
//! If a GitHub token is configured, the worker will also post a comment
//! and set a label on the PR.

use argus_core::{ArgusError, DecisionArtifact, FixPlan, PRReview, Verdict, VerdictStatus, RiskScore};
use argus_crypto::chain::append;
use argus_crypto::identity::AgentKeypair;
use argus_github::GitHubClient;
use argus_llm::{audit, ModelRegistry, ModelRole, NimClient};
use argus_slop::pipeline::AnalysisPipeline;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub mod audit_store;
pub mod cache;
pub mod routes;
pub mod shutdown;
pub use audit_store::InMemoryAuditStore;
pub use cache::IdempotencyCache;
pub use routes::{audit_export_handler, a2a_message_handler, agent_card_handler, build_agent_card, AgentCard, A2AMessage, A2APart, AgentSkill};
pub use shutdown::shutdown_signal;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeRequest {
    pub pr_url: String,
    /// Optional repository context (sample of files) for the arch fit check.
    #[serde(default)]
    pub repo_context: Option<String>,
    /// If true, post a comment to the PR.
    #[serde(default)]
    pub post_comment: bool,
    /// If true, set labels on the PR.
    #[serde(default)]
    pub set_labels: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeResponse {
    pub pr_ref: String,
    pub verdict: Verdict,
    pub slop_score: Option<f32>,
    pub fit_score: Option<f32>,
    pub security_summary: Option<String>,
    pub review: PRReview,
    pub comment_posted: bool,
    pub labels_set: bool,
    /// Structured handoff for downstream coding agents (Claude Code /
    /// Codex / Cursor / Devin). Sorted high-severity first. See
    /// `argus_core::FixPlan` for the JSON shape. [Refs: 1.2]
    pub fix_plan: FixPlan,
}

pub struct VerifyWorker {
    pub nim: NimClient,
    pub nim_model: String,
    pub github: Option<GitHubClient>,
    /// Per-worker session-scoped hash chain. Each `analyze()` call reads
    /// this, uses it as the `prev_hash` for the PRReview ledger entry,
    /// then advances it to the new entry's hash. Starts at GENESIS.
    ///
    /// Replaces the previous design where every request reset `prev_hash`
    /// to `argus_crypto::chain::GENESIS_HASH` — that made every review
    /// look like the first link in a new chain and broke tamper-evidence
    /// across requests handled by the same worker process.
    pub prev_hash: Arc<Mutex<[u8; 32]>>,
    /// In-memory audit-event log backing the NDJSON export endpoint
    /// (`GET /audit/export`, Roadmap 2.2). Cloning is cheap — both the
    /// worker (writer, via `analyze()`) and the HTTP route (reader, via
    /// `audit_export_handler`) hold clones that share the same
    /// `Arc<RwLock<Vec<AuditEvent>>>`.
    pub audit_store: InMemoryAuditStore,
    /// Per-worker session-scoped hash chain for the *audit* log
    /// (separate from `prev_hash`, which is for the PRReview ledger).
    /// Each `analyze()` call reads this, uses it as the `prev_hash`
    /// for the emitted `AuditEvent`, then advances it via
    /// `argus_llm::audit::next_prev_hash`.
    pub audit_prev_hash: Arc<Mutex<[u8; 32]>>,
    /// Per-worker ephemeral Ed25519 signing key for audit events.
    /// In production this would be loaded from a KMS/HSM; for the
    /// in-memory store (Roadmap 2.2), a fresh key per worker is
    /// sufficient — the signature proves the event was produced by
    /// *this* process, not a forger.
    pub audit_signing_key: ed25519_dalek::SigningKey,
}

/// Genesis prev_hash for the per-`VerifyWorker` session chain.
/// Mirrors `argus_crypto::chain::GENESIS_HASH` but as a raw `[u8; 32]`
/// so we can store it in an `Arc<Mutex<…>>` without going through hex.
const WORKER_GENESIS: [u8; 32] = [0u8; 32];

impl VerifyWorker {
    pub fn new(nim_key: &str) -> Self {
        Self {
            nim: NimClient::new(),
            nim_model: ModelRegistry::default_for_role(ModelRole::Verdict),
            github: None,
            prev_hash: Arc::new(Mutex::new(WORKER_GENESIS)),
            audit_store: InMemoryAuditStore::new(),
            audit_prev_hash: Arc::new(Mutex::new(WORKER_GENESIS)),
            audit_signing_key: ed25519_dalek::SigningKey::generate(
                &mut rand::rngs::OsRng,
            ),
        }
    }

    pub fn with_model(mut self, m: impl Into<String>) -> Self {
        self.nim_model = m.into();
        self.nim = NimClient::new().with_model(self.nim_model.clone());
        self
    }

    pub fn with_github(mut self, client: GitHubClient) -> Self {
        self.github = Some(client);
        self
    }

    /// Process a PR review request end-to-end.
    pub async fn analyze(
        &self,
        request: AnalyzeRequest,
    ) -> Result<AnalyzeResponse, ArgusError> {
        // Parse the PR URL
        let (owner, repo, number) = GitHubClient::parse_pr_url(&request.pr_url)
            .map_err(|e| ArgusError::InvalidInput(format!("invalid PR URL: {}", e)))?;
        let pr_ref = format!("{}/{}/pull/{}", owner, repo, number);

        // Fetch the diff (if we have a GitHub client)
        let diff = if let Some(gh) = &self.github {
            gh.get_diff(&owner, &repo, number).await
                .map_err(|e| ArgusError::Internal(format!("github diff fetch: {}", e)))?
        } else {
            return Err(ArgusError::InvalidInput(
                "No GitHub client configured; cannot fetch diff. Provide GITHUB_TOKEN.".into()
            ));
        };

        // Run the analysis pipeline
        let pipeline = AnalysisPipeline::new();
        // BYOK: the worker doesn't have the key here; the caller must have already
        // set it via env. For simplicity, we expect ARGUS_NIM_KEY to be set.
        let nim_key = std::env::var("ARGUS_NIM_KEY")
            .map_err(|_| ArgusError::Internal("ARGUS_NIM_KEY not set on server (BYOK required in X-LLM-Key header — server has fallback env)".into()))?;
        let out = pipeline.run(&self.nim, &pr_ref, &diff, request.repo_context.as_deref(), &nim_key).await;

        let risk = out.verdict.risk_score.as_f32();
        let slop_score = out.slop.as_ref().map(|s| s.slop_score);
        let fit_score = out.architecture.as_ref().map(|a| a.fit_score);
        let sec_sum = out.security.as_ref()
            .map(|s| format!("{} findings, highest {:?}", s.findings.len(), s.highest_severity));

        // Build the signed review (for the audit trail).
        //
        // The PRReview ledger prev_hash is the per-worker session hash,
        // not the per-request GENESIS. This makes the chain tamper-evident
        // across requests handled by the same worker process — the
        // previous implementation reset to GENESIS every request, which
        // broke the chain.
        let pr_commit = "fetched-at-runtime".to_string(); // simplified
        let agent = AgentKeypair::generate("aegis-verdict");
        let prev_hash: String = {
            let guard = self.prev_hash.lock().expect("prev_hash mutex poisoned");
            hex::encode(*guard)
        };
        let payload = serde_json::json!({
            "pr_ref": &pr_ref,
            "verdict_status": format!("{:?}", out.verdict.status),
            "risk_score": risk,
            "timestamp": Utc::now().to_rfc3339(),
        });
        let entry = append(&prev_hash, &payload);

        // Advance the per-worker session chain: next prev_hash is this
        // entry's hash, decoded from the hex the ledger produced.
        {
            let new_prev: [u8; 32] = hex::decode(&entry.hash)
                .expect("ledger hash is hex")
                .try_into()
                .expect("ledger hash is 32 bytes");
            *self.prev_hash.lock().expect("prev_hash mutex poisoned") = new_prev;
        }

        let review = PRReview {
            id: Uuid::new_v4(),
            pr_ref: pr_ref.clone(),
            pr_commit_hash: pr_commit,
            verdict: out.verdict.clone(),
            findings: vec![], // simplified for now
            agent_chain: vec![argus_core::AgentAction {
                agent: argus_core::AgentRole::AegisVerdict,
                spiffe_id: agent.spiffe_id.as_str().to_string(),
                action: "VERDICT_EMITTED".to_string(),
                timestamp: Utc::now(),
                ed25519_sig: "see-ledger".to_string(), // simplified
                payload_hash: entry.hash.clone(),
            }],
            created_at: Utc::now(),
            ledger_signature: entry.hash.clone(),
            prev_ledger_hash: prev_hash,
        };

        // Optionally post to GitHub
        let mut comment_posted = false;
        let mut labels_set = false;
        if let Some(gh) = &self.github {
            if request.post_comment {
                let body = GitHubClient::format_verdict_comment(
                    &pr_ref,
                    &format!("{:?}", out.verdict.status),
                    risk,
                    &out.verdict.summary,
                    &out.verdict.key_findings,
                    &out.verdict.action_items,
                    slop_score.unwrap_or(0.5),
                    fit_score.unwrap_or(0.5),
                    sec_sum.as_deref().unwrap_or("n/a"),
                );
                if let Ok(_id) = gh.post_comment(&owner, &repo, number, &body).await {
                    comment_posted = true;
                }
            }
            if request.set_labels {
                let label = match out.verdict.status {
                    VerdictStatus::Approved => "argus/approved",
                    VerdictStatus::ReviewRequired => "argus/needs-review",
                    VerdictStatus::Halted => "argus/halted",
                };
                if gh.set_labels(&owner, &repo, number, &[label]).await.is_ok() {
                    labels_set = true;
                }
            }
        }

        // Emit a single Article 12 audit event summarizing this review
        // (Roadmap 2.2). The cleartext prompt and response are
        // consumed by `emit_audit_event` and only their BLAKE3
        // fingerprints reach the store.
        let verdict_label = format!("{:?}", out.verdict.status).to_lowercase();
        let decision = DecisionArtifact {
            verdict: verdict_label,
            findings_count: out.verdict.key_findings.len() as u32,
            rationale: out.verdict.summary.clone(),
        };
        let audit_prev = *self
            .audit_prev_hash
            .lock()
            .expect("audit_prev_hash mutex poisoned");
        let audit_event = audit::emit_audit_event(
            &self.nim_model,
            "verify-worker-v1",
            &diff, // prompt_text — fingerprinted and dropped, never stored
            &out.verdict.summary, // raw_response — same posture
            0.7, // temperature — the worker's default for verdict synthesis
            vec![], // tool_calls — verifier makes no tool calls in this iteration
            decision,
            audit_prev,
            None,
            None,
            &self.audit_signing_key,
        );
        // Advance the audit chain so the next emission links to this one.
        *self
            .audit_prev_hash
            .lock()
            .expect("audit_prev_hash mutex poisoned") =
            audit::next_prev_hash(audit_prev, &audit_event);
        // Persist. The route reads from this store; the worker is the
        // sole writer in normal operation.
        self.audit_store.append(audit_event).await;

        // Build the hand-off plan BEFORE moving `review` into the
        // response struct — `from_findings` borrows `review.findings`.
        let fix_plan = FixPlan::from_findings(&review.findings);

        Ok(AnalyzeResponse {
            pr_ref,
            verdict: out.verdict,
            slop_score,
            fit_score,
            security_summary: sec_sum,
            review,
            comment_posted,
            labels_set,
            fix_plan,
        })
    }
}
