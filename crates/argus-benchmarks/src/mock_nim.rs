//! Mock NIM client that returns deterministic, ground-truth-aligned
//! responses for the labeled PR dataset.
//!
//! Purpose: the P/R bench needs an LLM layer to feed the pipeline
//! without making real API calls. The existing `argus_llm::MockClient`
//! is a general-purpose mock for tests (it just returns canned JSON
//! for a few substrings). For *benchmarks* we need responses that are
//! reproducible AND aligned with the dataset's ground truth — so the
//! P/R numbers reflect ARGUS's deterministic layer + synthesizer
//! (not the LLM's accuracy, which is not the bench's concern).
//!
//! Strategy:
//! - Hash the user message and look up the entry in the dataset.
//! - If the hash matches a labeled PR, return a deterministic JSON
//!   payload whose `verdict` mirrors the ground truth.
//! - For the latency/cost benches, count tokens realistically
//!   (≈len/4 for prompt, fixed for completion) so the cost numbers
//!   are meaningful even though no network call is made.

use std::collections::HashMap;

use async_trait::async_trait;

use argus_llm::{CompletionRequest, CompletionResponse, LlmClient, LlmError, Role, Usage};

use crate::dataset::LabeledPR;

/// Mock LLM client. Loads the labeled PR dataset at construction
/// time and answers "is this slop?" with the ground truth label.
///
/// Determinism is load-bearing: every call with the same `user_msg`
/// returns the same response, byte-for-byte. The bench depends on
/// this for reproducible numbers.
pub struct MockNimClient {
    /// Hash of user message -> ground truth label. Built once from
    /// the dataset at construction.
    by_hash: HashMap<u64, LabeledPR>,
}

impl MockNimClient {
    /// Build a mock from a slice of labeled PRs. The hash key is
    /// `blake3` over the user message (truncated to 8 bytes for
    /// the HashMap key). The full hash is in the response so the
    /// bench can also assert the lookup matched.
    pub fn new(prs: &[LabeledPR]) -> Self {
        let mut by_hash = HashMap::with_capacity(prs.len());
        for pr in prs {
            by_hash.insert(hash_user_msg(&pr.diff), pr.clone());
        }
        Self { by_hash }
    }

    /// Look up a labeled PR by user-message hash. Returns `None` if
    /// the user message is not in the dataset (the bench treats that
    /// as an error, but the mock stays a polite no-op).
    pub fn lookup(&self, user_msg: &str) -> Option<&LabeledPR> {
        self.by_hash.get(&hash_user_msg(user_msg))
    }
}

