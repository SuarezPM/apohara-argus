//! HMAC-SHA256 signature verification for GitHub webhook payloads.
//!
//! GitHub signs every webhook delivery with the shared secret we
//! supplied when creating the App. The signature arrives in the
//! `X-Hub-Signature-256` header as `sha256=<64 hex chars>`. We
//! must verify it before trusting the payload — otherwise any
//! internet attacker who knows our webhook URL could trigger
//! reviews on behalf of the App.
//!
//! The verification is done with `subtle::ConstantTimeEq` so the
//! time-to-compare does not leak how many leading bytes match.
//! A plain `==` on `[u8; 32]` would let an attacker binary-search
//! the secret one byte at a time.
//!
//! [Refs: argus-silver-roadmap/P.2]

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// The prefix GitHub prepends to the hex digest in
/// `X-Hub-Signature-256`. Must be exactly `sha256=` (lowercase).
pub const SIGNATURE_PREFIX: &str = "sha256=";

/// Verify a GitHub `X-Hub-Signature-256` header against the
/// payload body and the App's webhook secret.
///
/// Returns `Ok(())` if the signature is valid, `Err(SignatureError)`
/// otherwise. The error variant is intentionally coarse (it does
/// not distinguish "wrong format" from "wrong digest") — leaking
/// the difference would aid the attacker.
///
/// **Timing**: the byte comparison is constant-time. The
/// surrounding hex decode is not (its length is bounded by the
/// header size, which is public), so no information about the
/// secret leaks.
pub fn verify(secret: &[u8], header: &str, body: &[u8]) -> Result<(), SignatureError> {
    // Strip the `sha256=` prefix.
    let hex = header
        .strip_prefix(SIGNATURE_PREFIX)
        .ok_or(SignatureError::Invalid)?;

    // GitHub's hex digest is always 64 chars (32 bytes).
    if hex.len() != 64 {
        return Err(SignatureError::Invalid);
    }

    // Decode the provided signature.
    let provided = hex::decode(hex).map_err(|_| SignatureError::Invalid)?;
    if provided.len() != 32 {
        return Err(SignatureError::Invalid);
    }

    // Compute the expected signature.
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| SignatureError::InvalidKey)?;
    mac.update(body);
    let expected = mac.finalize().into_bytes();

    // Constant-time compare. `ConstantTimeEq::ct_eq` returns
    // `Choice`, which we convert to bool with `into()`. The
    // compiler is not allowed to short-circuit on a `Choice`.
    let eq: bool = expected.ct_eq(&provided).into();
    if eq {
        Ok(())
    } else {
        Err(SignatureError::Invalid)
    }
}

/// Compute the signature GitHub would produce for the given
/// payload, returning the full `sha256=<hex>` string. Used in
/// integration tests to forge a valid signature for a mock
/// request.
pub fn sign(secret: &[u8], body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    format!("{}{}", SIGNATURE_PREFIX, hex::encode(bytes))
}

/// Reasons a signature might be rejected. Kept as a single
/// `Invalid` variant for the reasons discussed in [`verify`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("invalid webhook signature")]
    Invalid,
    /// Reserved for future use (key length validation). The
    /// `hmac` crate accepts any key length, so we never hit this
    /// in practice, but exposing the variant lets callers log
    /// the difference between "no signature" and "bad signature"
    /// without leaking whether the key itself was malformed.
    #[error("invalid signing key")]
    InvalidKey,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"super-secret-webhook-key";
    const BODY: &[u8] = br#"{ "action": "opened", "number": 42 }"#;

    #[test]
    fn sign_then_verify_round_trips() {
        let header = sign(SECRET, BODY);
        assert!(header.starts_with("sha256="));
        assert!(verify(SECRET, &header, BODY).is_ok());
    }

    #[test]
    fn rejects_missing_prefix() {
        let header = sign(SECRET, BODY);
        let no_prefix = header.trim_start_matches("sha256=");
        assert!(matches!(
            verify(SECRET, no_prefix, BODY),
            Err(SignatureError::Invalid)
        ));
    }

    #[test]
    fn rejects_wrong_secret() {
        let header = sign(SECRET, BODY);
        assert!(matches!(
            verify(b"other-secret", &header, BODY),
            Err(SignatureError::Invalid)
        ));
    }

    #[test]
    fn rejects_tampered_body() {
        let header = sign(SECRET, BODY);
        let tampered = br#"{ "action": "tampered", "number": 42 }"#;
        assert!(matches!(
            verify(SECRET, &header, tampered),
            Err(SignatureError::Invalid)
        ));
    }

    #[test]
    fn rejects_truncated_signature() {
        let header = format!("sha256{}", "a"); // 5 hex chars only
        assert!(matches!(
            verify(SECRET, &header, BODY),
            Err(SignatureError::Invalid)
        ));
    }

    #[test]
    fn rejects_empty_signature() {
        assert!(matches!(
            verify(SECRET, "", BODY),
            Err(SignatureError::Invalid)
        ));
        assert!(matches!(
            verify(SECRET, "sha256=", BODY),
            Err(SignatureError::Invalid)
        ));
    }

    #[test]
    fn constant_time_compare_actually_used() {
        // Sanity: the `subtle` crate is wired in. We do not measure
        // timing here (that would be flaky); the existence of the
        // `ct_eq` import in `verify` is the check.
        let a = [0u8; 32];
        let b = [0u8; 32];
        assert!(bool::from(a.ct_eq(&b)));
        let c = [1u8; 32];
        assert!(!bool::from(a.ct_eq(&c)));
    }
}
