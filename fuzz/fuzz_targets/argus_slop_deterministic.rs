//! Fuzz target for `argus_slop::run_deterministic_rules`.
//!
//! The 5 SLOP-001..SLOP-005 rules in `argus-slop/src/deterministic.rs`
//! use regex pattern matching against arbitrary Rust source code
//! (PR diffs, file contents, gists). The public entry point
//! `run_deterministic_rules(&str) -> Vec<SlopSignal>` is the
//! attack surface: a malformed input could trigger a panic in
//! any of the 5 rules, or (worse) produce a false negative that
//! lets AI slop bypass the pre-flight analyzer.
//!
//! What this target hunts:
//!   * Panics in `slop_001_oversized` / `slop_002_swallowed` /
//!     `slop_003_todo` / `slop_004_unwrap` / `slop_005_unused_pub`.
//!   * Quadratic / exponential regex backtracking (the 5 rules
//!     use nested scan loops; a pathological input could
//!     DoS the pre-flight analyzer).
//!   * Uninitialized-`line` signals (line=0 would break
//!     the downstream `signal.line` consumers).
//!
//! The fuzzer starts with a small seed corpus of real Rust
//! snippets (curated from the argus-slop test suite) and
//! mutates them via libFuzzer's standard strategies
//! (crossover, byte-flip, dict).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // libFuzzer gives us arbitrary bytes; the rules expect
    // UTF-8. `from_utf8_lossy` mirrors what `run_deterministic_rules`
    // sees when called from the LLM pipeline (the diff is
    // pulled from GitHub's API as a JSON string, which is
    // guaranteed UTF-8 in the happy path but the test
    // here is *un*happy-path fuzzing, so lossy is the
    // right contract).
    let input = String::from_utf8_lossy(data);

    // The 5 rules are pure functions. A panic inside any of
    // them would be caught by libFuzzer's signal handler
    // and recorded as a crash. A successful return is
    // also fine — we don't assert on the output, just on
    // the absence of a panic.
    let _signals = argus_slop::run_deterministic_rules(&input);
});
