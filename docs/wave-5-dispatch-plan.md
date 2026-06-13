# Wave 5 Dispatch Plan — ARGUS Iteration

> **Status of dependencies (snapshot 2026-06-12):** Items 6.3 / 3.2 / 1.2 / 6.4 / 7.1 are pending. They can be dispatched as soon as the in-flight Wave 4 tasks complete and their files are unblocked.

## File-conflict matrix (who touches what)

| Item | Crates touched | Conflicts with |
|------|----------------|------------------|
| **6.3 OpenTelemetry** | argus-verify/main.rs, apohara-argus-cli/main.rs, argus-dashboard/main.rs | 1.1 (dashboard), 2.2 (verify), 6.2 (verify) — all currently in flight |
| **3.2 A2A AgentCards** | argus-verify/main.rs, Cargo.toml | 2.2, 6.2 (verify) |
| **1.2 Agent hand-off** | argus-verify/lib.rs, apohara-argus-core/types.rs | 2.2 (verify), but the types.rs add is independent |
| **6.4 SQLite persistence** | argus-verify/main.rs (new store), Cargo.toml | 2.2 (verify), depends on 2.2 completion |
| **7.1 HeyGen deeplink** | argus-dashboard/main.rs | 1.1 (dashboard) |

**Dispatch order (after deps clear):**
1. **1.2 first** (touches apohara-argus-core/types.rs + argus-verify/lib.rs — but types.rs is independent; verify/lib.rs can be edited after 2.2 commits)
2. **3.2 second** (after 2.2, before 6.4)
3. **6.4 third** (after 2.2 schema confirmed; before 6.3)
4. **7.1 fourth** (after 1.1 cohort view committed; 1h task)
5. **6.3 last** (largest; touches 3 mains, wait for all 1.1/2.2/3.2 to commit first)

---

## Item 1.2 — Agent hand-off `fix_plan.json`

**Category:** `unspecified-high`
**Skills:** none
**Files:**
- `crates/apohara-argus-core/src/types.rs` — add `FixPlan`, `FixStep`, `FixStepKind` enums + structs
- `crates/argus-verify/src/lib.rs` — populate FixPlan in `analyze()` after verdict synthesis
- `crates/argus-verify/src/handler.rs` (or wherever the response is built) — include `fix_plan` in `AnalyzeResponse`

**Definition of done:**
- `FixPlan { steps: Vec<FixStep> }` with `FixStep { kind, file, line_range, description, suggested_code }`
- 4+ tests: severity-order sort, empty case, JSON roundtrip, Claude-Code-compatible schema
- Commit: `feat(verify): agent hand-off fix_plan.json output [Refs: 1.2]`

---

## Item 3.2 — A2A AgentCards opt-in

**Category:** `unspecified-high`
**Skills:** none
**Files:**
- `Cargo.toml` (workspace) — add `a2a-rust = "0.1"` (per W0-1: 24 downloads, single author; risk: use with `default-features = false, features = ["client"]` to avoid axum 0.8 collision)
- `crates/argus-verify/Cargo.toml` — `a2a-rust = { workspace = true }`
- `crates/argus-verify/src/routes.rs` — add `GET /.well-known/agent-card.json` and `POST /a2a/message` routes (env-gated via `ARGUS_A2A_DISABLED`)

**Definition of done:**
- AgentCard JSON shape: 5 specialists as `skills`
- 3+ tests: AgentCard shape, A2A message roundtrip, opt-out 404
- Commit: `feat(verify): A2A AgentCards opt-in (3.2)`

---

## Item 6.4 — SQLite persistence

**Category:** `unspecified-high`
**Skills:** debugging
**Files:**
- `Cargo.toml` (workspace) — add `sqlx = { version = "0.9", features = ["runtime-tokio", "tls-rustls", "sqlite", "migrate"] }` (per W0-1 split: NOT `runtime-tokio-rustls`)
- `crates/argus-verify/Cargo.toml` — `sqlx = { workspace = true }`
- `crates/argus-verify/src/audit_store_sqlite.rs` (new module) — `SqliteAuditStore` impl
- `crates/argus-verify/src/main.rs` — wire SqliteAuditStore on startup, replace InMemoryAuditStore

**Definition of done:**
- Schema migration runs idempotently
- 3+ tests: write+restart+read, WAL recovery, idempotent migration
- Commit: `feat(verify): SQLite audit log persistence (6.4)`

---

## Item 7.1 — HeyGen deeplink (NOT integration)

**Category:** `quick`
**Files:**
- `crates/argus-dashboard/src/main.rs` — add `<a href="https://app.heygen.com/video-translate?script=...">` button

**Definition of done:**
- Deeplink generated from script text
- 3+ tests: href present in HTML, no server-side HTTP call
- Commit: `feat(dashboard): HeyGen deeplink (7.1)`

---

## Item 6.3 — OpenTelemetry (largest, dispatch last)

**Category:** `unspecified-high`
**Skills:** debugging
**Files:**
- `Cargo.toml` (workspace) — add `tracing-opentelemetry = "0.33"`, `opentelemetry = "0.27"`, `opentelemetry-otlp = "0.27"` (stdout exporter)
- `crates/argus-verify/src/main.rs` — init tracer
- `crates/apohara-argus-cli/src/main.rs` — init tracer
- `crates/argus-dashboard/src/main.rs` — init tracer
- `.env.example` — add OTEL env vars

**Definition of done:**
- 3+ tests: span emitted with attrs, service name propagates, `OTEL_SDK_DISABLED=true` zero-overhead
- Commit: `feat(observe): OpenTelemetry OTLP stdout exporter (6.3)`

---

## Risk register for Wave 5

- **6.3 risk:** OTel touches 3 mains, all touched by other in-flight items. HIGH collision risk. Dispatch last.
- **3.2 risk:** `a2a-rust` axum 0.8 collision (per W0-1). Use `default-features = false, features = ["client"]` to avoid.
- **6.4 risk:** `sqlx` 0.9 MSRV=1.94; need to bump workspace MSRV. Could conflict with 8.2's MSRV bump.
- **1.2 risk:** Low — types.rs is independent of verify/lib.rs (which 2.2 is editing).
- **7.1 risk:** Trivial (1h). But 1.1 cohort view is editing the same file.

---

## Wave 6 (deferred, dispatch after Wave 5)

- **4 EU AI Act Level 2** (deep, 20h) — `certifieddata/ai-decision-logging-spec` conformance, validators
- **5 MCP server** (deep, 24h) — new `crates/apohara-argus-mcp/` workspace member
- **7.2 BYVK opt-in** (unspecified-high, 8h) — feature flag-gated HeyGen/D-ID integration

## Wave 7 (final)

- Full test suite
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo build --release` (4 binaries)
- Update roadmap checkmarks
- Final commit
