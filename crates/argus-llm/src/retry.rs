//! Retry decorator for LLM client calls.
//!
//! Wraps any [`LlmClient`] and retries on transient errors (see
//! [`LlmError::is_retryable`]) using full-jitter exponential backoff.
//!
//! Full-jitter (per AWS architecture blog): `sleep = rand(0, min(max,
//! initial * 2^attempt))`. Spreads retry storms across callers better
//! than fixed or equal-jitter backoff and avoids the synchronization
//! problem where all clients retry at the same instant after a hiccup.
//!
//! Refs: supremum-roadmap §3.1.
//!
//! Kept in-house (≈80 LOC) on purpose: the `llm-retry` crate is v0.1.0
//! with only 14 downloads (per W0-1 audit, May 2026) — too immature
//! for a resilience-critical path.

use async_trait::async_trait;
use rand::Rng;
use std::time::Duration;

use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError};

/// Tunables for the retry decorator.
#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    /// Max retry attempts AFTER the initial call. `max_retries = 3`
    /// means up to 4 total attempts.
    pub max_retries: u32,
    /// First backoff. Doubles each attempt, capped at `max_backoff`.
    pub initial_backoff: Duration,
    /// Cap on the backoff window.
    pub max_backoff: Duration,
    /// If true (default), use full-jitter: `rand(0, exp_window)`.
    /// If false, sleep the full exponential window.
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            jitter: true,
        }
    }
}

/// Retry decorator. Holds the inner client and a [`RetryConfig`].
pub struct RetryClient<L: LlmClient> {
    inner: L,
    config: RetryConfig,
}

impl<L: LlmClient> RetryClient<L> {
    pub fn new(inner: L, config: RetryConfig) -> Self {
        Self { inner, config }
    }

    pub fn inner(&self) -> &L {
        &self.inner
    }

    /// Compute the backoff for `attempt` (0-based).
    ///
    /// With jitter: `rand(0, min(max_backoff, initial * 2^attempt))`.
    /// Without jitter: `min(max_backoff, initial * 2^attempt)`.
    fn backoff_for(&self, attempt: u32) -> Duration {
        // Cap the exponent at 32 to avoid overflow on u32 doubling.
        let exp = attempt.min(32);
        let window = self
            .config
            .initial_backoff
            .saturating_mul(1u32 << exp)
            .min(self.config.max_backoff);
        if self.config.jitter {
            if window.is_zero() {
                return Duration::ZERO;
            }
            let nanos = window.as_nanos() as u64;
            let jittered = rand::thread_rng().gen_range(0..nanos.max(1));
            Duration::from_nanos(jittered)
        } else {
            window
        }
    }
}