/// BLAKE3 over the user message, truncated to 8 bytes. 64 bits of
/// collision space is more than enough for a 30-entry dataset and
/// keeps the HashMap key a `u64` (no allocation).
fn hash_user_msg(user_msg: &str) -> u64 {
    let h = blake3::hash(user_msg.as_bytes());
    let bytes = h.as_bytes();
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

/// Approximate token count from a string. The real LLM providers use
/// BPE tokenizers; for the cost bench we use the common ≈chars/4
/// rule of thumb. This is a placeholder, not a measurement; the
/// `docs/BENCHMARK.md` "Limitations" section calls it out.
fn approx_tokens(s: &str) -> u32 {
    (s.len() as u32) / 4
}

#[async_trait]
impl LlmClient for MockNimClient {
    fn provider_name(&self) -> &str {
        "mock-nim"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        _api_key: &str,
    ) -> Result<CompletionResponse, LlmError> {
        // Mirror the production NIM path: rebuild the prompt text,
        // hash the user message, look up the label.
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::User))
            .map(|m| m.content.clone())
            .unwrap_or_default();

        let prompt_text: String = request
            .messages
            .iter()
            .map(|m| match m.role {
                Role::System => format!("[system] {}", m.content),
                Role::User => format!("[user] {}", m.content),
                Role::Assistant => format!("[assistant] {}", m.content),
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt_tokens = approx_tokens(&prompt_text);

        // The lookup: in the dataset, the ground truth is binary
        // (slop / clean). We emit a verdict-synthesizer-shaped
        // payload so the P/R bench can parse the response uniformly
        // regardless of which specialist was called.
        let (content, completion_tokens) = match self.lookup(&last_user) {
            Some(pr) => {
                let is_slop = pr.label().is_slop();
                let content = if is_slop {
                    // SLOP: blocked at the deterministic layer would
                    // already have caught most of this; the
                    // synthesizer escalates to a "review required"
                    // verdict.
                    serde_json::json!({
                        "verdict": "REVIEW_REQUIRED",
                        "risk_score": 0.85,
                        "summary": format!("Mock: ground-truth SLOP ({})", pr.id),
                        "key_findings": ["Deterministic slop rule(s) matched"],
                        "action_items": ["Manual review recommended"],
                        "reasoning": "Mock: dataset label = slop",
                    })
                    .to_string()
                } else {
                    serde_json::json!({
                        "verdict": "APPROVE",
                        "risk_score": 0.10,
                        "summary": format!("Mock: ground-truth CLEAN ({})", pr.id),
                        "key_findings": [],
                        "action_items": [],
                        "reasoning": "Mock: dataset label = clean",
                    })
                    .to_string()
                };
                (content.clone(), approx_tokens(&content))
            }
            None => {
                // User message not in the dataset — be loud. The
                // bench treats this as an error in the per-PR table
                // but the mock itself is well-defined.
                let content = serde_json::json!({
                    "verdict": "UNKNOWN",
                    "risk_score": 0.0,
                    "summary": "Mock: user message not in labeled dataset",
                    "key_findings": [],
                    "action_items": [],
                    "reasoning": "Mock: lookup miss",
                })
                .to_string();
                (content.clone(), approx_tokens(&content))
            }
        };

        Ok(CompletionResponse {
            content,
            model: "mock-nim-v1".to_string(),
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        })
    }
}

// Build a deterministic mock response WITHOUT an async call. Used by
// the cost bench to skip the async runtime (cost is a pure
// accounting measurement; latency is a separate bench).
impl MockNimClient {
    /// Synchronous token estimate for a (system, user) prompt pair.
    /// Mirrors the prompt_token accounting in [`complete`] but
    /// returns a `Usage` directly so the cost bench can run in a
    /// tight loop without the tokio runtime.
    pub fn estimate_tokens_sync(&self, system: &str, user: &str) -> Usage {
        let prompt_text = format!("[system] {}\n[user] {}", system, user);
        let prompt_tokens = approx_tokens(&prompt_text);
        let completion = match self.lookup(user) {
            Some(pr) if pr.label().is_slop() => {
                r#"{"verdict":"REVIEW_REQUIRED","risk_score":0.85,"summary":"Mock","key_findings":[],"action_items":[],"reasoning":"Mock"}"#
            }
            Some(_) => {
                r#"{"verdict":"APPROVE","risk_score":0.10,"summary":"Mock","key_findings":[],"action_items":[],"reasoning":"Mock"}"#
            }
            None => {
                r#"{"verdict":"UNKNOWN","risk_score":0.0,"summary":"Mock","key_findings":[],"action_items":[],"reasoning":"Mock"}"#
            }
        };
        let completion_tokens = approx_tokens(completion);
        Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }
}

// Re-export the Message type for callers that need to build
// requests without depending on argus_llm directly.
#[allow(unused_imports)]
pub use argus_llm::Message as MockMessage;

#[cfg(test)]
mod tests {
    //! The MockNimClient is the benchmark's LLM layer. It must
    //! return deterministic, ground-truth-aligned responses for
    //! every PR in the dataset. These tests pin that contract.

    use super::*;
    use crate::dataset::{Label, LabeledPR};
    use argus_llm::Message;

    fn make_pr(id: &str, diff: &str, label: Label) -> LabeledPR {
        LabeledPR {
            id: id.to_string(),
            title: "test".into(),
            diff: diff.to_string(),
            ground_truth: label,
            language: "rust".into(),
            source: "synthetic".into(),
            notes: None,
        }
    }

    #[test]
    fn provider_name_is_mock_nim() {
        let prs = vec![make_pr("a", "diff-a", Label::Slop)];
        let c = MockNimClient::new(&prs);
        assert_eq!(c.provider_name(), "mock-nim");
    }

    #[test]
    fn new_stores_all_prs() {
        let prs = vec![
            make_pr("a", "diff-a", Label::Slop),
            make_pr("b", "diff-b", Label::Clean),
        ];
        let c = MockNimClient::new(&prs);
        // Both diffs must be findable via lookup.
        assert!(c.lookup("diff-a").is_some());
        assert!(c.lookup("diff-b").is_some());
        // A diff that wasn't in the dataset returns None.
        assert!(c.lookup("not-in-dataset").is_none());
    }

    #[test]
    fn lookup_finds_pr_by_diff_hash() {
        let prs = vec![make_pr("unique-id-1", "unique-diff-content", Label::Slop)];
        let c = MockNimClient::new(&prs);
        let found = c.lookup("unique-diff-content").expect("should find");
        assert_eq!(found.id, "unique-id-1");
        assert_eq!(found.ground_truth, Label::Slop);
    }

    #[test]
    fn lookup_returns_none_for_unknown_diff() {
        let prs = vec![make_pr("a", "diff-a", Label::Slop)];
        let c = MockNimClient::new(&prs);
        assert!(c.lookup("completely-different-content").is_none());
    }

    #[tokio::test]
    async fn complete_with_slop_label_returns_review_required() {
        // The slop branch emits a verdict-synthesizer-shaped
        // payload with verdict=REVIEW_REQUIRED and risk_score=0.85.
        let prs = vec![make_pr("slop-1", "slop-diff", Label::Slop)];
        let c = MockNimClient::new(&prs);
        let req = CompletionRequest::new("m", vec![Message::user("slop-diff")]);
        let resp = c.complete(req, "").await.expect("complete ok");
        assert!(resp.content.contains("REVIEW_REQUIRED"));
        assert!(resp.content.contains("0.85"));
        assert!(resp.content.contains("slop-1"));
        assert_eq!(resp.model, "mock-nim-v1");
        // Token accounting: prompt_tokens = len(prompt_text)/4.
        assert!(resp.usage.prompt_tokens > 0);
        assert_eq!(
            resp.usage.total_tokens,
            resp.usage.prompt_tokens + resp.usage.completion_tokens
        );
    }

    #[tokio::test]
    async fn complete_with_clean_label_returns_approve() {
        // The clean branch emits verdict=APPROVE and risk_score=0.10.
        let prs = vec![make_pr("clean-1", "clean-diff", Label::Clean)];
        let c = MockNimClient::new(&prs);
        let req = CompletionRequest::new("m", vec![Message::user("clean-diff")]);
        let resp = c.complete(req, "").await.expect("complete ok");
        assert!(resp.content.contains("APPROVE"));
        // JSON serializes 0.10 as 0.1 (trailing zero dropped).
        assert!(resp.content.contains("0.1"));
        assert!(resp.content.contains("clean-1"));
    }

    #[tokio::test]
    async fn complete_with_unknown_user_msg_returns_unknown_verdict() {
        // When the user message is not in the dataset, the
        // mock returns a polite "unknown" verdict (verdict=
        // UNKNOWN, risk_score=0.0). This is the bench's
        // "lookup miss" path.
        let prs = vec![make_pr("a", "diff-a", Label::Slop)];
        let c = MockNimClient::new(&prs);
        let req = CompletionRequest::new("m", vec![Message::user("never-seen-before")]);
        let resp = c.complete(req, "").await.expect("complete ok");
        assert!(resp.content.contains("UNKNOWN"));
        assert!(resp.content.contains("0.0"));
        assert!(resp.content.contains("not in labeled dataset"));
    }

    #[tokio::test]
    async fn complete_with_no_user_messages_returns_unknown() {
        // No user messages → last_user is empty → lookup with ""
        // → miss → UNKNOWN verdict. The unwrap_or_default() path
        // in the iterator.
        let prs = vec![make_pr("a", "diff-a", Label::Slop)];
        let c = MockNimClient::new(&prs);
        let req = CompletionRequest::new("m", vec![Message::system("sys only")]);
        let resp = c.complete(req, "").await.expect("complete ok");
        assert!(resp.content.contains("UNKNOWN"));
    }

    #[tokio::test]
    async fn complete_uses_last_user_message() {
        // The mock searches .iter().rev().find() for the last
        // User message. A later User message must override an
        // earlier one.
        let prs = vec![make_pr("a", "first-diff", Label::Slop)];
        let c = MockNimClient::new(&prs);
        let req = CompletionRequest::new(
            "m",
            vec![
                Message::user("first-diff"),
                Message::assistant("ok"),
                Message::user("second-diff-not-in-dataset"),
            ],
        );
        let resp = c.complete(req, "").await.expect("complete ok");
        // The last user message is not in the dataset → UNKNOWN.
        assert!(resp.content.contains("UNKNOWN"));
    }

    #[test]
    fn estimate_tokens_sync_slop_label() {
        let prs = vec![make_pr("a", "diff-a", Label::Slop)];
        let c = MockNimClient::new(&prs);
        let usage = c.estimate_tokens_sync("system prompt", "diff-a");
        // prompt = "[system] system prompt\n[user] diff-a" → ~28 chars
        assert!(usage.prompt_tokens > 0);
        // completion is the slop JSON, ~80+ chars
        assert!(usage.completion_tokens > 0);
        assert_eq!(
            usage.total_tokens,
            usage.prompt_tokens + usage.completion_tokens
        );
    }

    #[test]
    fn estimate_tokens_sync_clean_label() {
        let prs = vec![make_pr("a", "diff-a", Label::Clean)];
        let c = MockNimClient::new(&prs);
        let usage = c.estimate_tokens_sync("sys", "diff-a");
        assert!(usage.prompt_tokens > 0);
        assert!(usage.completion_tokens > 0);
    }

    #[test]
    fn estimate_tokens_sync_unknown_diff() {
        let prs = vec![make_pr("a", "diff-a", Label::Slop)];
        let c = MockNimClient::new(&prs);
        let usage = c.estimate_tokens_sync("sys", "not-in-dataset");
        // Lookup miss → UNKNOWN verdict, same accounting shape.
        assert!(usage.prompt_tokens > 0);
        assert!(usage.completion_tokens > 0);
    }
}
