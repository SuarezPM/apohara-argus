# Changelog

All notable changes to this project are documented in this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
  [`crates/argus-mcp`](crates/argus-mcp), [Refs: 5]): new
  crate shipping a stdio JSON-RPC server with 4 tools
  (`aegis_slop`, `aegis_security`, `aegis_arch`,
  `aegis_verdict`) over the rmcp SDK. Per-call NIM key via
  the `ARGUS_NIM_KEY` env var (BYOK). Each tool returns a
  structured `SpecialistReport` envelope (specialist, prompt
  name, model id, latency, findings, summary). No persistent
  state, no daemon — short-lived process per MCP client.
- **EU AI Act Level 2 conformance** — `data_class` and
  `policy_version` on the audit record (commit `a47eabc`,
  [`crates/argus-core/src/types.rs`](crates/argus-core/src/types.rs),
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
- **MCP integration** ([`crates/argus-mcp`](crates/argus-mcp)):
  the 4 specialists exposed as MCP tools over stdio
  JSON-RPC, callable from Claude Code / Codex / Cursor.
- **Workspace scaffolding** (13 of the 14 crates are
  publish-eligible in spirit; the `publish = false` set
  covers the internal `argus-core` / `argus-crypto` /
  `argus-slop` / `argus-github` / `argus-agent` /
  `argus-otel` / `argus-benchmarks` crates per the OpenSSF
  Silver plan; the `publish = true` set is `argus-cli` /
  `argus-guard` / `argus-verify` / `argus-lens` /
  `argus-dashboard` / `argus-llm` / `argus-mcp`).
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
