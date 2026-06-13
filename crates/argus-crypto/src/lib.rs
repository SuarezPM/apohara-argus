//! argus-crypto — ed25519 signing, BLAKE3 hash chain, SPIFFE-like IDs
//!
//! Three responsibilities:
//! 1. Sign every action an agent takes (Ed25519)
//! 2. Chain every entry in the ledger (BLAKE3 hash chain)
//! 3. Mint SPIFFE-like identifiers per agent instance

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::Serialize;
use thiserror::Error;

pub mod chain;
pub mod identity;
pub mod spiffe_id;

pub use chain::{ChainEntry, HashChainError, GENESIS_HASH};
pub use identity::{AgentKeypair, SpiffeId, ARGUS_NAMESPACE, ARGUS_TRUST_DOMAIN};
pub use spiffe_id::{SpiffeError, SpiffeIdWrapper, TrustDomain};

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Invalid key: {0}")]
    InvalidKey(String),
    #[error("Base64 decode error: {0}")]
    Base64(String),
    #[error("JSON error: {0}")]
    Json(String),
}

pub type Result<T> = std::result::Result<T, CryptoError>;

pub fn sign(key: &SigningKey, payload: &[u8]) -> Signature {
    key.sign(payload)
}

pub fn verify(key: &VerifyingKey, payload: &[u8], sig: &Signature) -> Result<()> {
    key.verify(payload, sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

pub fn blake3_hex(payload: &[u8]) -> String {
    blake3::hash(payload).to_hex().to_string()
}

pub fn b64_encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>> {
    B64.decode(s)
        .map_err(|e| CryptoError::Base64(e.to_string()))
}

pub fn sign_json<T: Serialize>(key: &SigningKey, payload: &T) -> Result<String> {
    let bytes = serde_json::to_vec(payload).map_err(|e| CryptoError::Json(e.to_string()))?;
    let sig = key.sign(&bytes);
    Ok(B64.encode(sig.to_bytes()))
}

pub fn generate_keypair() -> (SigningKey, VerifyingKey) {
    let mut csprng = OsRng;
    let sk = SigningKey::generate(&mut csprng);
    let vk = sk.verifying_key();
    (sk, vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sign_verify() {
        let (sk, vk) = generate_keypair();
        let payload = b"hello argus";
        let sig = sign(&sk, payload);
        assert!(verify(&vk, payload, &sig).is_ok());
    }

    #[test]
    fn tampered_payload_fails() {
        let (sk, vk) = generate_keypair();
        let sig = sign(&sk, b"hello argus");
        assert!(verify(&vk, b"hello ARGUS", &sig).is_err());
    }

    #[test]
    fn blake3_deterministic() {
        let a = blake3_hex(b"hello");
        let b = blake3_hex(b"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn base64_roundtrip() {
        let original = b"some bytes \x00\x01\x02";
        let encoded = b64_encode(original);
        let decoded = b64_decode(&encoded).expect("decode");
        assert_eq!(original, decoded.as_slice());
    }

    #[test]
    fn sign_json_roundtrip() {
        let (sk, vk) = generate_keypair();
        let payload = serde_json::json!({ "verdict": "APPROVED", "score": 0.1 });
        let sig = sign_json(&sk, &payload).expect("sign");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let sig_bytes = b64_decode(&sig).expect("decode sig");
        let sig_obj = Signature::from_bytes(&sig_bytes.try_into().unwrap());
        assert!(verify(&vk, &bytes, &sig_obj).is_ok());
    }
}
