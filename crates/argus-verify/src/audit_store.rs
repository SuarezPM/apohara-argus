//! In-memory audit-event store backing the NDJSON export endpoint.
//! [Refs: 2.2]
//!
//! Holds a `Vec<AuditEvent>` behind a `tokio::sync::RwLock` so concurrent
//! analyze() calls can append without blocking readers, while still
//! giving the export route a consistent snapshot when it computes the
//! manifest.
//!
//! GDPR: only `AuditEvent` (which carries BLAKE3 fingerprints, not
//! cleartext) is stored. The store is process-local; for cross-process
//! durability and retention, see item 6.4 (SQLite ledger, roadmap §6.4).

use std::sync::Arc;

use argus_core::types::{AuditEvent, Manifest};
use blake3::Hasher;
use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
use tokio::sync::RwLock;

/// Process-local audit log. Cheap to clone — the inner `Arc` is
/// shared, the `RwLock` synchronizes concurrent appends and reads.
#[derive(Clone, Default)]
pub struct InMemoryAuditStore {
    events: Arc<RwLock<Vec<AuditEvent>>>,
}

impl InMemoryAuditStore {
    /// Empty store. Cheap; allocate the inner vec on first append.
    pub fn new() -> Self {
        Self { events: Arc::new(RwLock::new(Vec::new())) }
    }

    /// Append a single event.
    pub async fn append(&self, event: AuditEvent) {
        self.events.write().await.push(event);
    }

    /// Return all events whose `timestamp` is in `[from, to]`
    /// (inclusive on both ends), in arrival order.
    pub async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Vec<AuditEvent> {
        self.events
            .read()
            .await
            .iter()
            .filter(|e| e.timestamp >= from && e.timestamp <= to)
            .cloned()
            .collect()
    }

    /// Number of events currently held.
    pub async fn len(&self) -> usize {
        self.events.read().await.len()
    }

    /// Is the store empty?
    pub async fn is_empty(&self) -> bool {
        self.events.read().await.is_empty()
    }

