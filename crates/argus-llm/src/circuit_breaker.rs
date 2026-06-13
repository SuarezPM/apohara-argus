//! Circuit breaker for LLM client calls.
//!
//! Decorator over any [`LlmClient`]. When the upstream starts failing
//! repeatedly, the breaker opens and short-circuits calls with
//! [`CircuitError::Open`] (mapped to [`LlmError::CircuitOpen`] when used
//! through the [`LlmClient`] trait) so we don't pile on a degraded
//! endpoint. After a configured recovery window, the breaker enters
//! `HalfOpen`, allowing probe calls. A single failure in `HalfOpen`
//! snaps it back to `Open`; a single success closes it.
//!
//! Refs: supremum-roadmap §3.1.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex;

use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError};

/// Breaker state. Transitions are driven exclusively by
/// [`LlmCircuitBreaker::call`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal: calls flow through.
    Closed,
    /// Calls are short-circuited.
    Open,
    /// Probe state: a single call is allowed through to test recovery.
    HalfOpen,
}

/// Tunables for the breaker.
#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerConfig {
    /// Consecutive failures in `Closed` that trip the breaker to `Open`.
    pub failure_threshold: u32,
    /// How long to stay `Open` before allowing a `HalfOpen` probe.
    pub recovery_timeout: Duration,
    /// Max concurrent probes in `HalfOpen` (we use 1 for now).
    pub half_open_max_probes: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            half_open_max_probes: 1,
        }
    }
}

/// Errors surfaced by the breaker.
#[derive(Debug, Error)]
pub enum CircuitError {
    /// The breaker is open — the inner client was not called.
    #[error("Circuit breaker is open")]
    Open,
    /// The inner client was called and returned this error.
    #[error("Inner LLM call failed: {0}")]
    Inner(LlmError),
}

impl From<CircuitError> for LlmError {
    fn from(e: CircuitError) -> Self {
        match e {
            CircuitError::Open => LlmError::CircuitOpen,
            CircuitError::Inner(e) => e,
        }
    }
}

/// Circuit-breaker decorator wrapping any [`LlmClient`].
pub struct LlmCircuitBreaker<L: LlmClient> {
    inner: L,
    config: CircuitBreakerConfig,
    state: Arc<Mutex<CircuitState>>,
    consecutive_failures: Arc<Mutex<u32>>,
    last_failure: Arc<Mutex<Option<Instant>>>,
}

impl<L: LlmClient> LlmCircuitBreaker<L> {
    pub fn new(inner: L, config: CircuitBreakerConfig) -> Self {
        Self {
            inner,
            config,
            state: Arc::new(Mutex::new(CircuitState::Closed)),
            consecutive_failures: Arc::new(Mutex::new(0)),
            last_failure: Arc::new(Mutex::new(None)),
        }
    }

    /// Snapshot the current state. Useful for tests and metrics.
    pub async fn state(&self) -> CircuitState {
        *self.state.lock().await
    }

    /// Call the inner client through the breaker.
    pub async fn call(
        &self,
        request: CompletionRequest,
        api_key: &str,
    ) -> Result<CompletionResponse, CircuitError> {
        // 1. Check state. If Open and recovery_timeout has elapsed,
        //    transition to HalfOpen. Otherwise short-circuit.
        {
            let mut state = self.state.lock().await;
            if *state == CircuitState::Open {
                let last = *self.last_failure.lock().await;
                let elapsed = last.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
                if elapsed >= self.config.recovery_timeout {
                    *state = CircuitState::HalfOpen;
                } else {
                    return Err(CircuitError::Open);
                }
            }
        }

        // 2. Delegate to the inner client.
        let result = self.inner.complete(request, api_key).await;

        // 3. Update state based on the result.
        match result {
            Ok(resp) => {
                self.record_success().await;
                Ok(resp)
            }
            Err(e) => {
                self.record_failure().await;
                Err(CircuitError::Inner(e))
            }
        }
    }

    async fn record_success(&self) {
        let mut state = self.state.lock().await;
        let mut failures = self.consecutive_failures.lock().await;
        *failures = 0;
        *state = CircuitState::Closed;
    }

