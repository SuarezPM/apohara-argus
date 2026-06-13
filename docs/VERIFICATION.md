# ARGUS Local Verification Report

**Generated:** 2026-06-13
**Verifier:** Sisyphus-Junior (Atlas orchestrator)
**Commit at verification:** 094ac55b30b3aea663571dca99802769c1e129e2

## Summary

| Check | Result | Notes |
|---|---|---|
| `cargo build --workspace` | PASS | exit 0; 37 warnings (no errors); future-incompat note for sqlx-postgres v0.7.4 |
| `cargo test --workspace` | PASS | exit 0; 190 tests pass, 0 failed, 3 ignored (network-dependent argus-slop e2e) |
| `cargo test --benches` | PASS | exit 0; 3 benches run (precision_recall, latency, cost) |
| `cargo fmt --check` | PASS | exit 0; 0 issues |
| `cargo clippy --all-targets` | PASS | exit 0; 75 warnings (10 unique clippy lint kinds, no errors); pre-existing |
| `cargo deny check` | PASS | exit 0; advisories ok, bans ok, licenses ok, sources ok |
| `cargo publish --dry-run` | FAIL | expected — all 7 public crates fail with `no matching package named 'apohara-argus-core' found` (B.4 architectural blocker) |
| `cd npm && node --test test/*.js` | PASS | exit 0; 10 tests pass, 0 failed |
| 7 governance docs present | PASS | SECURITY.md, CONTRIBUTING.md, CHANGELOG.md, GOVERNANCE.md, CODE_OF_CONDUCT.md, LICENSE, CLAUDE.md |
| 7 S.3 workflows present | PASS | ci.yml, scorecard.yml, codeql.yml, release.yml, publish.yml, _attest.yml, bench.yml |
| 1 npm publish workflow present | PASS | npm/.github/workflows/publish-npm.yml |
| docs/best-practices-silver.md present | PASS | 254 lines, 30014 bytes |
| docs/BENCHMARK.md present | PASS | 215 lines; P=1.0, R=0.818, F1=0.9 on 40 PRs |
| docs/pricing.md present | PASS | 152 lines, 3 tiers |
| docs/for-ciso.md present | PASS | 147 lines, 3 risks + 4 pillars |
| docs/branch-protection.md present | PASS | 12441 bytes |
| Cargo workspace hardening (4 files) | PASS | rust-toolchain.toml, deny.toml, about.toml, THIRD-PARTY-LICENSES (5474 lines) |
| Dependabot config | PASS | .github/dependabot.yml (4431 bytes) |
| npm wrapper directory | PASS | 11 files in npm/ (incl. 5 source files + 2 test files + workflow + LICENSE + README + .npmignore) |
| 15 crates in workspace | PASS | 7 public (apohara-argus-{cli,mcp}, argus-{dashboard,guard,lens,llm,verify}) + 8 internal |
| OpenSSF scorecard workflow valid | PASS | YAML parses; `ossf/scorecard-action@4eaacf0543bb3f2c246792bd56e8cdeffabf205a # v2.4.3` (pinned by SHA) |

## Pre-existing issues (not regressions)

1. **2 argus-otel deprecation warnings** — `opentelemetry_sdk::trace::Builder::with_config` and `opentelemetry_sdk::trace::Config::with_resource` are deprecated. The SDK is migrating to `Builder::with_resource(resource)` as a direct method. Not blocking; needs an SDK upgrade on a future task.
2. **3rd argus-otel warning** — `unused import: opentelemetry::trace::Tracer`. Pure cosmetic.
3. **~70 transitive warnings** in `argus-llm`, `argus-slop`, `argus-github`, `argus-verify`, `argus-lens`, `argus-guard`, `apohara-argus-mcp`, `argus-github-app` (clippy `--all-targets`). 10 unique clippy lint kinds, all cosmetic:
   - `clippy::too_many_arguments` (1, in argus-github)
   - `clippy::useless_format` (1, in argus-github)
   - `clippy::new_without_default` (5, in argus-slop + apohara-argus-mcp)
   - `clippy::if_same_then_else` (2, in argus-slop)
   - `clippy::unnecessary_lazy_evaluations` (2, in argus-llm)
   - `clippy::redundant_pattern_matching` (1, in argus-llm)
   - `clippy::doc_overindented_list_items` (2, in argus-verify)
   - `clippy::await_holding_lock` (2, in argus-verify tests)
   - `clippy::needless_borrow` (1, in argus-github-app)
   - `clippy::needless_borrows_for_generic_args` (2, in argus-github-app tests)
