//! SQLite-backed audit-event store. [Refs: 6.4]
//!
//! Drop-in replacement for `InMemoryAuditStore` that survives process
//! restarts. Backed by sqlx + SQLite. Used when the operator wants
//! per-host durability (e.g., self-hosted deploys without Supabase).
//!
//! For multi-host setups, swap the `SqlitePool` for a Postgres
//! connection — the schema is intentionally portable.

use std::sync::Arc;
use std::time::Duration;

use argus_core::types::{AuditEvent, Manifest};
use blake3::Hasher;
use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tokio::sync::Mutex;

/// Thread-safe, process-persistent audit-event store backed by SQLite.
#[derive(Clone)]
pub struct SqliteAuditStore {
    pool: SqlitePool,
    /// In-memory cache of the most recent `prev_hash` for the next
    /// emitted event. The DB doesn't store per-store prev_hash; the
    /// worker holds that as part of its session state.
    ///
    /// Reserved for future use; currently the caller (VerifyWorker)
    /// passes the prev_hash explicitly. We keep the field so the
    /// `AuditStore` API is symmetric with `InMemoryAuditStore`.
    _phantom: Arc<Mutex<()>>,
}

impl SqliteAuditStore {
    /// Open a connection pool to the given SQLite URL (e.g.,
    /// `"sqlite://argus.db?mode=rwc"` or `"sqlite::memory:"`).
    /// Runs the schema migration on startup. Pool is small (1
    /// writer + a few readers) since writes are append-only.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let opts: SqliteConnectOptions = url.parse().map_err(|e: sqlx::Error| e)?;
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(opts)
            .await?;
        Self::migrate(&pool).await?;
        Ok(Self {
            pool,
            _phantom: Arc::new(Mutex::new(())),
        })
    }

    /// Idempotent schema migration. We inline a `CREATE TABLE IF NOT
    /// EXISTS` rather than carry a migrations/ directory — single
    /// table, single version, easier to reason about.
    async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS audit_events (
                audit_id TEXT PRIMARY KEY NOT NULL,
                timestamp TEXT NOT NULL,
                model_id TEXT NOT NULL,
                prompt_template_version TEXT NOT NULL,
                prompt_fingerprint BLOB NOT NULL,
                response_fingerprint BLOB NOT NULL,
                temperature REAL NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                estimated_cost_usd REAL NOT NULL,
                decision_json TEXT NOT NULL,
                prev_hash BLOB NOT NULL,
                signature BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_audit_events_timestamp
                ON audit_events (timestamp);
            "#,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Append a single event. The store is the source of truth for
    /// ordering; we trust the caller's `audit_id` to be unique.
    pub async fn append(&self, event: AuditEvent) -> Result<(), sqlx::Error> {
        let decision_json = serde_json::to_string(&event.decision)
            .map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        let ts = event.timestamp.to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO audit_events (
                audit_id, timestamp, model_id, prompt_template_version,
                prompt_fingerprint, response_fingerprint, temperature,
                input_tokens, output_tokens, estimated_cost_usd,
                decision_json, prev_hash, signature
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(event.audit_id.to_string())
        .bind(ts)
        .bind(&event.model_id)
        .bind(&event.prompt_template_version)
        .bind(&event.prompt_fingerprint[..])
        .bind(&event.response_fingerprint[..])
        .bind(event.temperature)
        .bind(event.input_tokens as i64)
        .bind(event.output_tokens as i64)
        .bind(event.estimated_cost_usd)
        .bind(decision_json)
        .bind(&event.prev_hash[..])
        .bind(&event.signature.to_bytes()[..])
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Number of events currently held.
    pub async fn len(&self) -> Result<usize, sqlx::Error> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_events")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0 as usize)
    }

    /// Is the store empty?
    pub async fn is_empty(&self) -> Result<bool, sqlx::Error> {
        Ok(self.len().await? == 0)
    }

    /// Manifest footer for a set of events. The BLAKE3 hash is over
    /// the canonical JSON of each event in arrival order with the
    /// per-event `signature` field zeroed — same posture as the
    /// in-memory store.
    pub fn manifest_for(&self, events: &[AuditEvent]) -> Manifest {
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

    async fn fresh_store() -> SqliteAuditStore {
        SqliteAuditStore::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite must connect")
    }

    fn sample_event(ts: DateTime<Utc>, model: &str) -> AuditEvent {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let mut ev = AuditEvent {
            audit_id: Uuid::new_v4(),
            timestamp: ts,
            model_id: model.into(),
            prompt_template_version: "v1".into(),
            prompt_fingerprint: [0u8; 32],
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

    #[tokio::test]
    async fn empty_store_has_zero_events() {
        let s = fresh_store().await;
        assert_eq!(s.len().await.unwrap(), 0);
        assert!(s.is_empty().await.unwrap());
    }

    #[tokio::test]
    async fn append_increments_count() {
        let s = fresh_store().await;
        s.append(sample_event(Utc::now(), "m1")).await.unwrap();
        s.append(sample_event(Utc::now(), "m2")).await.unwrap();
        assert_eq!(s.len().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn duplicate_audit_id_is_rejected() {
        let s = fresh_store().await;
        let ev = sample_event(Utc::now(), "m");
        s.append(ev.clone()).await.unwrap();
        // Same audit_id → PRIMARY KEY violation
        let res = s.append(ev).await;
        assert!(res.is_err(), "duplicate audit_id must be rejected");
    }

    #[tokio::test]
    async fn manifest_for_empty_events_has_empty_b3_hash() {
        let s = fresh_store().await;
        let m = s.manifest_for(&[]);
        assert_eq!(m.count, 0);
        assert!(m.b3_hash.is_empty());
    }
}
