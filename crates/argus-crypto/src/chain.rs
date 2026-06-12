//! BLAKE3 hash chain for the ARGUS audit ledger.
//!
//! Every ledger entry contains `prev_hash` and `hash` where:
//!   hash = blake3(prev_hash || serialized_payload)
//!
//! This makes the ledger tamper-evident: any modification of an earlier
//! entry invalidates the chain from that point forward.

use blake3::Hasher;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HashChainError {
    #[error("Chain broken: expected prev_hash {expected}, got {actual}")]
    Broken { expected: String, actual: String },
    #[error("Empty chain")]
    Empty,
}

/// The genesis hash of a chain (the "prev_hash" of the first entry).
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEntry {
    pub prev_hash: String,
    pub hash: String,
    pub payload: serde_json::Value,
}

/// Append a payload to the chain, returning the new entry.
/// `prev_hash` should be the hash of the previous entry (or GENESIS_HASH).
pub fn append(prev_hash: &str, payload: &serde_json::Value) -> ChainEntry {
    let payload_bytes = serde_json::to_vec(payload).expect("payload must be JSON");
    let mut hasher = Hasher::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(&payload_bytes);
    let hash = hasher.finalize().to_hex().to_string();
    ChainEntry {
        prev_hash: prev_hash.to_string(),
        hash,
        payload: payload.clone(),
    }
}

/// Verify that a sequence of entries forms a valid chain.
pub fn verify_chain(entries: &[ChainEntry]) -> Result<(), HashChainError> {
    if entries.is_empty() {
        return Err(HashChainError::Empty);
    }
    let mut expected_prev = GENESIS_HASH.to_string();
    for (i, entry) in entries.iter().enumerate() {
        if entry.prev_hash != expected_prev {
            return Err(HashChainError::Broken {
                expected: expected_prev,
                actual: entry.prev_hash.clone(),
            });
        }
        // Re-compute the hash to verify it matches.
        let payload_bytes = serde_json::to_vec(&entry.payload).expect("payload must be JSON");
        let mut hasher = Hasher::new();
        hasher.update(entry.prev_hash.as_bytes());
        hasher.update(&payload_bytes);
        let computed = hasher.finalize().to_hex().to_string();
        if computed != entry.hash {
            return Err(HashChainError::Broken {
                expected: computed,
                actual: entry.hash.clone(),
            });
        }
        expected_prev = entry.hash.clone();
        // Silence unused warning for index
        let _ = i;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn append_creates_consistent_hash() {
        let payload = json!({"verdict": "APPROVED", "score": 0.1});
        let e1 = append(GENESIS_HASH, &payload);
        assert_eq!(e1.prev_hash, GENESIS_HASH);
        assert!(!e1.hash.is_empty());

        // Re-append should produce same hash
        let e1_again = append(GENESIS_HASH, &payload);
        assert_eq!(e1.hash, e1_again.hash);
    }

    #[test]
    fn chain_verifies() {
        let mut entries = Vec::new();
        let mut prev = GENESIS_HASH.to_string();
        for i in 0..5 {
            let payload = json!({"i": i, "verdict": "OK"});
            let entry = append(&prev, &payload);
            prev = entry.hash.clone();
            entries.push(entry);
        }
        assert!(verify_chain(&entries).is_ok());
    }

    #[test]
    fn tampered_chain_fails() {
        let mut entries = Vec::new();
        let mut prev = GENESIS_HASH.to_string();
        for i in 0..3 {
            let entry = append(&prev, &json!({"i": i}));
            prev = entry.hash.clone();
            entries.push(entry);
        }
        // Tamper with the second entry's payload
        entries[1].payload = json!({"i": 999, "tampered": true});
        assert!(verify_chain(&entries).is_err());
    }

    #[test]
    fn empty_chain_errors() {
        assert!(verify_chain(&[]).is_err());
    }

    #[test]
    fn broken_prev_hash_fails() {
        let mut entries = Vec::new();
        entries.push(append(GENESIS_HASH, &json!({"a": 1})));
        entries.push(append(GENESIS_HASH, &json!({"a": 2}))); // wrong prev_hash
        assert!(verify_chain(&entries).is_err());
    }
}
