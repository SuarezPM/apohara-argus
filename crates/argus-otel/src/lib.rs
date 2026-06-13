//! OpenTelemetry init helpers. [Refs: 6.3]
//!
//! Uses the `opentelemetry-stdout` exporter so we don't need an external
//! collector. The full trace stream is written to stderr as JSON, where
//! `tracing-subscriber` already pipes logs.
//!
//! Env gating:
//! - `ARGUS_OTEL_DISABLED=true` → no-op (zero overhead, default)
//! - `ARGUS_OTEL_DISABLED=false` (or unset) → init the stdout exporter
//!
//! Why opt-in (not opt-out): on Fly.io free tier every cycle counts.
//! When a user has no collector, the JSON spans just add to log volume.

use opentelemetry::trace::Tracer;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_sdk::trace::{Config as SdkTraceConfig, TracerProvider};
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Resource attribute keys we tag every span with. Keeps grep'ing the
/// stdout JSON painless.
const SERVICE_NAME: &str = "argus";
const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returned to callers so they can shut down the provider on signal.
/// The shutdown call is best-effort — it blocks for up to 1s to flush
/// any pending spans to stdout.
pub struct TelemetryGuard {
    provider: TracerProvider,
}

/// `true` when the user has explicitly disabled OTel for this run.
pub fn is_disabled() -> bool {
    std::env::var("ARGUS_OTEL_DISABLED")
        .ok()
        .map(|v| v == "true" || v == "1" || v == "yes")
        .unwrap_or(false)
}

/// Initialize the OTel + tracing pipeline. Returns a guard the caller
/// must hold for the lifetime of the program; dropping it triggers
/// the SDK's default shutdown (best-effort flush).
///
/// Safe to call multiple times — subsequent calls are no-ops (the
/// tracing subscriber is global state and we use `try_init`).
pub fn init(service_label: &str) -> Option<TelemetryGuard> {
    if is_disabled() {
        return None;
    }

    let exporter = opentelemetry_stdout::SpanExporter::default();
    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_config(SdkTraceConfig::default().with_resource(Resource::new(vec![
            KeyValue::new("service.name", SERVICE_NAME),
            KeyValue::new("service.version", SERVICE_VERSION),
            KeyValue::new("service.component", service_label.to_string()),
        ])))
        .build();
    let tracer = provider.tracer(service_label.to_string());

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // Try to install the global subscriber. If something already
    // installed one (e.g., the binary's `tracing_subscriber::fmt()`
    // call in `main()`), we leave it alone and just return None so
    // the caller knows spans won't be exported.
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_line_number(false)
        .with_file(false);

    let result = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_layer)
        .try_init();

    if result.is_err() {
        return None;
    }

    Some(TelemetryGuard { provider })
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        // Best-effort: flush up to 1s before exit.
        let _ = self.provider.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes env-var tests so parallel test execution doesn't
    /// leak `ARGUS_OTEL_DISABLED` between tests. Without this, one
    /// test's `set_var("true")` can race with another's assertion.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn is_disabled_returns_true_when_env_var_is_true() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ARGUS_OTEL_DISABLED", "true") };
        assert!(is_disabled());
        unsafe { std::env::remove_var("ARGUS_OTEL_DISABLED") };
    }

    #[test]
    fn is_disabled_returns_false_when_env_var_is_false() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ARGUS_OTEL_DISABLED", "false") };
        assert!(!is_disabled());
        unsafe { std::env::remove_var("ARGUS_OTEL_DISABLED") };
    }

    #[test]
    fn is_disabled_returns_false_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("ARGUS_OTEL_DISABLED") };
        assert!(!is_disabled());
    }

    #[test]
    fn is_disabled_accepts_yes_and_one_aliases() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ARGUS_OTEL_DISABLED", "yes") };
        assert!(is_disabled());
        unsafe { std::env::set_var("ARGUS_OTEL_DISABLED", "1") };
        assert!(is_disabled());
        unsafe { std::env::set_var("ARGUS_OTEL_DISABLED", "no") };
        assert!(!is_disabled());
        unsafe { std::env::remove_var("ARGUS_OTEL_DISABLED") };
    }

    #[test]
    fn init_returns_none_when_disabled_via_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ARGUS_OTEL_DISABLED", "true") };
        assert!(init("test-component").is_none());
        unsafe { std::env::remove_var("ARGUS_OTEL_DISABLED") };
    }
}
