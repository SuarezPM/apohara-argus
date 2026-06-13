//! Generic OpenAI-compatible LLM client.
//!
//! This is the implementation behind `NimClient` — any OpenAI-compatible
//! endpoint (NVIDIA NIM, Together, Groq, local llama.cpp with the
//! OpenAI-compatible shim, etc.) can be used by just pointing this at a
//! different base URL.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError, Message, Role, Usage};

#[derive(Debug, Clone)]
pub struct OpenAICompatClient {
    pub base_url: String,
    pub timeout: Duration,
    pub http: Client,
}

impl OpenAICompatClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("reqwest client should build");
        Self {
            base_url: base_url.into(),
            timeout: Duration::from_secs(120),
            http,
        }
    }
}

#[derive(Debug, Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<&'a [String]>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    id: Option<String>,
    model: String,
    choices: Vec<Choice>,
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    index: u32,
    message: ChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ApiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: ApiErrorBody,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
    code: Option<String>,
}

impl From<ApiRequest<'_>> for serde_json::Value {
    fn from(req: ApiRequest<'_>) -> Self {
        serde_json::to_value(&req).expect("serialize request")
    }
}

#[async_trait]
impl LlmClient for OpenAICompatClient {
    fn provider_name(&self) -> &str {
        "openai-compat"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        api_key: &str,
    ) -> Result<CompletionResponse, LlmError> {
        if api_key.is_empty() {
            return Err(LlmError::MissingKey);
        }

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let api_req = ApiRequest {
            model: &request.model,
            messages: &request.messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            top_p: request.top_p,
            stop: request.stop.as_deref(),
        };

        let body: serde_json::Value = api_req.into();

        let resp = self
            .http
            .post(&url)
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LlmError::Timeout(self.timeout)
                } else {
                    LlmError::Http(e.to_string())
                }
            })?;

        let status = resp.status();
        if !status.is_success() {
            // Capture the `Retry-After` header (seconds form) BEFORE we
            // consume the response body. We only honor it for 429/503,
            // which are the codes that actually carry a meaningful hint.
            let retry_after = if status.as_u16() == 429 || status.as_u16() == 503 {
                resp.headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(Duration::from_secs)
            } else {
                None
            };

            let text = resp.text().await.unwrap_or_default();
            if status.as_u16() == 429 || status.as_u16() == 503 {
                return Err(LlmError::RateLimited { retry_after });
            }
            if let Ok(err) = serde_json::from_str::<ApiError>(&text) {
                return Err(LlmError::Api {
                    status: status.as_u16(),
                    message: err.error.message,
                });
            }
            return Err(LlmError::Api {
                status: status.as_u16(),
                message: text,
            });
        }

        let parsed: ApiResponse = resp
            .json()
            .await
            .map_err(|e| LlmError::Parse(e.to_string()))?;

        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| LlmError::Parse("no choices in response".into()))?;

        let usage = parsed
            .usage
            .map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            })
            .unwrap_or_default();

        Ok(CompletionResponse {
            content,
            model: parsed.model,
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_correct_url() {
        let c = OpenAICompatClient::new("https://integrate.api.nvidia.com/v1/");
        let url = format!("{}/chat/completions", c.base_url.trim_end_matches('/'));
        assert_eq!(url, "https://integrate.api.nvidia.com/v1/chat/completions");
    }

    #[test]
    fn empty_key_returns_missing() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let c = OpenAICompatClient::new("https://example.com/v1");
        rt.block_on(async {
            let res = c
                .complete_one_shot("test", "sys", "user", "", 0.5, 100)
                .await;
            assert!(matches!(res, Err(LlmError::MissingKey)));
        });
    }
}
