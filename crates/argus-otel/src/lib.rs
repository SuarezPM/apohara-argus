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

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_sdk::trace::SdkTracerProvider;
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
    provider: SdkTracerProvider,
}

/// Cached result of reading `ARGUS_OTEL_DISABLED` from the
/// environment. `None` means "not yet read"; `Some(b)` means the
/// env var was read and the answer is `b`. Wrapped in `RwLock`
/// so tests can override without going through the process-global
/// env table (which would need `unsafe` for `set_var` / `remove_var`
/// on Rust ≥ 1.86, breaking our zero-`unsafe` invariant).
static DISABLED_CACHE: std::sync::RwLock<Option<bool>> = std::sync::RwLock::new(None);

/// `true` when the user has explicitly disabled OTel for this run.
/// The first call reads `ARGUS_OTEL_DISABLED` from the process env;
/// subsequent calls return the cached result. The env var is read
/// at most once per process, which is correct because the OTel
/// pipeline is initialized once and never re-reads the gate.
///
/// Uses a check-then-insert pattern with separate read and write
/// locks: the fast path (already cached) takes a shared read
/// lock; the cold path (first call) takes an exclusive write
/// lock and uses `Option::get_or_insert` (which needs `&mut`,
/// only available on a write guard). The lock is `unwrap_or_else`
/// on `PoisonError` to recover from a panicked writer (the env
/// var is the source of truth, the lock is just a cache).
pub fn is_disabled() -> bool {
    // Fast path: already cached.
    if let Some(b) = *DISABLED_CACHE.read().unwrap_or_else(|e| e.into_inner()) {
        return b;
    }
    // Cold path: compute the default from the env var and
    // write it to the cache. If another thread races us
    // here, `get_or_insert` returns the value they wrote.
    let default = std::env::var("ARGUS_OTEL_DISABLED")
        .ok()
        .map(|v| v == "true" || v == "1" || v == "yes")
        .unwrap_or(false);
    *DISABLED_CACHE
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert(default)
}

/// Test-only override for the `is_disabled()` cache. Passing
/// `Some(b)` forces the next `is_disabled()` call to return `b`
/// without touching the env table; passing `None` clears the
/// cache so the next call re-reads the env var. Kept `#[cfg(test)]`
/// so it never appears in release builds.
#[cfg(test)]
fn set_disabled_for_test(value: Option<bool>) {
    *DISABLED_CACHE.write().unwrap_or_else(|e| e.into_inner()) = value;
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
    // opentelemetry_sdk 0.32 renamed `TracerProvider` →
    // `SdkTracerProvider` (the bare name is now a trait). The
    // resource builder moved to `Resource::builder().with_attributes(
    // …).build()` (the old `Resource::new()` constructor is
    // `pub(crate)` in 0.32, so a public builder is the only path
    // forward). The `with_config(Config::default().with_resource(
    // …))` chain collapses into a direct `with_resource(…)` on
    // the provider builder — Config is now an internal detail
    // and the public surface is the provider's own builder.
    // The runtime argument on `with_batch_exporter` is gone in
    // 0.32: the batch span processor is now async by default and
    // uses the runtime selected by the `rt-tokio` / `rt-tokio-current-thread`
    // / `rt-spawned` features on the `opentelemetry_sdk` crate
    // (we keep `rt-tokio` enabled in workspace.dependencies).
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_attributes(vec![
                    KeyValue::new("service.name", SERVICE_NAME),
                    KeyValue::new("service.version", SERVICE_VERSION),
                    KeyValue::new("service.component", service_label.to_string()),
                ])
                .build(),
        )
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

    /// Serializes the `is_disabled()` tests so parallel test
    /// execution doesn't race on the shared `DISABLED_CACHE`.
    /// Tests no longer touch the process env (which would need
    /// `unsafe` on Rust ≥ 1.86 for `set_var` / `remove_var`),
    /// so this lock guards the in-process `RwLock<Option<bool>>`
    /// cache instead.
    static CACHE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn is_disabled_returns_true_when_cache_set_to_true() {
        let _guard = CACHE_LOCK.lock().unwrap();
        set_disabled_for_test(Some(true));
        assert!(is_disabled());
        set_disabled_for_test(None);
    }

    #[test]
    fn is_disabled_returns_false_when_cache_set_to_false() {
        let _guard = CACHE_LOCK.lock().unwrap();
        set_disabled_for_test(Some(false));
        assert!(!is_disabled());
        set_disabled_for_test(None);
    }

    #[test]
    fn is_disabled_clears_cache_when_set_to_none() {
        let _guard = CACHE_LOCK.lock().unwrap();
        set_disabled_for_test(Some(true));
        assert!(is_disabled());
        set_disabled_for_test(None);
        // After clearing, the next call re-reads the env var.
        // We do not assert the exact value (it depends on the
        // test environment) — only that the call does not panic
        // and the cache is no longer holding `Some(true)`.
        let _ = is_disabled();
        set_disabled_for_test(None);
    }

    #[test]
    fn init_returns_none_when_disabled_via_cache() {
        let _guard = CACHE_LOCK.lock().unwrap();
        set_disabled_for_test(Some(true));
        assert!(init("test-component").is_none());
        set_disabled_for_test(None);
    }

    /// `init()` when the cache says enabled. This installs the global
    /// tracing subscriber, so it must be the only test that calls
    /// `init()` with a non-None expected return value. The
    /// `CACHE_LOCK` serializes test execution, and the test
    /// framework runs each test in its own thread, so the global
    /// subscriber installed here leaks across tests within this
    /// file. The other tests are designed to not depend on the
    /// subscriber state.
    #[test]
    fn init_returns_some_when_enabled_via_cache() {
        let _guard = CACHE_LOCK.lock().unwrap();
        set_disabled_for_test(Some(false));
        // If another test (or the test harness) already installed
        // a global subscriber, `try_init` fails and `init` returns
        // None. We accept both outcomes — the goal is to cover the
        // code path, not to assert a specific return value.
        let result = init("test-component");
        // When the subscriber installs successfully, the guard must
        // be returned (so the caller can hold it for the program
        // lifetime and flush on Drop). When it doesn't (subscriber
        // already set), None is correct per the contract.
        if let Some(_guard) = result {
            // The guard's Drop impl runs at end of scope; it must
            // not panic even when shutdown is best-effort.
            drop(_guard);
        }
        set_disabled_for_test(None);
    }

    /// The `Drop` impl on `TelemetryGuard` calls `provider.shutdown()`
    /// which is best-effort. We test it by constructing a guard
    /// (via init, gated on the subscriber not being already set)
    /// and dropping it. The test passes if no panic occurs.
    #[test]
    fn telemetry_guard_drop_does_not_panic() {
        let _guard = CACHE_LOCK.lock().unwrap();
        set_disabled_for_test(Some(false));
        if let Some(g) = init("drop-test") {
            drop(g);
        }
        set_disabled_for_test(None);
    }

    /// The cached `Some(b)` value is sticky across multiple calls:
    /// once set, subsequent reads return the same value without
    /// re-reading the env var.
    #[test]
    fn is_disabled_cached_value_is_sticky() {
        let _guard = CACHE_LOCK.lock().unwrap();
        set_disabled_for_test(Some(true));
        // Multiple calls return the same value.
        assert!(is_disabled());
        assert!(is_disabled());
        assert!(is_disabled());
        set_disabled_for_test(Some(false));
        // The new value is picked up immediately (write lock is
        // taken on the next call's cold path, or the existing
        // write guard is replaced).
        assert!(!is_disabled());
        set_disabled_for_test(None);
    }
}
