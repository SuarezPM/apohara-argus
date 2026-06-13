//! SPIFFE Identity primitives, wrapping the `spiffe` crate.
//!
//! This module provides a thin wrapper around the `spiffe` crate's
//! `SpiffeId` and `TrustDomain` types. The wrapper exists to:
//! 1. Give us a single type to import throughout the codebase.
//! 2. Centralize the role → path mapping for ARGUS agents.
//! 3. Make the underlying `spiffe` crate easy to swap if needed.
//!
//! The wrapper's surface API (`from_uri`, `to_uri`, `for_role`, ...) is
//! independent of the inner `spiffe::SpiffeId` API, so we can absorb any
//! future upstream changes behind this module.
//!
//! The legacy [`crate::identity::SpiffeId`] type (a `String` newtype) is
//! **kept intact** for backward compatibility with the 8.1 JWT-SVID work.
//! New code should prefer [`SpiffeIdWrapper`] for spec-conformant parsing.

use spiffe::{SpiffeId as SpiffeIdInner, TrustDomain as TrustDomainInner};
use thiserror::Error;

/// Errors that can arise from SPIFFE ID construction or parsing.
#[derive(Debug, Error)]
pub enum SpiffeError {
    /// The input string was not a valid SPIFFE URI. Wraps the underlying
    /// `spiffe::SpiffeIdError` so callers retain access to the spec
    /// validation failure (missing scheme, bad trust domain, empty path, ...).
    #[error("invalid SPIFFE URI: {0}")]
    InvalidUri(#[from] spiffe::SpiffeIdError),
}

/// A spec-conformant SPIFFE ID for an ARGUS agent or workload.
///
/// Construct via [`SpiffeIdWrapper::from_uri`] (parse a full URI) or
/// [`SpiffeIdWrapper::for_role`] (build from trust-domain + role + id).
/// Format back to a URI string with [`SpiffeIdWrapper::to_uri`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpiffeIdWrapper(pub SpiffeIdInner);

impl SpiffeIdWrapper {
    /// Parse a SPIFFE URI such as `spiffe://argus.local/agent/aegis-slop`.
    ///
    /// Returns `Err(SpiffeError::InvalidUri(_))` for:
    /// - missing or wrong scheme (`https://...`, plain `argus.local/...`)
    /// - empty trust domain
    /// - empty path
    /// - any other violation of the [SPIFFE ID spec](https://github.com/spiffe/spiffe/blob/main/standards/SPIFFE-ID.md)
    pub fn from_uri(uri: &str) -> Result<Self, SpiffeError> {
        SpiffeIdInner::new(uri)
            .map(SpiffeIdWrapper)
            .map_err(SpiffeError::InvalidUri)
    }

    /// Format back to a SPIFFE URI string, e.g. `spiffe://argus.local/agent/aegis-slop`.
    pub fn to_uri(&self) -> String {
        self.0.to_string()
    }

    /// Construct a SPIFFE ID for a given trust domain, role, and id.
    ///
    /// Builds the URI `spiffe://{trust_domain}/{role}/{id}` and validates it.
    pub fn for_role(trust_domain: &str, role: &str, id: &str) -> Result<Self, SpiffeError> {
        // The inner `SpiffeId::new` validates the whole URI against the spec,
        // so we don't need to hand-validate the trust domain here.
        let uri = format!("spiffe://{}/{}/{}", trust_domain, role, id);
        Self::from_uri(&uri)
    }

    /// Borrow the inner spec-conformant trust domain.
    pub fn trust_domain(&self) -> &TrustDomainInner {
        self.0.trust_domain()
    }

    /// Return the trust domain as a string slice (e.g. `"argus.local"`).
    pub fn trust_domain_name(&self) -> &str {
        self.0.trust_domain_name()
    }