    async fn record_failure(&self) {
        let mut state = self.state.lock().await;
        let mut failures = self.consecutive_failures.lock().await;
        let mut last = self.last_failure.lock().await;
        *failures += 1;
        *last = Some(Instant::now());

        match *state {
            CircuitState::Closed => {
                if *failures >= self.config.failure_threshold {
                    *state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                *state = CircuitState::Open;
            }
            CircuitState::Open => {
                // Already open.
            }
        }
    }
}

#[async_trait]
impl<L: LlmClient + Send + Sync> LlmClient for LlmCircuitBreaker<L> {
    fn provider_name(&self) -> &str {
        self.inner.provider_name()
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        api_key: &str,
    ) -> Result<CompletionResponse, LlmError> {
        self.call(request, api_key).await.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::ScriptedMock;
    use crate::{CompletionRequest, Message, Usage};

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

    fn req() -> CompletionRequest {
        CompletionRequest::new("test-model", vec![Message::user("hi")])
    }

    /// 5 failures → Open. After recovery_timeout → HalfOpen. 1 success
    /// → Closed. After the probe, failures are reset to 0, so the
    /// breaker needs 5 more consecutive failures to re-open.
    #[tokio::test]
    async fn test_breaker_state_cycle() {
        let cfg = CircuitBreakerConfig {
            failure_threshold: 5,
            recovery_timeout: Duration::from_millis(50),
            half_open_max_probes: 1,
        };
        let script = vec![
            Err(err_500()),
            Err(err_500()),
            Err(err_500()),
            Err(err_500()),
            Err(err_500()),
            Ok(ok_response()),
            Err(err_500()),
            Err(err_500()),
            Err(err_500()),
            Err(err_500()),
            Err(err_500()),
        ];
        let mock = ScriptedMock::new(script);
        let breaker = LlmCircuitBreaker::new(mock.clone(), cfg);

        for i in 0..5 {
            let r = breaker.call(req(), "k").await;
            assert!(
                matches!(r, Err(CircuitError::Inner(_))),
                "call {} should be Inner",
                i
            );
        }
        assert_eq!(breaker.state().await, CircuitState::Open);

        let r = breaker.call(req(), "k").await;
        assert!(matches!(r, Err(CircuitError::Open)));
        assert_eq!(mock.call_count(), 5, "inner must not be called while open");

        tokio::time::sleep(Duration::from_millis(60)).await;
        let r = breaker.call(req(), "k").await;
        assert!(r.is_ok(), "probe in HalfOpen should pass through: {:?}", r);
        assert_eq!(breaker.state().await, CircuitState::Closed);
        assert_eq!(*breaker.consecutive_failures.lock().await, 0);

        for _ in 0..4 {
            let r = breaker.call(req(), "k").await;
            assert!(matches!(r, Err(CircuitError::Inner(_))));
            assert_eq!(breaker.state().await, CircuitState::Closed);
        }
        let r = breaker.call(req(), "k").await;
        assert!(matches!(r, Err(CircuitError::Inner(_))));
        assert_eq!(breaker.state().await, CircuitState::Open);
    }

    /// When the breaker is Open, the inner client is NOT called.
    #[tokio::test]
    async fn test_breaker_open_does_not_call_inner() {
        let cfg = CircuitBreakerConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_secs(60),
            half_open_max_probes: 1,
        };
        let mock = ScriptedMock::new(vec![Err(err_500())]);
        let breaker = LlmCircuitBreaker::new(mock.clone(), cfg);

        breaker.call(req(), "k").await.ok();
        breaker.call(req(), "k").await.ok();
        assert_eq!(breaker.state().await, CircuitState::Open);
        let calls_at_trip = mock.call_count();
        assert_eq!(calls_at_trip, 2);

        for _ in 0..5 {
            let r = breaker.call(req(), "k").await;
            assert!(matches!(r, Err(CircuitError::Open)));
        }
        assert_eq!(
            mock.call_count(),
            calls_at_trip,
            "inner must not be called while open"
        );
    }

    /// 100 parallel calls under Closed all succeed.
    #[tokio::test]
    async fn test_concurrent_breaker_state() {
        let cfg = CircuitBreakerConfig {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            half_open_max_probes: 1,
        };
        let mock = ScriptedMock::new(vec![Ok(ok_response())]);
        let breaker = std::sync::Arc::new(LlmCircuitBreaker::new(mock.clone(), cfg));

        let mut handles = Vec::new();
        for _ in 0..100 {
            let b = breaker.clone();
            handles.push(tokio::spawn(async move { b.call(req(), "k").await }));
        }
        for h in handles {
            let r = h.await.unwrap();
            assert!(r.is_ok());
        }
        assert_eq!(mock.call_count(), 100);
        assert_eq!(breaker.state().await, CircuitState::Closed);
        assert_eq!(*breaker.consecutive_failures.lock().await, 0);
    }

    /// The LlmClient impl on the breaker maps `CircuitError::Open` →
    /// `LlmError::CircuitOpen` and `CircuitError::Inner(e)` → `e`.
    #[tokio::test]
    async fn test_breaker_lmclient_impl_maps_errors() {
        let cfg = CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            half_open_max_probes: 1,
        };
        let mock = ScriptedMock::new(vec![Err(err_500())]);
        let breaker = LlmCircuitBreaker::new(mock.clone(), cfg);

        let r = breaker.complete(req(), "k").await;
        assert!(matches!(r, Err(LlmError::Api { status: 500, .. })));

        let r = breaker.complete(req(), "k").await;
        assert!(matches!(r, Err(LlmError::CircuitOpen)));
    }
}
