//! Empty stub for `sqlx-mysql`.
//!
//! The real `sqlx-mysql@0.8.6` transitively pulls in `rsa@0.9.10`,
//! which has RUSTSEC-2023-0071 (Marvin Attack: timing sidechannel
//! in PKCS#1 v1.5 decryption, unfixed upstream). It only reaches
//! our `Cargo.lock` as an optional feature the workspace never
//! enables (the `mysql` feature is not in the workspace's sqlx
//! feature list). The OpenSSF Scorecard `Vulnerabilities` check
//! inspects `Cargo.lock` via `osv-scanner` and reports 0 only when
//! `rsa` is absent.
//!
//! Patching `sqlx-mysql` to this empty stub removes the entire
//! `rsa` subtree (rsa, pkcs1, pkcs8, signature, spki, sha1, sha2,
//! hmac, md-5, num-bigint, num-integer, num-iter, num-traits, pem,
//! getrandom, digest, crypto-bigint, crypto-common, elliptic-curve,
//! etc., ~30 transitive crates) from `Cargo.lock`.
//!
//! If a future workspace member ever enables sqlx's `mysql` feature,
//! the stub will fail to link with a clear `error[E0432]` directing
//! the maintainer to either (a) wait for an upstream `rsa` fix
//! (none exists as of 0.9.10) or (b) accept and document the
//! vulnerability in `deny.toml` with a written justification.
//!
//! The scorecard `Vulnerabilities` check goes from 0 to 9+ when
//! this stub is in place.

#![allow(dead_code, missing_docs)]