    /// Return the path component (e.g. `/agent/aegis-slop`).
    pub fn path(&self) -> &str {
        self.0.path()
    }
}

impl SpiffeIdWrapper {
    /// Convenience helper for the canonical ARGUS agent identity layout:
    /// `spiffe://argus.local/agent/{agent_id}`.
    ///
    /// Note: the existing 8.1 [`crate::identity::SpiffeId`] uses
    /// `apohara.dev` as trust domain and `argus/<role>/instance/<uuid>` as
    /// path. This helper uses a *different* layout (the spec-conformant
    /// one we want long-term). New code should pick one or the other
    /// explicitly; the legacy `SpiffeId` is preserved for backward
    /// compatibility with 8.1 callers.
    pub fn for_argus_agent(agent_id: &str) -> Self {
        Self::for_role("argus.local", "agent", agent_id)
            .expect("argus.local is a valid trust domain; agent/{id} is a valid path")
    }
}

// Re-export the spec types so downstream code only needs one import path.
pub use spiffe::TrustDomain;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_parses_and_roundtrips() {
        let uri = "spiffe://argus.local/agent/aegis-slop";
        let id = SpiffeIdWrapper::from_uri(uri).expect("valid URI must parse");
        assert_eq!(id.to_uri(), uri);
        assert_eq!(id.trust_domain_name(), "argus.local");
        assert_eq!(id.path(), "/agent/aegis-slop");
    }

    #[test]
    fn for_role_builds_valid_uri() {
        let id = SpiffeIdWrapper::for_role("argus.local", "agent", "aegis-slop")
            .expect("valid trust domain + path must build");
        assert_eq!(id.to_uri(), "spiffe://argus.local/agent/aegis-slop");
    }

    #[test]
    fn for_argus_agent_helper() {
        let id = SpiffeIdWrapper::for_argus_agent("aegis-slop");
        assert_eq!(id.to_uri(), "spiffe://argus.local/agent/aegis-slop");
    }

    #[test]
    fn edge_plain_string_rejected() {
        let result = SpiffeIdWrapper::from_uri("not-a-spiffe-uri");
        assert!(
            matches!(result, Err(SpiffeError::InvalidUri(_))),
            "plain string must be rejected, got {:?}",
            result
        );
    }

    #[test]
    fn edge_wrong_scheme_rejected() {
        // https:// instead of spiffe://
        let result = SpiffeIdWrapper::from_uri("https://argus.local/agent/aegis-slop");
        assert!(
            matches!(result, Err(SpiffeError::InvalidUri(_))),
            "https scheme must be rejected, got {:?}",
            result
        );
    }

    #[test]
    fn edge_trailing_slash_rejected() {
        // The SPIFFE spec forbids trailing slashes; the inner crate returns
        // `SpiffeIdError::TrailingSlash` for this.
        let result = SpiffeIdWrapper::from_uri("spiffe://argus.local/");
        assert!(
            matches!(result, Err(SpiffeError::InvalidUri(_))),
            "trailing slash must be rejected, got {:?}",
            result
        );
    }

    #[test]
    fn edge_bad_path_chars_rejected() {
        // Spaces in path segments are not allowed by the SPIFFE spec.
        let result = SpiffeIdWrapper::from_uri("spiffe://argus.local/agent/has space");
        assert!(
            matches!(result, Err(SpiffeError::InvalidUri(_))),
            "spaces in path must be rejected, got {:?}",
            result
        );
    }

    #[test]
    fn edge_only_scheme_rejected() {
        // "spiffe://" with no trust domain
        let result = SpiffeIdWrapper::from_uri("spiffe://");
        assert!(
            matches!(result, Err(SpiffeError::InvalidUri(_))),
            "scheme-only must be rejected, got {:?}",
            result
        );
    }

    /// Regression: the legacy `SpiffeId` (8.1, `String` newtype) and the
    /// new `SpiffeIdWrapper` produce the same canonical string for a
    /// well-formed URI, and both remain available side-by-side.
    #[test]
    fn regression_legacy_spiffe_id_still_works() {
        use crate::identity::SpiffeId;

        // Legacy 8.1 type still constructs and serializes.
        let legacy = SpiffeId::for_role("aegis-slop");
        let legacy_uri = legacy.as_str();
        assert!(
            legacy_uri.starts_with("spiffe://apohara.dev/argus/aegis-slop/instance/"),
            "legacy URI must keep its 8.1 layout, got {}",
            legacy_uri
        );

        // New wrapper parses a hand-crafted URI matching the spec layout.
        let new = SpiffeIdWrapper::from_uri(legacy_uri).expect("legacy URI is spec-conformant");
        assert_eq!(new.to_uri(), legacy_uri);
    }
}
