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

use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError, Message, Usage};

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
    // Kept for OpenAI response compatibility; not consumed by the
    // completion path but required for future observability/debug hooks.
    #[allow(dead_code)]
    id: Option<String>,
    model: String,
    choices: Vec<Choice>,
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    // `index` and `finish_reason` are part of the OpenAI Chat Completions
    // response contract; we do not currently branch on them, but the
    // deserializer needs the fields present to avoid schema drift.
    #[allow(dead_code)]
    index: u32,
    message: ChoiceMessage,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    // `role` is always `"assistant"` for chat completions; not read by the
    // completion path but kept for OpenAI compatibility and future audit
    // hooks (e.g. confirming the role matches what we sent).
    #[allow(dead_code)]
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
    // `error_type` and `code` are part of the OpenAI error envelope. We
    // surface only `message` today; keeping the other fields avoids
    // rejecting responses when providers extend the schema.
    #[allow(dead_code)]
    #[serde(rename = "type")]
    error_type: Option<String>,
    #[allow(dead_code)]
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Spin up a one-shot mock HTTP server on 127.0.0.1:0 (random
    /// port). Returns the base URL to point a client at. The server
    /// accepts ONE connection, reads the request, writes the canned
    /// `status` + `body` response, then exits. This is enough for
    /// the `complete()` path which makes a single POST and reads
    /// the response. Loops for retries from the client's retry
    /// logic if any.
    async fn spawn_mock(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = match listener.accept().await {
                    Ok(c) => c,
                    Err(_) => break,
                };
                // Read up to 16 KiB of the request (discard it).
                let mut buf = vec![0u8; 16384];
                let _ = stream.read(&mut buf).await;
                // Write the canned response.
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
                    status = status,
                    len = body.len(),
                    body = body,
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        format!("http://{}/v1", addr)
    }

    /// Standard OpenAI-compatible success response with the
    /// assistant content we want to assert on.
    const OK_BODY: &str = r#"{"id":"x","model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}"#;

    /// OpenAI-compatible error envelope (used by all 4xx/5xx paths).
    const ERR_BODY: &str =
        r#"{"error":{"message":"bad key","type":"invalid_request_error","code":"x"}}"#;

    #[test]
    fn builds_correct_url() {
        let c = OpenAICompatClient::new("https://integrate.api.nvidia.com/v1/");
        let url = format!("{}/chat/completions", c.base_url.trim_end_matches('/'));
        assert_eq!(url, "https://integrate.api.nvidia.com/v1/chat/completions");
    }

    #[test]
    fn new_client_uses_default_timeout() {
        let c = OpenAICompatClient::new("https://x/v1");
        assert_eq!(c.timeout, Duration::from_secs(120));
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

    #[tokio::test]
    async fn complete_200_returns_content_and_usage() {
        let url = spawn_mock(200, OK_BODY).await;
        let c = OpenAICompatClient::new(url);
        let resp = c
            .complete_one_shot("test-model", "sys", "user", "k", 0.5, 100)
            .await
            .unwrap();
        assert_eq!(resp.content, "hello");
        assert_eq!(resp.model, "test-model");
        assert_eq!(resp.usage.prompt_tokens, 5);
        assert_eq!(resp.usage.completion_tokens, 3);
        assert_eq!(resp.usage.total_tokens, 8);
    }

    #[tokio::test]
    async fn complete_429_returns_rate_limited() {
        let url = spawn_mock(429, ERR_BODY).await;
        let c = OpenAICompatClient::new(url);
        let res = c.complete_one_shot("m", "s", "u", "k", 0.5, 100).await;
        assert!(matches!(res, Err(LlmError::RateLimited { .. })));
    }

    #[tokio::test]
    async fn complete_503_returns_rate_limited() {
        let url = spawn_mock(503, ERR_BODY).await;
        let c = OpenAICompatClient::new(url);
        let res = c.complete_one_shot("m", "s", "u", "k", 0.5, 100).await;
        assert!(matches!(res, Err(LlmError::RateLimited { .. })));
    }

    #[tokio::test]
    async fn complete_500_returns_api_error_with_message() {
        let url = spawn_mock(500, ERR_BODY).await;
        let c = OpenAICompatClient::new(url);
        let res = c.complete_one_shot("m", "s", "u", "k", 0.5, 100).await;
        match res {
            Err(LlmError::Api { status, message }) => {
                assert_eq!(status, 500);
                assert_eq!(message, "bad key");
            }
            other => panic!("expected LlmError::Api, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn complete_500_non_json_body_returns_api_error_with_text() {
        let url = spawn_mock(500, "internal server error").await;
        let c = OpenAICompatClient::new(url);
        let res = c.complete_one_shot("m", "s", "u", "k", 0.5, 100).await;
        match res {
            Err(LlmError::Api { status, message }) => {
                assert_eq!(status, 500);
                assert_eq!(message, "internal server error");
            }
            other => panic!("expected LlmError::Api, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn complete_200_no_choices_returns_parse_error() {
        let body = r#"{"id":"x","model":"m","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
        let url = spawn_mock(200, body).await;
        let c = OpenAICompatClient::new(url);
        let res = c.complete_one_shot("m", "s", "u", "k", 0.5, 100).await;
        assert!(matches!(res, Err(LlmError::Parse(_))));
    }

    #[tokio::test]
    async fn complete_200_no_usage_uses_default() {
        let body = r#"{"id":"x","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"x"},"finish_reason":"stop"}]}"#;
        let url = spawn_mock(200, body).await;
        let c = OpenAICompatClient::new(url);
        let resp = c
            .complete_one_shot("m", "s", "u", "k", 0.5, 100)
            .await
            .unwrap();
        // Default Usage is all zeros when the provider omits it.
        assert_eq!(resp.usage.prompt_tokens, 0);
        assert_eq!(resp.usage.completion_tokens, 0);
        assert_eq!(resp.usage.total_tokens, 0);
    }

    #[tokio::test]
    async fn complete_unreachable_returns_http_error() {
        // Port 1 is reserved and almost never bound; reqwest should
        // fail with a connection error, not a timeout.
        let c = OpenAICompatClient::new("http://127.0.0.1:1/v1");
        let res = c.complete_one_shot("m", "s", "u", "k", 0.5, 100).await;
        assert!(matches!(res, Err(LlmError::Http(_))));
    }

    #[tokio::test]
    async fn complete_invalid_json_returns_parse_error() {
        let url = spawn_mock(200, "not json {").await;
        let c = OpenAICompatClient::new(url);
        let res = c.complete_one_shot("m", "s", "u", "k", 0.5, 100).await;
        assert!(matches!(res, Err(LlmError::Parse(_))));
    }

    #[tokio::test]
    async fn provider_name_is_openai_compat() {
        let c = OpenAICompatClient::new("http://x/v1");
        assert_eq!(c.provider_name(), "openai-compat");
    }
}
