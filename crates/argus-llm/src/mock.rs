//! Mock LLM client for tests and dev-without-key mode.
//!
//! Returns deterministic responses based on the prompt content. Used in
//! integration tests and as a fallback when `ARGUS_MOCK_LLM=1` is set.

use async_trait::async_trait;

use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError, Usage};

pub struct MockClient;

#[async_trait]
impl LlmClient for MockClient {
    fn provider_name(&self) -> &str {
        "mock"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        _api_key: &str,
    ) -> Result<CompletionResponse, LlmError> {
        // Combine the last user message into a deterministic mock response.
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, super::Role::User))
            .map(|m| m.content.clone())
            .unwrap_or_default();

        // Very simple heuristics so the mock "does something useful".
        let content = if last_user.contains("AWS") || last_user.contains("sk-") {
            r#"{"highest_severity":"CRITICAL","findings":[{"severity":"CRITICAL","category":"hardcoded_secret","description":"AWS key detected in diff"}],"summary":"Mock: hardcoded secret detected."}"#.to_string()
        } else if last_user.to_lowercase().contains("foo bar baz") {
            r#"{"slop_score":0.92,"signals_detected":["verbose_comments","generic_names"],"confidence":0.8,"reasoning":"Mock: clearly verbose slop."}"#.to_string()
        } else {
            r#"{"verdict":"REVIEW_REQUIRED","risk_score":0.3,"summary":"Mock verdict","key_findings":["No critical issues"],"action_items":[],"reasoning":"Mock response"}"#.to_string()
        };

        Ok(CompletionResponse {
            content,
            model: "mock".to_string(),
            usage: Usage {
                prompt_tokens: last_user.len() as u32 / 4,
                completion_tokens: 80,
                total_tokens: last_user.len() as u32 / 4 + 80,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    //! The MockClient is used by `tests/pipeline_e2e.rs` and as
    //! a fallback when `ARGUS_MOCK_LLM=1` is set. These tests pin
    //! the deterministic heuristic contract — the 3 branches
    //! (AWS key / slop pattern / default) must return the
    //! expected canned JSON for downstream consumers to parse.

    use super::*;
    use crate::Message;

    #[test]
    fn provider_name_is_mock() {
        assert_eq!(MockClient.provider_name(), "mock");
    }

    #[test]
    fn complete_with_aws_key_returns_critical_security_finding() {
        let req = CompletionRequest::new(
            "m",
            vec![Message::user("+ AWS_ACCESS_KEY_ID=AKIA1234567890ABCDEF")],
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(MockClient.complete(req, "")).unwrap();
        // The AWS-key branch returns a security report with
        // highest_severity=CRITICAL. The mock does not depend on
        // the request model — we just verify the heuristic
        // triggered.
        assert!(resp.content.contains("CRITICAL"));
        assert!(resp.content.contains("hardcoded_secret"));
        assert!(resp.content.contains("AWS key"));
        assert_eq!(resp.model, "mock");
        // The prompt_tokens field is `last_user.len() / 4`. We
        // don't pin the exact value (the test prompt's length
        // would make this fragile to copy-paste edits); we just
        // verify it's > 0 and that total = prompt + completion.
        assert!(resp.usage.prompt_tokens > 0);
        assert_eq!(resp.usage.completion_tokens, 80);
        assert_eq!(
            resp.usage.total_tokens,
            resp.usage.prompt_tokens + resp.usage.completion_tokens
        );
    }

    #[test]
    fn complete_with_sk_prefix_returns_critical_security_finding() {
        // The mock also triggers on "sk-" (OpenAI key prefix).
        // This is a different detection path from the AWS branch
        // but produces the same canned response.
        let req = CompletionRequest::new(
            "m",
            vec![Message::user("+ OPENAI_API_KEY=sk-abcdef1234567890")],
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(MockClient.complete(req, "")).unwrap();
        assert!(resp.content.contains("CRITICAL"));
    }

    #[test]
    fn complete_with_foo_bar_baz_returns_slop_finding() {
        // The slop-pattern branch fires on the substring
        // "foo bar baz" (case-insensitive). It returns a slop
        // report with score 0.92 and verbose_comments signal.
        let req = CompletionRequest::new(
            "m",
            vec![Message::user("+ // foo bar baz is a common test phrase")],
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(MockClient.complete(req, "")).unwrap();
        assert!(resp.content.contains("slop_score"));
        assert!(resp.content.contains("verbose_comments"));
        assert!(resp.content.contains("0.92"));
    }

    #[test]
    fn complete_with_foo_bar_baz_uppercase_returns_slop_finding() {
        // The slop pattern is case-insensitive (the mock calls
        // to_lowercase() before matching).
        let req = CompletionRequest::new("m", vec![Message::user("+ // FOO BAR BAZ shouting")]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(MockClient.complete(req, "")).unwrap();
        assert!(resp.content.contains("slop_score"));
    }

    #[test]
    fn complete_with_default_content_returns_review_required_verdict() {
        // The default branch (no AWS, no "foo bar baz") returns
        // a verdict report with status REVIEW_REQUIRED and
        // risk_score 0.3. This is the "nothing special detected"
        // path.
        let req = CompletionRequest::new("m", vec![Message::user("+ let x = 1;")]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(MockClient.complete(req, "")).unwrap();
        assert!(resp.content.contains("REVIEW_REQUIRED"));
        assert!(resp.content.contains("0.3"));
        assert!(resp.content.contains("Mock verdict"));
    }

    #[test]
    fn complete_with_no_user_messages_returns_default_verdict() {
        // If the request has no User messages (only System, or
        // empty), the mock falls back to the default branch.
        // This is the `unwrap_or_default()` path in the iterator.
        let req = CompletionRequest::new("m", vec![Message::system("sys only")]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(MockClient.complete(req, "")).unwrap();
        // No user content → empty last_user → default branch.
        assert!(resp.content.contains("REVIEW_REQUIRED"));
        // Usage: prompt_tokens = last_user.len() / 4 = 0/4 = 0
        assert_eq!(resp.usage.prompt_tokens, 0);
        assert_eq!(resp.usage.completion_tokens, 80);
        assert_eq!(resp.usage.total_tokens, 80);
    }

    #[test]
    fn complete_uses_last_user_message_when_multiple() {
        // The mock searches `.iter().rev().find()` for the last
        // User message in the conversation. An earlier User
        // message with "AWS" must NOT trigger the critical
        // branch if a later User message doesn't contain it.
        let req = CompletionRequest::new(
            "m",
            vec![
                Message::user("first message with AWS key"),
                Message::assistant("ok"),
                Message::user("second message without secrets"),
            ],
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(MockClient.complete(req, "")).unwrap();
        // The last user message has no AWS/sk-/foo bar baz →
        // default branch.
        assert!(resp.content.contains("REVIEW_REQUIRED"));
        assert!(!resp.content.contains("CRITICAL"));
    }

    #[test]
    fn complete_ignores_assistant_messages() {
        // Assistant messages must not be considered for the
        // heuristic. Only the last User message matters.
        let req = CompletionRequest::new(
            "m",
            vec![
                Message::assistant("AWS key in here, but I'm the assistant"),
                Message::user("plain user text"),
            ],
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(MockClient.complete(req, "")).unwrap();
        assert!(resp.content.contains("REVIEW_REQUIRED"));
    }
}