#[async_trait]
impl<L: LlmClient + Send + Sync> LlmClient for RetryClient<L> {
    fn provider_name(&self) -> &str {
        self.inner.provider_name()
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        api_key: &str,
    ) -> Result<CompletionResponse, LlmError> {
        let mut last_err: Option<LlmError> = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.complete(request.clone(), api_key).await {
                Ok(r) => return Ok(r),
                Err(e) if e.is_retryable() => {
                    if attempt < self.config.max_retries {
                        let backoff = self.backoff_for(attempt);
                        tokio::time::sleep(backoff).await;
                    }
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.expect("loop runs at least once"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::ScriptedMock;
    use crate::{CompletionRequest, Message, Usage};
    use std::time::Instant;

    fn ok_response() -> CompletionResponse {
        CompletionResponse {
            content: "ok".into(),
            model: "test".into(),
            usage: Usage::default(),
        }
    }

    fn err_500() -> LlmError {
        LlmError::Api {
            status: 500,
            message: "boom".into(),
        }
    }

    fn err_400() -> LlmError {
        LlmError::Api {
            status: 400,
            message: "bad".into(),
        }
    }

    fn err_timeout() -> LlmError {
        LlmError::Timeout(Duration::from_secs(1))
    }

    fn req() -> CompletionRequest {
        CompletionRequest::new("test-model", vec![Message::user("hi")])
    }

    /// Three transient (500) failures, then success on the 4th attempt.
    /// Inner is called 4 times.
    #[tokio::test]
    async fn test_retry_exponential_backoff_eventual_success() {
        let cfg = RetryConfig {
            max_retries: 3,
            initial_backoff: Duration::from_millis(20),
            max_backoff: Duration::from_millis(200),
            jitter: true,
        };
        let script = vec![
            Err(err_500()),
            Err(err_500()),
            Err(err_500()),
            Ok(ok_response()),
        ];
        let mock = ScriptedMock::new(script);
        let client = RetryClient::new(mock.clone(), cfg);

        let start = Instant::now();
        let r = client.complete(req(), "k").await;
        let elapsed = start.elapsed();
        assert!(r.is_ok(), "4th attempt should succeed: {:?}", r);
        assert_eq!(mock.call_count(), 4);
        assert!(elapsed >= Duration::ZERO, "elapsed was negative: {:?}", elapsed);
    }

    /// When jitter is OFF, the sleep is exactly the exponential window.
    /// We can assert an exact lower bound on elapsed time.
    #[tokio::test]
    async fn test_retry_no_jitter_exact_bounds() {
        let cfg = RetryConfig {
            max_retries: 2,
            initial_backoff: Duration::from_millis(20),
            max_backoff: Duration::from_millis(200),
            jitter: false,
        };
        // 3 attempts, 2 retries. Backoffs: 20ms, 40ms. Sum = 60ms.
        let script = vec![Err(err_500()), Err(err_500()), Err(err_500())];
        let mock = ScriptedMock::new(script);
        let client = RetryClient::new(mock.clone(), cfg);

        let start = Instant::now();
        let r = client.complete(req(), "k").await;
        let elapsed = start.elapsed();
        assert!(r.is_err());
        assert_eq!(mock.call_count(), 3);
        assert!(
            elapsed >= Duration::from_millis(60),
            "elapsed {:?} should be >= 60ms (sum of 20+40)",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "elapsed {:?} should be < 500ms (no scheduler hitch)",
            elapsed
        );
    }

    /// 400 is NOT retryable. call_count == 1.
    #[tokio::test]
    async fn test_no_retry_on_4xx() {
        let cfg = RetryConfig {
            max_retries: 3,
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(100),
            jitter: false,
        };
        let mock = ScriptedMock::new(vec![Err(err_400())]);
        let client = RetryClient::new(mock.clone(), cfg);

        let r = client.complete(req(), "k").await;
        assert!(matches!(r, Err(LlmError::Api { status: 400, .. })));
        assert_eq!(mock.call_count(), 1, "4xx must not be retried");
    }

    /// A `MissingKey` error is a caller error — never retried.
    #[tokio::test]
    async fn test_no_retry_on_missing_key() {
        let cfg = RetryConfig::default();
        let mock = ScriptedMock::new(vec![Err(LlmError::MissingKey)]);
        let client = RetryClient::new(mock.clone(), cfg);

        let r = client.complete(req(), "k").await;
        assert!(matches!(r, Err(LlmError::MissingKey)));
        assert_eq!(mock.call_count(), 1);
    }

    /// `CircuitOpen` is semantically NOT retryable (don't hammer a
    /// breaker that's protecting an unhealthy upstream).
    #[tokio::test]
    async fn test_no_retry_on_circuit_open() {
        let cfg = RetryConfig::default();
        let mock = ScriptedMock::new(vec![Err(LlmError::CircuitOpen)]);
        let client = RetryClient::new(mock.clone(), cfg);

        let r = client.complete(req(), "k").await;
        assert!(matches!(r, Err(LlmError::CircuitOpen)));
        assert_eq!(mock.call_count(), 1);
    }

    /// A `Timeout` IS retryable.
    #[tokio::test]
    async fn test_retry_on_timeout() {
        let cfg = RetryConfig {
            max_retries: 2,
            initial_backoff: Duration::from_millis(5),
            max_backoff: Duration::from_millis(50),
            jitter: false,
        };
        let script = vec![Err(err_timeout()), Ok(ok_response())];
        let mock = ScriptedMock::new(script);
        let client = RetryClient::new(mock.clone(), cfg);

        let r = client.complete(req(), "k").await;
        assert!(r.is_ok());
        assert_eq!(mock.call_count(), 2);
    }

    /// `RetryClient` delegates `provider_name` to the inner.
    #[tokio::test]
    async fn test_provider_name_delegates() {
        let mock = ScriptedMock::new(vec![Ok(ok_response())]);
        let client = RetryClient::new(mock, RetryConfig::default());
        assert_eq!(client.provider_name(), "scripted");
    }

    /// Sanity: `backoff_for` windows are monotonic and capped.
    #[tokio::test]
    async fn test_backoff_for_windows() {
        let cfg = RetryConfig {
            max_retries: 5,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(1),
            jitter: false,
        };
        let mock = ScriptedMock::new(vec![Ok(ok_response())]);
        let client = RetryClient::new(mock, cfg);
        assert_eq!(client.backoff_for(0), Duration::from_millis(100));
        assert_eq!(client.backoff_for(1), Duration::from_millis(200));
        assert_eq!(client.backoff_for(2), Duration::from_millis(400));
        assert_eq!(client.backoff_for(3), Duration::from_millis(800));
        // Capped at max_backoff (1s).
        assert_eq!(client.backoff_for(4), Duration::from_secs(1));
        assert_eq!(client.backoff_for(5), Duration::from_secs(1));
    }
}
