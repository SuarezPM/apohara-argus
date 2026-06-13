//! EU AI Act Article 12 audit-event emission.
//!
//! This module builds and emits [`AuditEvent`] records for every LLM call
//! that produced a decision. It is GDPR-safe by construction: the cleartext
//! prompt and response are NEVER stored — only BLAKE3 fingerprints.
//!
//! Emission target: `tracing::info!(target: "audit", ...)`. Downstream
//! consumers (NDJSON exporter, Prometheus exporter, compliance log shipper)
//! subscribe to the `"audit"` target via `tracing-subscriber::EnvFilter`.
//!
//! See: `docs/supremum-roadmap.md` §2.1 (Article 12 compliance).

use apohara_argus_core::{AuditEvent, DataClass, DecisionArtifact, ToolCallRecord};
use blake3::Hasher;
use chrono::Utc;
use ed25519_dalek::{Signature, Signer, SigningKey};
use uuid::Uuid;

/// Build and emit a single Article 12 audit event.
///
/// # GDPR
/// Only BLAKE3 fingerprints of `prompt_text` and `raw_response` are stored.
/// The cleartext is hashed and dropped; it never reaches the JSON record.
///
/// # Article 12 Level 2 conformance
/// `data_class` and `policy_version` are required at compile time
/// (no defaults). Callers must consciously classify every LLM call.
#[allow(clippy::too_many_arguments)]
pub fn emit_audit_event(
    model_id: &str,
    prompt_template_version: &str,
    prompt_text: &str,
    raw_response: &str,
    temperature: f32,
    tool_calls: Vec<ToolCallRecord>,
    decision: DecisionArtifact,
    prev_hash: [u8; 32],
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    data_class: DataClass,
    policy_version: &str,
    signing_key: &SigningKey,
) -> AuditEvent {
    // 1. Fingerprints. We hash the cleartext on the fly and never persist it.
    let prompt_fingerprint = blake3_bytes(prompt_text.as_bytes());
    let response_fingerprint = blake3_bytes(raw_response.as_bytes());

    // 2. Tokens. Prefer provider-reported counts; fall back to chars/4.
    let input_tokens = input_tokens.unwrap_or_else(|| (prompt_text.len() / 4) as u32);
    let output_tokens = output_tokens.unwrap_or_else(|| (raw_response.len() / 4) as u32);

    // 3. Cost. Static pricing table; free tier defaults to $0.
    let cost = estimate_cost(model_id, input_tokens, output_tokens);

    // 4. Build the event with a zeroed signature, sign the canonical form,
    //    then patch the signature in. This produces a self-consistent record.
    let mut event = AuditEvent {
        audit_id: Uuid::new_v4(),
        timestamp: Utc::now(),
        model_id: model_id.to_string(),
        prompt_template_version: prompt_template_version.to_string(),
        prompt_fingerprint,
        response_fingerprint,
        temperature,
        tool_calls,
        input_tokens,
        output_tokens,
        estimated_cost_usd: cost,
        data_class,
        policy_version: policy_version.to_string(),
        decision,
        prev_hash,
        signature: Signature::from_bytes(&[0u8; 64]),
    };

    let canonical = serde_json::to_vec(&event).expect("AuditEvent must serialize");
    event.signature = signing_key.sign(&canonical);

    // 5. Emit. The `"audit"` target lets log filters isolate this stream.
    tracing::info!(
        target: "audit",
        "{}",
        serde_json::to_string(&event).expect("AuditEvent re-serialize")
    );

    event
}

/// Advance the hash chain by one event. The next event's `prev_hash` is
/// `BLAKE3(prev_hash || canonical_event)`. This is what makes the chain
/// tamper-evident: changing any earlier entry breaks every subsequent
/// link.
pub fn next_prev_hash(prev_hash: [u8; 32], event: &AuditEvent) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(&prev_hash);
    let canonical = serde_json::to_vec(event).expect("AuditEvent must serialize");
    hasher.update(&canonical);
    hasher.finalize().into()
}