    /// Compute the `Manifest` footer for a set of events. The BLAKE3
    /// hash is over the canonical JSON of each event in arrival order
    /// with the per-event `signature` field zeroed. When `events` is
    /// empty, `b3_hash` is the empty string.
    pub async fn manifest_for(&self, events: &[AuditEvent]) -> Manifest {
        let mut hasher = Hasher::new();
        for e in events {
            let mut e_no_sig = e.clone();
            e_no_sig.signature = Signature::from_bytes(&[0u8; 64]);
            let bytes = serde_json::to_vec(&e_no_sig)
                .expect("AuditEvent must serialize to JSON");
            hasher.update(&bytes);
        }
        let b3_hash = if events.is_empty() {
            String::new()
        } else {
            hasher.finalize().to_hex().to_string()
        };
        Manifest {
            count: events.len() as u32,
            first_at: events.first().map(|e| e.timestamp),
            last_at: events.last().map(|e| e.timestamp),
            b3_hash,
            generated_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_core::{AuditEvent, DecisionArtifact};
    use chrono::{TimeZone, Utc};
    use ed25519_dalek::{Signer, SigningKey};
    use uuid::Uuid;

    fn sample_event(ts: DateTime<Utc>, model: &str, prompt: &[u8; 32]) -> AuditEvent {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let mut ev = AuditEvent {
            audit_id: Uuid::new_v4(),
            timestamp: ts,
            model_id: model.into(),
            prompt_template_version: "v1".into(),
            prompt_fingerprint: *prompt,
            response_fingerprint: [0u8; 32],
            temperature: 0.7,
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            estimated_cost_usd: 0.0,
            decision: DecisionArtifact {
                verdict: "warn".into(),
                findings_count: 1,
                rationale: "test".into(),
            },
            prev_hash: [0u8; 32],
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
        };
        let mut canonical = ev.clone();
        canonical.signature = ed25519_dalek::Signature::from_bytes(&[0u8; 64]);
        let bytes = serde_json::to_vec(&canonical).unwrap();
        ev.signature = key.sign(&bytes);
        ev
    }

    fn fp(i: u8) -> [u8; 32] {
        let mut a = [0u8; 32];
        a[0] = i;
        a
    }

    fn ts(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn append_three_events_query_returns_all_three() {
        let store = InMemoryAuditStore::new();
        let e1 = sample_event(ts(2026, 6, 12, 10), "m1", &fp(1));
        let e2 = sample_event(ts(2026, 6, 12, 11), "m2", &fp(2));
        let e3 = sample_event(ts(2026, 6, 12, 12), "m3", &fp(3));

        store.append(e1.clone()).await;
        store.append(e2.clone()).await;
        store.append(e3.clone()).await;

        let range = store
            .query_range(ts(2026, 1, 1, 0), ts(2027, 1, 1, 0))
            .await;
        assert_eq!(range.len(), 3);
        assert_eq!(range[0].audit_id, e1.audit_id);
        assert_eq!(range[1].audit_id, e2.audit_id);
        assert_eq!(range[2].audit_id, e3.audit_id);

        let manifest = store.manifest_for(&range).await;
        assert_eq!(manifest.count, 3);
        assert_eq!(manifest.first_at, Some(e1.timestamp));
        assert_eq!(manifest.last_at, Some(e3.timestamp));
        assert_eq!(manifest.b3_hash.len(), 64);
        assert!(!manifest.b3_hash.is_empty());
    }

    #[tokio::test]
    async fn empty_range_returns_zero_events_with_empty_manifest_hash() {
        let store = InMemoryAuditStore::new();
        let range = store
            .query_range(ts(2026, 1, 1, 0), ts(2026, 12, 31, 23))
            .await;
        assert!(range.is_empty());
        let manifest = store.manifest_for(&range).await;
        assert_eq!(manifest.count, 0);
        assert_eq!(manifest.first_at, None);
        assert_eq!(manifest.last_at, None);
        assert!(manifest.b3_hash.is_empty());
    }

    #[tokio::test]
    async fn from_after_to_returns_no_events() {
        let store = InMemoryAuditStore::new();
        store.append(sample_event(ts(2026, 6, 12, 10), "m", &fp(1))).await;
        let range = store
            .query_range(ts(2026, 12, 31, 0), ts(2026, 1, 1, 0))
            .await;
        assert!(range.is_empty());
    }

    #[tokio::test]
    async fn manifest_hash_is_reproducible_for_same_events() {
        let store = InMemoryAuditStore::new();
        let events = vec![
            sample_event(ts(2026, 6, 12, 10), "m1", &fp(1)),
            sample_event(ts(2026, 6, 12, 11), "m2", &fp(2)),
            sample_event(ts(2026, 6, 12, 12), "m3", &fp(3)),
        ];
        let m1 = store.manifest_for(&events).await;
        let m2 = store.manifest_for(&events).await;
        assert_eq!(m1.b3_hash, m2.b3_hash);
        assert_eq!(m1.count, m2.count);
    }

    #[tokio::test]
    async fn manifest_hash_differs_when_events_differ() {
        let store = InMemoryAuditStore::new();
        let events_a = vec![sample_event(ts(2026, 6, 12, 10), "m1", &fp(1))];
        let events_b = vec![sample_event(ts(2026, 6, 12, 10), "m1", &fp(2))];
        let m_a = store.manifest_for(&events_a).await;
        let m_b = store.manifest_for(&events_b).await;
        assert_ne!(m_a.b3_hash, m_b.b3_hash);
    }

    #[tokio::test]
    async fn stored_event_prompt_fingerprint_is_32_bytes_not_text() {
        let store = InMemoryAuditStore::new();
        let ev = sample_event(ts(2026, 6, 12, 10), "m", &fp(7));
        store.append(ev.clone()).await;
        let range = store
            .query_range(ts(2026, 1, 1, 0), ts(2027, 1, 1, 0))
            .await;
        assert_eq!(range.len(), 1);
        let stored = &range[0];
        assert_eq!(stored.prompt_fingerprint.len(), 32);
        let json = serde_json::to_string(stored).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let fp_str = v["prompt_fingerprint"].as_str().expect("hex string");
        assert_eq!(fp_str.len(), 64);
        assert!(fp_str.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
