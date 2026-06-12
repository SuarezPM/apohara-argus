//! In-memory idempotency cache for `POST /analyze`. [Refs: 6.2]
//!
//! Prevents double-billing when GitHub (or any upstream) retries a webhook:
//! if the caller supplies the same `X-Idempotency-Key` header with the
//! same `pr_url`, we return the previously-computed verdict instead of
//! running the (expensive) analysis pipeline again.
//!
//! Storage is `tokio::sync::RwLock<HashMap>` — single-process, in-memory
//! only. Entries expire after `DEFAULT_TTL` (24h). A background task in
//! `main.rs` calls `cleanup_expired()` hourly so the map cannot grow
//! unboundedly across long-lived workers.
//!
//! Design notes:
//! - Keyed by `(idempotency_key, pr_url)`. Same key + different PR =
//!   treated as a new request (cache miss), so a caller cannot reuse a
//!   key across distinct PRs by accident.
//! - Cached body is the full `AnalyzeResponse` serialised to JSON. The
//!   handler round-trips it back through `serde_json::from_value` —
//!   cheap, and avoids threading a generic cache type through Axum state.
//! - `pr_url` is the request-level identifier; the spec also refers to
//!   it as `pr_sha` in the cache API. The semantic is the same: a
//!   stable per-request discriminator the client controls.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Default TTL for cached entries. 24h covers the typical GitHub
/// webhook retry window with margin; longer retentions would need a
/// persistent store, which is a different roadmap item.
pub const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// The body stored in the cache plus the request discriminator
/// (`pr_url` in the current handler, generically called `pr_sha` in
/// the cache API).
#[derive(Clone, Serialize, Deserialize)]
pub struct CachedVerdict {
    pub body: serde_json::Value,
    pub pr_sha: String,
}

struct CacheEntry {
    verdict: CachedVerdict,
    created_at: Instant,
}

/// Thread-safe, in-memory idempotency cache. Clone is cheap — it
/// shares the inner `Arc<RwLock<…>>`.
#[derive(Clone)]
pub struct IdempotencyCache {
    map: Arc<RwLock<HashMap<String, CacheEntry>>>,
    ttl: Duration,
}

impl IdempotencyCache {
    /// New cache with the default 24h TTL.
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }

    /// New cache with a caller-supplied TTL. Used by tests for
    /// short-window expiration checks.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            map: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }

    /// Returns the cached body for `key` if it exists, has not
    /// expired, and was cached for the same `pr_sha`. Otherwise `None`.
    pub async fn get(&self, key: &str, pr_sha: &str) -> Option<serde_json::Value> {
        let map = self.map.read().await;
        if let Some(entry) = map.get(key) {
            if entry.created_at.elapsed() < self.ttl && entry.verdict.pr_sha == pr_sha {
                return Some(entry.verdict.body.clone());
            }
        }
        None
    }

    /// Stores `body` under `key`, tagged with `pr_sha`. Replaces any
    /// previous entry under the same key (last-write-wins).
    pub async fn put(&self, key: String, pr_sha: String, body: serde_json::Value) {
        let mut map = self.map.write().await;
        map.insert(
            key,
            CacheEntry {
                verdict: CachedVerdict { body, pr_sha },
                created_at: Instant::now(),
            },
        );
    }

    /// Removes all entries older than the TTL. Returns the number of
    /// entries removed (useful for tracing/logging).
    pub async fn cleanup_expired(&self) -> usize {
        let mut map = self.map.write().await;
        let initial = map.len();
        map.retain(|_, entry| entry.created_at.elapsed() < self.ttl);
        initial - map.len()
    }

    /// Number of entries currently held (including not-yet-expired
    /// and possibly some not-yet-cleaned expired ones — cleanup is
    /// opportunistic via the background task).
    pub async fn size(&self) -> usize {
        self.map.read().await.len()
    }
}

impl Default for IdempotencyCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn happy_path_put_then_get() {
        let cache = IdempotencyCache::new();
        let body = json!({ "verdict": "approved", "risk": 0.1 });

        assert!(cache.get("abc", "sha1").await.is_none());
        cache
            .put("abc".into(), "sha1".into(), body.clone())
            .await;

        let got = cache.get("abc", "sha1").await.expect("cache hit");
        assert_eq!(got, body);
    }

    #[tokio::test]
    async fn same_key_different_pr_sha_is_miss() {
        let cache = IdempotencyCache::new();
        cache
            .put("abc".into(), "sha1".into(), json!({ "v": 1 }))
            .await;

        // Same idempotency key but a different PR — must NOT return
        // the cached body. A caller reusing a key across distinct
        // PRs would otherwise get the wrong verdict.
        assert!(cache.get("abc", "sha2").await.is_none());
    }

    #[tokio::test]
    async fn expired_entry_is_miss() {
        // 1ms TTL: even with a fast clock, the entry is dead by the
        // time we sleep 10ms.
        let cache = IdempotencyCache::with_ttl(Duration::from_millis(1));
        cache
            .put("k".into(), "p".into(), json!({ "v": 1 }))
            .await;

        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(cache.get("k", "p").await.is_none());
    }

    #[tokio::test]
    async fn concurrent_puts_gets_do_not_deadlock() {
        let cache = IdempotencyCache::new();

        // 100 readers + 100 writers hitting disjoint keys, all
        // spawned together. If the RwLock poisoned or deadlocked
        // this would hang the test process past tokio's default
        // budget.
        let mut handles = Vec::with_capacity(200);
        for i in 0..100 {
            let c = cache.clone();
            handles.push(tokio::spawn(async move {
                c.put(format!("k{i}"), format!("sha{i}"), json!({ "i": i }))
                    .await;
            }));
        }
        for i in 0..100 {
            let c = cache.clone();
            handles.push(tokio::spawn(async move {
                let _ = c.get(&format!("k{i}"), &format!("sha{i}")).await;
            }));
        }

        for h in handles {
            // `expect` here would surface a JoinError as a panic
            // with the original task's panic message, which is
            // what we want if any of them deadlock.
            h.await.expect("task panicked or was cancelled");
        }

        // All 100 puts landed.
        assert_eq!(cache.size().await, 100);

        // cleanup_expired should be a no-op (none are expired at
        // 24h TTL) and must not deadlock either.
        let removed = cache.cleanup_expired().await;
        assert_eq!(removed, 0);
    }
}
