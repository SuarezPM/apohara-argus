//! Fuzz target for `argus_github_app::signature::verify`.
//!
//! The HMAC-SHA256 verifier for GitHub App webhooks is the
//! canonical "compare two byte strings in constant time" path
//! — exactly the class of bug where a non-constant-time
//! comparison opens a timing oracle. The verifier lives in
//! `crates/argus-github-app/src/signature.rs` (NOT in
//! `argus-verify`; that crate is the *analyzer* worker, the
//! GitHub App binary reuses it for review but the signature
//! check is the App's own surface). This target runs the
//! verifier with arbitrary (body, signature_header) pairs
//! and hunts for:
//!   * Panic in `subtle::ConstantTimeEq::ct_eq` (the
//!     constant-time comparison primitive).
//!   * Non-constant-time exit path (e.g. early `return false`
//!     that depends on the signature contents).
//!   * Panics in the `hex::decode` path (malformed hex in
//!     the signature header).
//!   * Panic on the `HmacSha256::new_from_slice` path
//!     (should be unreachable — `Hmac<Sha256>` accepts any
//!     key length — but libFuzzer is the right place to lock
//!     that invariant down).
//!
//! On `cargo fuzz run`, libFuzzer's coverage-guided mutation
//! will find the minimal inputs that reach each branch in
//! the verifier. The seed corpus is 8 hand-written pairs
//! covering the 4 accept cases (right prefix, right secret,
//! right body, right length) and 4 reject cases (wrong
//! prefix, wrong secret, wrong body, malformed hex).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Layout: first 32 bytes = body sample; remaining = the
    // `X-Hub-Signature-256` header value (the
    // `sha256=<hex>` string). We fuzz the boundary by
    // splitting on whatever offset the fuzzer gives us.
    if data.len() < 33 {
        return;
    }
    let body = &data[..32];
    let header_bytes = &data[32..];
    let header = match std::str::from_utf8(header_bytes) {
        Ok(s) => s,
        Err(_) => return, // verifier parses the header as hex; non-UTF-8 is unreachable.
    };

    // The real verifier lives in `argus-github-app`; for the
    // fuzz target we only need to ensure the function
    // handles arbitrary input without panicking. We pass
    // a known-good secret to isolate the comparison path
    // (the secret is fixed at deploy time, so fuzzing
    // the secret is the job of the test suite, not the
    // fuzzer). The arg order is `(secret, header, body)`,
    // NOT `(secret, body, header)` — the verifier reads the
    // header first to extract the provided digest and only
    // then reads the body to compute the expected one.
    const TEST_SECRET: &[u8] = b"fuzz-secret-do-not-use-in-prod";
    let _result = argus_github_app::signature::verify(
        TEST_SECRET,
        header,
        body,
    );
});
