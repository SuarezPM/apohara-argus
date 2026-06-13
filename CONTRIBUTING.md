# Contributing to ARGUS

Thanks for considering a contribution. ARGUS is a trust layer for
AI-generated code, so two rules are non-negotiable: **the build stays
green** and **every claim is code-verifiable** (no doc or README assertion
ships without a test that pins it).

## Build, test, lint

```sh
cargo build                              # build the workspace
cargo test                               # all unit + integration tests
cargo test --benches                     # also run the harness=false benches
                                         #   (regex_redos as a regression gate)
cargo clippy --all-targets -- -D warnings  # lints are errors
cargo fmt --check                        # formatting must be clean
cargo deny check licenses                # dependency-license allowlist
```

A change is not done until `cargo test`, `cargo test --benches`,
`cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` all
pass.

The three quickest ways to exercise a change by hand: the **guard**
(`cargo run -p argus-guard -- check '<diff>'`), the **verify** HTTP
surface (`cargo run -p argus-verify` and `curl` the `/analyze`
endpoint), and the **slop detector** (`cargo run -p argus-slop --
detect < file.rs`).

## Quality gate

Every commit MUST keep the following green. CI enforces all of them;
please run them locally before opening a PR:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --benches            # the ReDoS bench is a regression gate
cargo deny check licenses       # dependency-license allowlist
cargo deny check advisories     # RUSTSEC advisories
```

Pull requests that break `cargo test`, introduce clippy warnings, or
relax the 0-FP / 0-FN benchmark will not be merged.

## Adding a slop rule (the deterministic layer)

Slop rules live in `crates/argus-slop/src/rules/` and are registered
in the rule registry (`crates/argus-slop/src/registry.rs`).

1. Add a matcher `fn m_<name>(s: &str) -> bool` (use the `re!` macro
   for a compile-constant regex; **no nested quantifiers** — the
   ReDoS bench guards sub-ms matching).
2. Register a `SlopRule { id, severity, category, matcher }` in
   `rules()`. Severity drives the tier: clearly slop-equivalent
   `>= 8` (Block), ambiguous `5..=7` (Warn), benign-looking but
   suspicious `3..=4` (Flag).
3. **Required fixtures (both directions):**
   - **Positive:** the slop form Blocks (add to
     `crates/argus-slop/tests/corpus/slop_dangerous.txt` and/or a
     unit test in the rule module).
   - **Negative:** a *benign* Rust idiom Allows (add to
     `crates/argus-slop/tests/corpus/slop_benign.txt` and/or
     `crates/argus-slop/tests/gate_fp.rs`). State the
     **false-positive risk** of your regex in a comment — what
     benign code might it bite, and why it does not.
4. If the rule interacts with the CordonEnforcer (the synthesizer
   redaction), add a case to `crates/argus-verify/tests/cordon.rs`.

The FP/FN benchmark (`crates/argus-slop/tests/benchmark.rs`) asserts
**0% FP and 0% FN on the curated corpus**; a benign Rust idiom that
Blocks is a real bug to fix, not a number to relax.

## Adding an LLM specialist prompt

Specialist prompts live in `crates/apohara-argus-core/prompts/`. Adding a
new prompt is a **breaking change** for the audit chain
(`policy_version` is part of the 15-field `AuditEvent`), so:

1. Add the new prompt file with a `version: ` line.
2. Bump the `policy_version` constant in
   `crates/apohara-argus-core/src/types.rs`.
3. Wire the new prompt in the specialist that consumes it
   (`SlopDetector`, `SecurityReview`, `ArchitectureFit`, or
   `VerdictSynthesizer`).
4. Add **a positive and a negative fixture** in
   `crates/argus-llm/tests/fixtures/` so a regression in the
   specialist's contract fails CI without burning a NIM call.

## Honesty rule (mandatory)

**When you change slop, verify, or audit behavior, update the
honesty net in the SAME change:**

- If you close (or open) a slop evasion, update
  `crates/argus-slop/tests/evasions.rs` so it *pins the new
  reality* (Block vs. Allow/incidental), **and** update the README
  "Now caught" / "Still out of scope" lists. The
  `crates/argus-slop/tests/readme_sync.rs` test asserts these two
  stay consistent — a drift fails the build.
- If you change the threat model, update `SECURITY.md` so its
  "Covers / does NOT cover" still matches reality.
- All tests must stay green (`cargo test` **and**
  `cargo test --benches`).

No claim ships that a test cannot back.

## Coding standards

The project's **required coding style is enforced automatically**,
so there is no style guide to memorize:

- **Formatting:** `rustfmt` with the repository defaults
  (`cargo fmt`). All code MUST be `rustfmt`-clean; CI runs
  `cargo fmt --check`.
- **Linting:** `clippy` with **warnings denied**
  (`cargo clippy --all-targets -- -D warnings`). Contributions
  MUST be clippy-clean; CI denies any warning
  (`RUSTFLAGS: "-D warnings"`).
- **Language:** code and comments are in **English**; comment the
  *why*, not the *what*. A new slop rule carries an `fp_risk` note
  (what benign code could match and why the severity is set where
  it is).

Because both tools run in CI and are required to pass, compliance
is checked on every change rather than left to reviewer discretion.

## Testing policy

Tests are part of the change, not an afterthought:

- **Major new functionality MUST add tests** to the automated
  suite in the same change. A feature without tests is not
  considered complete and will not be merged. A new slop rule
  MUST ship fixtures in **both directions** (a positive case that
  Blocks/Warns and a benign negative that Allows) — see *Adding
  a slop rule* and *Adding an LLM specialist prompt* above.
- **Bug fixes SHOULD add a regression test** that fails before
  the fix and passes after, so the bug cannot silently return.
- The automated suite runs **on every push and pull request**
  (CI, across Linux / macOS / Windows) and reports
  success / failure; a red suite blocks the merge.
- Precision is **measured, not asserted** — the FP / FN
  benchmark (`crates/argus-slop/tests/benchmark.rs`) asserts
  **`0` false positives and `0` false negatives** on the
  curated corpus; a benign Rust idiom that Blocks or a missed
  slop pattern is a real bug to fix, **not a number to relax**.

Statement coverage is measured with
[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov)
(`cargo llvm-cov --summary-only`); see `docs/dependency-audit.md`
for the current figure.

## Pull requests

The `main` branch is **protected**: it cannot be pushed to
directly, and force-push and branch deletion are disabled. Every
change — including the maintainer's — lands through a pull
request that **must pass the full CI suite** (rustfmt, clippy
`-D warnings`, `cargo-deny` licenses + advisories, the
clean-install independence gate, the default-build purity
guard, and the test matrix on Linux / macOS / Windows) before
it can be merged.

- Keep changes focused; one logical change per PR.
- Update [`CHANGELOG.md`](CHANGELOG.md) under `[Unreleased]`
  when your change is user-visible.
- Code and comments are written in English. Comment the *why*,
  not the *what*.

### Conventional Commits

Commit messages follow
[Conventional Commits](https://www.conventionalcommits.org/):
`feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `test:`,
`bench:`, `ci:`, etc. This keeps the history machine-readable
and drives the changelog.

### Developer Certificate of Origin (DCO)

By contributing, you certify the
[DCO](https://developercertificate.org/): that you wrote the
patch or otherwise have the right to submit it under the
project's license. Sign off your commits with `git commit -s`,
which appends a `Signed-off-by:` trailer. CI will reject
contributions whose commits lack the `Signed-off-by` line.

## License

ARGUS is licensed under the **MIT License** — see
[`LICENSE`](LICENSE) for the full text.

Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in the work by you, as
defined in the MIT license, shall be licensed under the MIT
license, without any additional terms or conditions.
