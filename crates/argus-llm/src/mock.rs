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