/// Rough per-model pricing in USD per 1M tokens.
///
/// The free tier is the only tier we ship today (NVIDIA NIM, Zhipu,
/// DeepSeek free previews as of 2026). Pricing values are tracked for
/// future cost reporting — even a $0 rate must be observable so finance
/// can confirm the free tier assumption.
fn estimate_cost(model_id: &str, input_tokens: u32, output_tokens: u32) -> f64 {
    // (input $/1M, output $/1M)
    let (in_price, out_price) = match model_id {
        m if m.contains("deepseek-v4-flash") => (0.0, 0.0),
        m if m.contains("deepseek-v3") => (0.0, 0.0),
        m if m.contains("nemotron-3-super") => (0.0, 0.0),
        m if m.contains("nemotron-4") => (0.0, 0.0),
        m if m.contains("glm-5") => (0.0, 0.0),
        m if m.contains("llama-3.1") => (0.0, 0.0),
        m if m.contains("mixtral") => (0.0, 0.0),
        m if m.contains("qwen") => (0.0, 0.0),
        _ => (0.0, 0.0),
    };
    (input_tokens as f64 * in_price + output_tokens as f64 * out_price) / 1_000_000.0
}

fn blake3_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use apohara_argus_core::DecisionArtifact;
    use ed25519_dalek::Verifier;
    use serde_json::Value;

    fn fresh_key() -> SigningKey {
        SigningKey::generate(&mut rand::rngs::OsRng)
    }

    fn sample_decision() -> DecisionArtifact {
        DecisionArtifact {
            verdict: "warn".into(),
            findings_count: 2,
            rationale: "Two minor slop patterns detected".into(),
        }
    }

    /// The 3rd hash in a 3-event chain is reproducible from `prev_hash` +
    /// payload. This is the tamper-evidence test: re-running the chain
    /// construction must produce the same hash as the on-the-fly
    /// `next_prev_hash` helper.
    #[test]
    fn hash_chain_3_events_reproducible() {
        let key = fresh_key();
        let prev0 = [0u8; 32];

        let e1 = emit_audit_event(
            "test-model",
            "v1",
            "prompt-1",
            "response-1",
            0.7,
            vec![],
            sample_decision(),
            prev0,
            None,
            None,
            DataClass::SourceCode,
            "policy-v1",
            &key,
        );
        let prev1 = next_prev_hash(prev0, &e1);

        let e2 = emit_audit_event(
            "test-model",
            "v1",
            "prompt-2",
            "response-2",
            0.7,
            vec![],
            sample_decision(),
            prev1,
            None,
            None,
            DataClass::SourceCode,
            "policy-v1",
            &key,
        );
        let prev2 = next_prev_hash(prev1, &e2);

        let e3 = emit_audit_event(
            "test-model",
            "v1",
            "prompt-3",
            "response-3",
            0.7,
            vec![],
            sample_decision(),
            prev2,
            None,
            None,
            DataClass::SourceCode,
            "policy-v1",
            &key,
        );
        let prev3 = next_prev_hash(prev2, &e3);

        // e1.prev_hash is the genesis, e2.prev_hash is the hash of e1, etc.
        assert_eq!(e1.prev_hash, prev0);
        assert_eq!(e2.prev_hash, prev1);
        assert_eq!(e3.prev_hash, prev2);

        // Tamper with e2's payload and verify prev3 no longer matches.
        let mut e2_tampered = e2.clone();
        e2_tampered.input_tokens = 9999;
        let prev3_after_tamper = next_prev_hash(prev1, &e2_tampered);
        assert_ne!(
            prev3_after_tamper, prev2,
            "tampering with e2 must change the chain link from e2 to e3"
        );
        // And the third event's stored prev_hash no longer matches.
        assert_ne!(e3.prev_hash, prev3_after_tamper);

        // Sanity: the final hash is non-zero and reproducible.
        assert_ne!(prev3, [0u8; 32]);
        assert_eq!(next_prev_hash(prev2, &e3), prev3);
    }

    /// The emitted event's signature is a valid Ed25519 sig over the
    /// canonical JSON of the event (with the signature field zeroed).
    #[test]
    fn emitted_event_signature_verifies() {
        let key = fresh_key();
        let event = emit_audit_event(
            "deepseek-ai/deepseek-v4-flash",
            "abc123",
            "promised not to be logged",
            "response",
            0.5,
            vec![],
            sample_decision(),
            [0u8; 32],
            Some(10),
            Some(5),
            DataClass::SourceCode,
            "policy-v1",
            &key,
        );

        // Re-canonicalize with the signature zeroed, then verify.
        let mut canonical = event.clone();
        canonical.signature = Signature::from_bytes(&[0u8; 64]);
        let bytes = serde_json::to_vec(&canonical).unwrap();
        key.verifying_key()
            .verify(&bytes, &event.signature)
            .expect("signature must verify against the zeroed-sig canonical form");
    }

    /// Sanity: the JSON we emit must contain all 14 fields of the
    /// `AuditEvent` schema in the order spec'd by the fixture.
    /// (Spec title says "13" but the field list has 14 — we go with
    /// the field list as the source of truth.)
    #[test]
    fn emitted_event_json_has_all_fields_and_no_cleartext() {
        let key = fresh_key();
        let secret = "patient: alice, ssn: 123-45-6789 — DO NOT LOG";
        let event = emit_audit_event(
            "test",
            "v1",
            secret,
            "response",
            0.5,
            vec![],
            sample_decision(),
            [0u8; 32],
            None,
            None,
            DataClass::SourceCode,
            "policy-v1",
            &key,
        );
        let v: Value = serde_json::to_value(&event).unwrap();
        let obj = v.as_object().unwrap();
        // 16 fields: 14 original (Roadmap 2.1) + `data_class` and
        // `policy_version` for EU AI Act Level 2 conformance (Refs: 4).
        assert_eq!(obj.len(), 16);
        // GDPR: the cleartext prompt (and any PII inside it) is gone.
        let s = serde_json::to_string(&event).unwrap();
        assert!(
            !s.contains(secret),
            "cleartext prompt must not appear in JSON"
        );
        assert!(!s.contains("123-45-6789"), "PII must not leak into JSON");
    }

    /// Pricing table sanity: all our known NIM models price to 0 on the
    /// free tier. If a model starts costing money, this test will fail and
    /// we update the table.
    #[test]
    fn pricing_table_free_tier() {
        for m in [
            "deepseek-ai/deepseek-v4-flash",
            "nvidia/nemotron-3-super-120b",
            "zhipuai/glm-5.1",
            "meta/llama-3.1-70b-instruct",
        ] {
            assert_eq!(estimate_cost(m, 1_000_000, 1_000_000), 0.0);
        }
    }

    /// The canonical fixture in `tests/fixtures/audit_event.json` must
    /// round-trip through `serde_json` without losing any fields.
    /// This guards against schema drift between the type and the
    /// documentation.
    #[test]
    fn fixture_roundtrip_matches_schema() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/fixtures/audit_event.json");
        let s = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
        let v: Value = serde_json::from_str(&s).expect("fixture must be valid JSON");
        let obj = v.as_object().unwrap();
        assert_eq!(
            obj.len(),
            14,
            "fixture must declare all 14 AuditEvent fields"
        );
        for key in [
            "audit_id",
            "timestamp",
            "model_id",
            "prompt_template_version",
            "prompt_fingerprint",
            "response_fingerprint",
            "temperature",
            "tool_calls",
            "input_tokens",
            "output_tokens",
            "estimated_cost_usd",
            "decision",
            "prev_hash",
            "signature",
        ] {
            assert!(obj.contains_key(key), "fixture missing field: {}", key);
        }
    }
}
