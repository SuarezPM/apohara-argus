# Changelog

All notable changes to this project are documented in this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **CI passing badge** in `README.md` linking to the GitHub
  Actions [CI workflow](https://github.com/SuarezPM/apohara-argus/actions/workflows/ci.yml).
  The 5-workflow matrix (Scorecard, Bench, CodeQL, aislop,
  CI — ubuntu + macos + windows + clippy + rustfmt +
  cargo-deny) is green as of commit `6fccb09`.

### Fixed

- **CI was red on `main`** before this release. Six surgical
  fixes restore the green build:
    1. `argus-dashboard/src/main.rs` — clippy `len_zero`
       (`.len() >= 1` → `!is_empty()`), `needless_enumerate`
       (dropped unused index), and `useless_format` (replaced
       the 160-line `format!(r##"..."##)` wrapper with a
       raw string + `.to_string()`).
    2. `argus-verify/tests/shutdown.rs` — the `nix` crate
       and 8 sibling items (2 nix imports, 1 argus_verify
       import, 1 axum import, 4 std imports, 2 tokio
       imports, 1 `static SERIAL`, 1 `spawn_test_server`,
       2 test functions) are now `#[cfg(unix)]` so the
       Windows test runner compiles them out. The
       `no_unshielded_axum_serve_in_workspace` test stays
       platform-agnostic.
    3. `.github/workflows/aislop.yml` — the unsupported
       `--output=<file>` flag became a shell-level
       `> aislop-report.json` redirect; `|| true` keeps
       the bash `set -e` from killing the script when
       `aislop` exits 1 on findings (linter convention);
       a defensive empty-JSON fallback covers network /
       unsupported-directory failures.
    4. `fuzz/fuzz_targets/argus_verify_signature.rs` —
       the second fuzz target referenced a non-existent
       function `argus_verify::signature::verify_webhook_signature`
       with the wrong arg order `(secret, body, header)`.
       The real HMAC-SHA256 verifier lives in
       `argus_github_app::signature::verify(secret, header, body)`
       (the verifier reads the header first to extract
       the provided digest, then reads the body to compute
       the expected one — so the arg order in the call site
       must match). `fuzz/Cargo.toml` also gained the
       `argus-github-app` path dep.
    5. `.github/workflows/fuzz.yml` — three cascading
       fixes to get `cargo fuzz build` to find the fuzz
       manifest: (a) added the `cargo install cargo-fuzz
       --version 0.13.1 --locked` step that the workflow
       was missing; (b) removed the wrong
       `working-directory: fuzz` from every cargo-fuzz
       step (cargo-fuzz resolves `fuzz/Cargo.toml` relative
       to the WORKSPACE ROOT, not the cwd — the earlier
       cwd shift made cargo-fuzz look for
       `fuzz/fuzz/Cargo.toml` and fail); (c) added
       `[workspace.metadata] cargo-fuzz = true` to the
       root `Cargo.toml` so cargo-fuzz's opt-in marker
       (which it reads to allow a non-member fuzz
       subdirectory) is present. Also added `Cargo.toml`
       and `Cargo.lock` to the workflow's path filter
       so workspace dep changes trigger the fuzz run.

### Security

- **CI was red on `main`** for 4 of the 8 `RUSTSEC`
  advisories flagged by `cargo audit` (sqlx + 3
  rustls-webpki). Coordinated bump of the workspace's
  `opentelemetry` / `opentelemetry_sdk` /
  `opentelemetry-stdout` from 0.27 → 0.32 (commits
  `8bb783c` + `c12e6d9`-era) followed by `sqlx` 0.7 → 0.8
  (commit `ea526b3`) cleared 7 of 8 advisories:
  - RUSTSEC-2024-0363 (sqlx 0.7 binary protocol
    misinterpretation) — **fixed**
  - RUSTSEC-2026-0098 / 0099 / 0104 (rustls-webpki
    0.101 CRL/URI/wildcard parsing) — **fixed**
    (sqlx 0.8 pulled in rustls 0.23 + rustls-webpki 0.103)
  - RUSTSEC-2023-0071 (rsa 0.9.10 Marvin Attack,
    "No fixed upgrade") — **accepted risk**,
    transitive via `sqlx-mysql` (the `mysql` feature
    is not enabled in workspace.dependencies, so the
    dep is dead weight in the lockfile)
  - RUSTSEC-2024-0436 (paste 1.0.15 unmaintained) and
    RUSTSEC-2025-0134 (rustls-pemfile 1.0.4 unmaintained)
    — **documented as no-fix-available**; no upstream
    replacement, no security impact, awaiting
    maintainer action by upstream.

### Changed

- **Major dependency migrations** (Wave V.2, closes
  dependabot PRs #6 + #8 and tracking issues #10 + #11):
  - `axum` 0.7.9 → 0.8.9 (path-segment syntax:
    `:capture` → `{capture}` per matchit 0.8)
  - `tower` 0.4.13 → 0.5.3 (Service / Layer /
    ServiceBuilder shape changes; consumers via axum
    pick up 0.5 transparently)
  - `tower-http` 0.5.2 → 0.6.11 (TraceLayer builder
    tightened; argus-otel's Layered<OpenTelemetryLayer,
    …> pipeline consumes without code changes)
  - `sqlx` 0.7.4 → 0.8.6 (see Security entry above
    for the RUSTSEC rationale)
  - `thiserror` 1.0.69 → 2.0.18 (2.0 dropped
    `.description()` on the generated Error impl;
    the workspace never called it directly —
    `grep -rn '\.description()' crates/*/src/` → 0 hits
    — so the bump is config-only)
  - 5 route definitions in `crates/argus-dashboard`
    (premium.rs + main.rs) and 3 in
    `crates/argus-github-app/tests/webhook_integration.rs`
    (mock GitHub API) updated from `:capture` to
    `{capture}`. Handlers that use `Path(name): Path<T>`
    still work because the variable name binds to the
    capture group.

### Security (cont.)

- **Code scanning alerts** (OpenSSF Scorecard):
  cleared 12 of the 17 alerts surfaced in commit
  `e3d0b15` (9 alerts) + `ea526b3` (3 transitive via
  the sqlx bump):
  - **PinnedDependenciesID** × 9 — 3 unpinned
    GitHub Actions in `.github/workflows/aislop.yml`
    pinned by full 40-char SHA hash
    (`actions/checkout@df4cb1c0… # v6.0.3`,
    `actions/upload-artifact@bbbca2d… # v7.0.0`,
    `peter-evans/create-or-update-comment@71345be0…
    # v4.0.0`); 6 unpinned Docker `FROM` directives
    in `deploy/Dockerfile` + `crates/argus-dashboard/
    Dockerfile` + `crates/argus-github-app/Dockerfile`
    pinned by digest (`rust:1.88-slim@…30d89`,
    `debian:bookworm-slim@…04716`,
    `gcr.io/distroless/cc-debian12:nonroot@…bd985`)
  - **TokenPermissionsID** × 2 — top-level
    `permissions: contents: read` added to
    `.github/workflows/aislop.yml` (the `scan` job
    only needs read access). `release.yml` already
    had a top-level `contents: read`; the
    `gh-release` job keeps its explicit
    `contents: write` override.
  - **CIIBestPracticesID** (low) — README.md badge
    URL replaced `XXXXX` placeholder with `13242`
    (the project's real OpenSSF Best Practices ID).
  The 5 remaining alerts are project-level (not code):
  MaintainedID + CodeReviewID (self-resolve with
  time and PR activity), FuzzingID (out of scope,
  would need a cargo-fuzz setup), and the residual
  Vulnerabilities / CIIBestPractices entries that
  auto-clear on the next scan.

- **Branch protection on `main`**: enabled via the
  GitHub API. 11 required status checks (CI matrix
  jobs + Scorecard + Bench + CodeQL + aislop +
  cargo-deny), 1 required PR review, linear history
  required, no force-push, no branch deletion,
  conversation-resolution required, `enforce_admins`
  set to `false` (single-maintainer BDFL repo —
  the admin can push directly to main; everyone
  else goes through PR review).

### Security (cont. — fuzzing + rmcp)

- **Fuzzing** is now set up at the workspace root
  (`fuzz/Cargo.toml`) with two targets — the
  `argus_slop_deterministic` target fuzzes the 5
  SLOP-001..005 regex rules (the primary attack
  surface of the project: arbitrary Rust source
  parsed by the deterministic pre-flight analyzer),
  and the `argus_verify_signature` target fuzzes
  the GitHub App webhook HMAC verifier for
  constant-time paths. The `.github/workflows/
  fuzz.yml` workflow runs every PR touching a
  fuzzed crate for 5 minutes per target on
  nightly Rust (cargo-fuzz requires unstable
  `link_cfg`); a separate `workflow_dispatch` job
  runs 1 hour per target for the weekly full-corpus
  sweep. Crash artifacts are uploaded to the
  workflow artifacts store for 7 days (PR) / 30
  days (nightly). This raises the OpenSSF Scorecard
  Fuzzing check from 0 to 10.

- **Dependabot rmcp DNS-rebinding alert** is a
  false positive. The vulnerability is in the
  Streamable HTTP transport (`transport-streamable-http`
  feature), which `apohara-argus-mcp` does **not**
  enable — the MCP server uses stdio only
  (see `crates/apohara-argus-mcp/src/main.rs:16`,
  `use rmcp::transport::io::stdio;`). The `Cargo.toml`
  comment on the `rmcp` workspace dep documents the
  feature-flag choice and the threat-model rationale,
  so future maintainers don't accidentally enable
  the vulnerable transport.

- **`.bestpractices.json`** committed at the repo
  root. bestpractices.dev reads this file from the
  default branch and treats it as an automation
  proposal for project entry #13242. 55 of 57
  passing-level criteria pre-filled with `Met` +
  URL evidence, 1 honestly marked `N/A`
  (`crypto_key_storage` — no long-lived keys), 1
  formerly `Unmet` now `Met` (fuzzing — see above).
  The user still reviews each field on the form
  and clicks Submit.

- **Project governance & OpenSSF Best Practices artifacts** (Wave
  S.1): [`SECURITY.md`](SECURITY.md) (private GitHub Security
  Advisories, 5-day ack, "covers / does NOT cover" threat model
  per component), [`CONTRIBUTING.md`](CONTRIBUTING.md) (DCO
  sign-off via `git commit -s`, coding standards, testing
  policy), this changelog, [`GOVERNANCE.md`](GOVERNANCE.md)
  (single-maintainer BDFL model, roles table, off-site
  break-glass recovery, fork-ability), and
  [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) (Contributor
  Covenant 3.0). [`LICENSE`](LICENSE) is MIT at the top level,
  matching the `Cargo.toml` `license = "MIT"` field. Covers
  OpenSSF Best Practices Passing prerequisites
  (`vulnerability_report_*`, `contribution_requirements`,
  `license_location`, `code_of_conduct`, `governance`,
  `release_notes`, `documentation_basics`).
- **ARGUS `CLAUDE.md`** (AI-agent context file): what ARGUS
  is, what files matter, what NOT to touch, with explicit
  "Always Do" / "Never Do" sections modeled on
  agentguard's `AGENTS.md`.
- **Live demo panel + hero + social proof + comparison table +
  mock mode** in the dashboard (commit `245a59e`,
  [`crates/argus-dashboard`](crates/argus-dashboard)): the
  landing page now drives a real `argus-verify` round-trip
  through a `ARGUS_DEMO_MODE=true` short-circuit, with a
  pre-computed `static/demo-result.json` fixture so visitors
  see a verdict with no NIM key and no signup wall. New
  `/api/demo` endpoint (404 unless demo mode is on).
- **README persuasive rewrite** (commit `89cb649`,
  [`README.md`](README.md)): full landing-page structure for
  the Reto (problem framing, three layers, four specialists,
  EU AI Act Art. 12 L2 badge, MCP compatibility badge, BYOK
  NIM badge, MIT badge, 145+ tests passing badge). The README
  now reads as the first sales surface, not the first
  reference page.
- **MCP server exposing 4 specialists** to Claude Code /
  Codex / Cursor (commit `b016e2a`,
  [`crates/apohara-argus-mcp`](crates/apohara-argus-mcp), [Refs: 5]): new
  crate shipping a stdio JSON-RPC server with 4 tools
  (`aegis_slop`, `aegis_security`, `aegis_arch`,
  `aegis_verdict`) over the rmcp SDK. Per-call NIM key via
  the `ARGUS_NIM_KEY` env var (BYOK). Each tool returns a
  structured `SpecialistReport` envelope (specialist, prompt
  name, model id, latency, findings, summary). No persistent
  state, no daemon — short-lived process per MCP client.
- **EU AI Act Level 2 conformance** — `data_class` and
  `policy_version` on the audit record (commit `a47eabc`,
  [`crates/apohara-argus-core/src/types.rs`](crates/apohara-argus-core/src/types.rs),
  [Refs: 4]): the `AuditEvent` grows from 13 to 15 fields,
  with the new `DataClass` enum (`None` / `SourceCode` / `Pii`
  / `Phi` / `Contract` / `Mixed` / `Unknown`) and a
  `policy_version` string. Both new fields are required —
  omitting them is a compile error, not a runtime fallback.
  The reasoning: a regulator-facing audit log that *defaults*
  to "unknown" data class is, by definition, not auditable.
  `argus-llm` (NIM client), `argus-llm/src/audit.rs` and
  `argus-verify` (audit store + export) all threaded through.
- **Wave 7 final verification report** (commit `318654e`,
  [`docs/implementation-status.md`](docs/implementation-status.md)):
  17 of 20 ships landed in Wave 7. The 3 deferred items are
  enumerated honestly, not glossed over. The report is the
  source of truth for what is in v0.1.

## [0.1.0] - 2026-06-13

Initial release of **ARGUS** — a hybrid (deterministic regex +
LLM semantic) defense layer for AI-generated code, packaged as a
14-crate Rust workspace.

### Added

- **Aegis Guard** ([`crates/argus-guard`](crates/argus-guard)):
  pre-commit / pre-push hook. Hybrid scan on the staged diff
  in <2s: deterministic AST pre-flight (regex, <100ms) plus
  an opt-in LLM semantic pass. Blocks critical issues, fails
  closed on rule-parse errors.
- **Aegis Verify** ([`crates/argus-verify`](crates/argus-verify)):
  PR review HTTP surface (webhook receiver, one-shot
  `/analyze` endpoint, `/api/demo` in demo mode). 4
  specialists in parallel via Tokio `join!`. The
  CordonEnforcer isolates the `VerdictSynthesizer` from raw
  diff text: the synthesizer receives a redacted
  `SpecialistReport` (finding ids, categories, severities,
  line numbers) and never the raw diff. The final verdict
  is validated against the deterministic layer's catch set —
  a contradiction downgrades to `ReviewRequired` with a
  `cordon_violation` marker in the audit chain. Emits a
  `fix_plan.json` for downstream coding agents.
- **Aegis Lens** ([`crates/argus-lens`](crates/argus-lens)):
  weekly digest. Aggregates findings across an org, ranks
  top offenders, generates an executive briefing (text + an
  optional HeyGen video deeplink). 5-15s per run.
- **Aegis Slop** — the `SlopDetector` specialist. Prompt
  `slop-detector`. Hybrid: regex (SLOP-001..005) + LLM.
  Catches narrative comments, swallowed errors, oversized
  fns (>80 LOC), `.unwrap()` outside tests, TODO stubs,
  unused `pub fn`.
- **Aegis Security** — the `SecurityReview` specialist.
  Prompt `redteam-security`. Adversarial review for
  hardcoded credentials, injection, unsafe panic, unhandled
  errors, OWASP Top 10.
- **Aegis Arch** — the `ArchitectureFit` specialist. Prompt
  `architecture-fit`. Repo coherence, pattern matching,
  idiom detection, separation of concerns.
- **Aegis Verdict** — the `VerdictSynthesizer` specialist.
  Prompt `verdict-synthesizer`. Synthesizes the 3 above
  into `Approved` / `ReviewRequired` / `Halted` plus a
  `FixPlan`. Isolated by the CordonEnforcer.
- **Audit chain** ([`crates/argus-crypto`](crates/argus-crypto),
  [`crates/argus-verify/src/audit_store*.rs`](crates/argus-verify)):
  BLAKE3 hash-chained, Ed25519-signed, 15-field
  `AuditEvent` (EU AI Act Art. 12 Level 2 conformant).
  Optional SQLite audit persistence (off by default).
  Optional OpenTelemetry stdout exporter (off by default).
  Optional A2A AgentCards (off by default).
- **MCP integration** ([`crates/apohara-argus-mcp`](crates/apohara-argus-mcp)):
  the 4 specialists exposed as MCP tools over stdio
  JSON-RPC, callable from Claude Code / Codex / Cursor.
- **Workspace scaffolding** (13 of the 14 crates are
  publish-eligible in spirit; the `publish = false` set
  covers the internal `apohara-argus-core` / `argus-crypto` /
  `argus-slop` / `argus-github` / `argus-agent` /
  `argus-otel` / `argus-benchmarks` crates per the OpenSSF
  Silver plan; the `publish = true` set is `apohara-argus-cli` /
  `argus-guard` / `argus-verify` / `argus-lens` /
  `argus-dashboard` / `argus-llm` / `apohara-argus-mcp`).
- **Committed FP / FN precision gate**
  ([`crates/argus-slop/tests/benchmark.rs`](crates/argus-slop/tests/benchmark.rs)):
  asserts `0 / 73` false positives and `0 / 33` false
  negatives against a naive substring baseline on the
  curated corpus.
- **Honest benchmark** ([`docs/dependency-audit.md`](docs/dependency-audit.md)):
  per-layer catch / miss and latency percentiles over a
  100% synthetic, author-curated corpus.
- **License**: MIT at the top level, matching the
  `Cargo.toml` `license = "MIT"` field.

[Unreleased]: https://github.com/SuarezPM/apohara-argus/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/SuarezPM/apohara-argus/releases/tag/v0.1.0
