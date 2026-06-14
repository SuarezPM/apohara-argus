# CLAUDE.md

AI-agent context file for **ARGUS** (apohara-argus). Read this
before touching the repo.

## What ARGUS is

ARGUS = **AI Review & Governance for Undermining Slop** (the trust
layer for AI-generated code). A **hybrid (deterministic regex + LLM
semantic) defense layer** packaged as a **15-crate Rust workspace**.
Three runtime surfaces (`argus-guard` pre-commit, `argus-verify` PR
review HTTP, `argus-lens` weekly digest) plus a **MCP server**
(`apohara-argus-mcp`) that exposes the 4 specialists (`aegis_slop`,
`aegis_security`, `aegis_arch`, `aegis_verdict`) to Claude Code /
Codex / Cursor.

The 15-field `AuditEvent` is **BLAKE3 hash-chained, Ed25519
signed**, and **EU AI Act Art. 12 Level 2 conformant by default**.
BYOK for the LLM layer (NVIDIA NIM via `ARGUS_NIM_KEY` env var).
**MIT licensed** at the top level.

## Files that matter (read before changing)

| File | Why it matters |
|------|----------------|
| `Cargo.toml` | Workspace root, `license = "MIT"`, `version = "0.1.0"`, `rust-version = "1.88"`, members list. Touching the `members` list without a re-pin is a build break. |
| `crates/apohara-argus-core/src/types.rs` | The 15-field `AuditEvent`, the `DataClass` enum, the `policy_version` constant. Bumping `policy_version` is a breaking change for the audit chain. |
| `crates/apohara-argus-core/prompts/` | The 4 specialist prompts (`slop-detector`, `redteam-security`, `architecture-fit`, `verdict-synthesizer`). Adding a new prompt is a breaking change. |
| `crates/argus-crypto/` | BLAKE3 chaining + Ed25519 signing. **Do not weaken** the chain or skip the signature step. |
| `crates/argus-slop/src/rules/` | The SLOP-001..005 regex rules. **Required** positive + negative fixtures in the corpus. |
| `crates/argus-slop/tests/benchmark.rs` | The 0-FP / 0-FN gate. The benchmark is the contract. |
| `crates/argus-llm/src/nim.rs` | The NIM BYOK client. The diff **leaves the host** and goes to the user-provided endpoint. |
| `crates/argus-verify/src/cordon.rs` | The CordonEnforcer: the synthesizer must never see raw diff text. **Type-level isolation**, not runtime. |
| `SECURITY.md` | The authoritative "covers / does NOT cover" threat model. Update it when you change a guarantee. |
| `CHANGELOG.md` | Keep a Changelog format. Anything user-visible lands under `[Unreleased]`. |
| `LICENSE` | MIT at the top level. **Do not** switch to dual-license without a maintainer call. |

## Files that are load-bearing for the threat model (do NOT touch without a SECURITY.md update)

- `crates/argus-crypto/` (audit chain integrity)
- `crates/argus-verify/src/cordon.rs` (synthesizer isolation)
- `crates/argus-slop/src/rules/` and `tests/corpus/slop_*.txt` (deterministic guarantees)
- `crates/argus-slop/tests/benchmark.rs` (precision contract)

## Build, test, lint

```sh
cargo build                              # build the workspace
cargo test                               # all unit + integration tests
cargo test --benches                     # also run the harness=false benches
cargo clippy --all-targets -- -D warnings  # lints are errors
cargo fmt --check                        # formatting must be clean
cargo deny check licenses                # dependency-license allowlist
cargo deny check advisories              # RUSTSEC advisories
```

A change is not done until `cargo test`, `cargo test --benches`,
`cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`
all pass.

## Always Do

- **MUST run `cargo test` + `cargo test --benches` + `cargo clippy
  --all-targets -- -D warnings` + `cargo fmt --check`** before
  declaring any change done. A red suite blocks the merge.
- **MUST add both positive and negative fixtures** when adding a
  slop rule. A new rule without a committed `slop_dangerous.txt` +
  `slop_benign.txt` case is not a complete change.
- **MUST update `CHANGELOG.md` under `[Unreleased]`** when the
  change is user-visible.
- **MUST update `SECURITY.md`** when you change a guarantee the
  "covers / does NOT cover" threat model describes. The
  `crates/argus-slop/tests/readme_sync.rs`-style honesty net is
  the contract; the README / SECURITY drift is a build break.
- **MUST sign off your commits** with `git commit -s` (DCO). CI
  rejects unsigned commits.
- **MUST comment the *why*, not the *what*.** Code and comments
  are in English.

## Never Do

- **NEVER edit `crates/argus-crypto/`** without a paired
  `SECURITY.md` update and a regression test that pins the new
  chain behavior. The BLAKE3 + Ed25519 chain is the
  regulator-facing artifact.