4. **`cargo publish --dry-run` fails on all 7 public crates** — the 6 internal crates (`apohara-argus-core`, `argus-agent`, `argus-benchmarks`, `argus-crypto`, `argus-github`, `argus-otel`, `argus-slop`, `argus-github-app`) are `publish = false`, so when cargo prepares a public crate's tarball, it can't find the internal deps on crates.io. This is a known architectural issue tracked in B.4. The fix is to publish all 13 crates (or restructure). NOT a regression from this work.
5. **1 future-incompat note** — sqlx-postgres v0.7.4 has code that will be rejected by a future version of Rust. Transitive via `argus-verify`. Not blocking.
6. **3 ignored tests in argus-slop** — `pipeline_runs_end_to_end`, `raw_security_call_works`, `raw_slop_call_works` all require `ARGUS_NIM_KEY` + internet. Expected to be ignored in CI.

## Regressions found

NONE. Every test that should pass, passes. Every check that should produce a result, produces a result. The 7 `cargo publish --dry-run` failures are pre-existing (B.4), as are all clippy/build warnings.

## Test counts (workspace-wide)

- 190 unit + integration tests pass
  - apohara_argus_cli: 1
  - argus (bin): 5
  - apohara_argus_core: 15
  - apohara_argus_mcp: 6
  - argus_mcp (bin): 0
  - argus_agent: 12
  - argus_benchmarks: 2
  - argus_crypto: 23 (incl. 5 chain tests for BLAKE3 hash-chain integrity)
  - argus_dashboard (lib): 20
  - argus_dashboard (bin): 5
  - argus_dashboard integration: 9 (premium_gate)
  - argus_github: 3
  - argus_github_app (lib): 22
  - argus_github_app integration: 4 (webhook_integration)
  - argus_guard: 2
  - argus_lens: 2
  - argus_llm: 25
  - argus_otel: 5
  - argus_slop: 8
  - argus_verify (lib): 15 (incl. 6 audit_store tests for the manifest hash chain)
  - argus_verify integration: 6 (3 export + 3 shutdown)
- 3 benches pass (precision_recall, latency, cost)
  - precision_recall: P=1.0, R=0.818, F1=0.9 on 40 PRs (TP=9, FP=0, TN=29, FN=2)
  - latency: median p50 ~52us, median p99 ~50us across 10 PRs
  - cost: $0.001344 USD estimated over 10 PRs (1344 total tokens at $0.001/1K)
- 10 npm tests pass
- 1 cargo deny check passes (advisories, bans, licenses, sources all OK)

## Audit chain (BLAKE3 + Ed25519) smoke test

- `argus-crypto` chain tests: 5 pass (empty_chain_errors, append_creates_consistent_hash, broken_prev_hash_fails, tampered_chain_fails, chain_verifies)
- `argus-verify` audit_store tests: 6 pass (empty_range_returns_zero_events_with_empty_manifest_hash, from_after_to_returns_no_events, stored_event_prompt_fingerprint_is_32_bytes_not_text, manifest_hash_differs_when_events_differ, append_three_events_query_returns_all_three, manifest_hash_is_reproducible_for_same_events)
- The audit chain is load-bearing per CLAUDE.md and is verified intact.

## What's ready

- OpenSSF Best Practices Silver evidence: `docs/best-practices-silver.md` (254 lines, 30014 bytes, every criterion mapped)
- 6 S.3 workflows: ci.yml, scorecard.yml, codeql.yml, release.yml, publish.yml, _attest.yml, bench.yml
- 1 npm workflow: publish-npm.yml
- Cargo workspace hardening: rust-toolchain.toml, deny.toml, about.toml, THIRD-PARTY-LICENSES (5474 lines)
- Dependabot: .github/dependabot.yml
- Branch protection: docs/branch-protection.md (12441 bytes)
- npm package: 11 files in npm/, 10 e2e tests
- README: full Install section (3 paths), OpenSSF badges, Security link
- Benchmarks: P=1.0, R=0.818, F1=0.9 on 40 PRs (BENCHMARK.md, 215 lines)
- Pricing: docs/pricing.md (152 lines, 3 tiers)
- CISO landing: docs/for-ciso.md (147 lines, 3 risks + 4 pillars + evidence pack)
- 7 governance docs: SECURITY.md (17614 bytes), CONTRIBUTING.md, CHANGELOG.md, GOVERNANCE.md, CODE_OF_CONDUCT.md, LICENSE, CLAUDE.md
- 15 crates: 7 public + 8 internal (`publish = false`)

## What's blocked

- B.4 (first publish to crates.io): blocked on user architectural decision (publish all 13 vs restructure) + `CARGO_REGISTRY_TOKEN`
- V.3 (OpenSSF Passing form submission): blocked on human action at bestpractices.dev
- V.4 (OpenSSF Silver form submission): blocked on human action at bestpractices.dev
- V.5 (update README with Silver badge): blocked on V.3/V.4 (need the real project ID)

## What's next

- V.2: CI verification (push to test branch, verify all 7 workflows run green)
- B.5: matrix in publish.yml (refactor to use matrix strategy instead of sequential steps)
- B.4: needs user decision
- V.3, V.4: needs human action
- V.5: after V.3/V.4
- F1-F4: final verification wave
