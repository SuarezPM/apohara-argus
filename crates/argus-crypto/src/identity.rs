//! SPIFFE-like identity for ARGUS agents.
//!
//! Each agent instance gets a unique identity at creation time:
//!   spiffe://apohara.dev/argus/<role>/instance/<run_id>
//!
//! The identity is associated with a fresh Ed25519 keypair. All
//! messages signed by the agent are verifiable against its SPIFFE ID.

use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{generate_keypair, CryptoError, Result};

pub const ARGUS_TRUST_DOMAIN: &str = "apohara.dev";
pub const ARGUS_NAMESPACE: &str = "argus";

/// A SPIFFE-like ID for an agent instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SpiffeId(pub String);

impl SpiffeId {
    /// Mint a fresh ID for a given role and run.
    pub fn for_run(role: &str, run_id: &Uuid) -> Self {
        Self(format!(
            "spiffe://{}/{}/{}/instance/{}",
            ARGUS_TRUST_DOMAIN, ARGUS_NAMESPACE, role, run_id
        ))
    }

    /// Mint a fresh ID for a static role (used for long-lived agents).
    pub fn for_role(role: &str) -> Self {
        let run_id = Uuid::new_v4();
        Self(format!(
            "spiffe://{}/{}/{}/instance/{}",
            ARGUS_TRUST_DOMAIN, ARGUS_NAMESPACE, role, run_id
        ))
    }

    pub fn as_str(&self) -> &str { &self.0 }

    /// Extract the role from the ID. Returns the segment after `argus/`.
    pub fn role(&self) -> Option<&str> {
        self.0.split('/').nth(4)
    }
}

/// An agent's keypair + identity, bundled together.
#[derive(Debug)]
pub struct AgentKeypair {
    pub spiffe_id: SpiffeId,
    pub signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
}

impl AgentKeypair {
    /// Create a fresh keypair with a new SPIFFE ID for the given role.
    pub fn generate(role: &str) -> Self {
        let (sk, vk) = generate_keypair();
        Self {
            spiffe_id: SpiffeId::for_role(role),
            signing_key: sk,
            verifying_key: vk,
        }
    }

    /// Create a fresh keypair with a specific run_id.
    pub fn generate_for_run(role: &str, run_id: &Uuid) -> Self {
        let (sk, vk) = generate_keypair();
        Self {
            spiffe_id: SpiffeId::for_run(role, run_id),
            signing_key: sk,
            verifying_key: vk,
        }
    }

    /// Sign a payload with this agent's key.
    pub fn sign(&self, payload: &[u8]) -> ed25519_dalek::Signature {
        self.signing_key.sign(payload)
    }

    /// Verify a payload was signed by this agent.
    pub fn verify(&self, payload: &[u8], sig: &ed25519_dalek::Signature) -> Result<()> {
        self.verifying_key
            .verify(payload, sig)
            .map_err(|_| CryptoError::InvalidSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spiffe_id_format() {
        let id = SpiffeId::for_role("aegis-slop");
        assert!(id.as_str().starts_with("spiffe://apohara.dev/argus/aegis-slop/instance/"));
    }

    #[test]
    fn role_extraction() {
        let id = SpiffeId::for_role("aegis-verdict");
        assert_eq!(id.role(), Some("aegis-verdict"));
    }

    #[test]
    fn fresh_ids_are_unique() {
        let a = SpiffeId::for_role("aegis-slop");
        let b = SpiffeId::for_role("aegis-slop");
        assert_ne!(a, b);
    }

    #[test]
    fn keypair_signs_and_verifies() {
        let kp = AgentKeypair::generate("aegis-slop");
        let payload = b"hello from aegis";
        let sig = kp.sign(payload);
        assert!(kp.verify(payload, &sig).is_ok());
        assert!(kp.verify(b"tampered", &sig).is_err());
    }
}