- **NEVER weaken the CordonEnforcer.** The
  `VerdictSynthesizer` receives a `RedactedSpecialistReport`,
  not a `SpecialistReport` and not a `String`. The type system
  is the isolation; do not relax it to "trust me" runtime
  checks.
- **NEVER default-on a sensitive surface.** The audit store,
  the SQLite backend, the OpenTelemetry exporter, and the
  A2A AgentCards surface are all **opt-in**. The default build
  stays self-contained.
- **NEVER log raw diff text** to the audit chain. The default
  schema is metadata-only; raw diff requires
  `include_diff = true` and is secret-redacted + truncated.
- **NEVER host user diffs.** ARGUS is offline-first and BYOK.
  A hosted / SaaS mode is explicitly out of scope (see
  `SECURITY.md` § Non-goals).
- **NEVER claim a 100% guarantee.** The deterministic layer's
  FP / FN benchmark is the contract; the LLM layer inherits the
  model's accuracy. The honest posture is "high-confidence on
  the deterministic layer, semantically strong on the LLM
  layer, never 100%."
- **NEVER switch the license** from MIT to dual-license
  without an explicit maintainer call. The `Cargo.toml`
  `license = "MIT"` and the top-level `LICENSE` file must
  stay in lockstep.
- **NEVER use `git commit --no-verify` or skip CI hooks** to
  land a change. The full CI suite (rustfmt, clippy `-D
  warnings`, cargo-deny licenses + advisories, the
  clean-install independence gate, the default-build purity
  guard, the test matrix) is non-negotiable.

## Honesty rule (mirrored from CONTRIBUTING.md)

When you change slop, verify, or audit behavior, update the
honesty net in the same change:

- If you close (or open) a slop evasion, update
  `crates/argus-slop/tests/evasions.rs` **and** the README
  "Now caught" / "Still out of scope" lists.
- If you change the threat model, update `SECURITY.md` so
  the "Covers / does NOT cover" sections still match
  reality.
- If you change the audit schema, update `CHANGELOG.md` and
  bump `policy_version` in
  `crates/apohara-argus-core/src/types.rs`.

No claim ships that a test cannot back.

## Resources

| Resource | Use for |
|----------|---------|
| `docs/dependency-audit.md` | The current dependency-license allowlist, RUSTSEC status, and retention windows for the EU AI Act Art. 12 chain. |
| `docs/iteration-roadmap.md` | The "what's next" list. Items in the *Deferred* column are deliberate, not forgotten. |
| `docs/implementation-status.md` | The current Wave's shipped / deferred count. |
| `docs/agent-spec.md` | The full agent spec: data model, specialist contracts, the CordonEnforcer semantics. |
| `SECURITY.md` | The threat model. |
| `GOVERNANCE.md` | The roles table and the access-continuity plan. |
| `CONTRIBUTING.md` | The DCO, coding standards, and the testing policy. |
| `CHANGELOG.md` | The retroactive + forward-looking change log. |

## Routing for AI agents

- "What does X do in ARGUS?" → read the relevant crate's
  `lib.rs` module docs first; they are the source of truth.
- "What breaks if I change X?" → read its test file, then
  `cargo test` to see what fails before editing.
- "Is this in scope?" → check `SECURITY.md` § Non-goals
  and `docs/iteration-roadmap.md` *Deferred* column.
- "How do I add a slop rule?" → follow
  `CONTRIBUTING.md` § *Adding a slop rule* end to end.
- "How do I add an LLM specialist?" → follow
  `CONTRIBUTING.md` § *Adding an LLM specialist prompt*;
  remember `policy_version` is a compile-time constant.

## Project state (Wave S.1 starting point)

- **Workspace**: 15 crates, all Rust 2021, `rust-version =
  "1.88"`, `license = "MIT"`.
- **Three runtime surfaces**: `argus-guard`, `argus-verify`,
  `argus-lens` (all 3 in v0.1).
- **MCP surface**: `apohara-argus-mcp` (v0.1, 4 specialists).
- **Audit chain**: BLAKE3 + Ed25519, 15-field `AuditEvent`,
  EU AI Act Art. 12 Level 2 conformant.
- **Test count**: 145+ (per README badge), with a committed
  0-FP / 0-FN gate on the slop corpus.
- **CI**: rustfmt, clippy `-D warnings`, cargo-deny licenses
  + advisories, the test matrix on Linux / macOS / Windows,
  the aislop-score badge, the SLSA L3 attestation.
- **Open**: the OpenSSF Silver + Scorecard 7.4 + crates.io /
  npm publishing plan (Wave S in progress; this file is Wave
  S.1).
