# 🛡️ ARGUS -- The verification layer for AI-generated code

> **AI generates code at near-zero cost. Human review didn't get faster. The bottleneck inverted: it's no longer generation -- it's verification.**
>
> ARGUS is the verification infrastructure. **15 Rust crates, 4 specialists, an audit chain that's BLAKE3-hash-chained and Ed25519-signed -- EU AI Act Art. 12 Level 2 ready by default.** MIT licensed. BYOK. Zero SaaS lock-in.

[![CI](https://github.com/SuarezPM/apohara-argus/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/SuarezPM/apohara-argus/actions/workflows/ci.yml)
[![aislop score](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/SuarezPM/apohara-argus/main/aislop-score.json)](https://github.com/SuarezPM/apohara-argus/actions/workflows/aislop.yml)
![Rust 100%](https://img.shields.io/badge/rust-100%25-orange?logo=rust)
![EU AI Act Art.12](https://img.shields.io/badge/EU%20AI%20Act-Art.%2012%20L2%20ready-blue)
![MCP compatible](https://img.shields.io/badge/MCP-Claude%20Code%2FCodex%2FCursor-green)
![BYOK](https://img.shields.io/badge/BYOK-NVIDIA%20NIM-76b900)
![License: MIT](https://img.shields.io/badge/license-MIT-lightgrey)
![Tests 194+](https://img.shields.io/badge/tests-194%2B%20passing-brightgreen)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/SuarezPM/apohara-argus/badge)](https://scorecard.dev/viewer/?uri=github.com/SuarezPM/apohara-argus)
[![OpenSSF Best Practices](https://www.bestpractices.dev/projects/XXXXX/badge)](https://www.bestpractices.dev/projects/XXXXX)
[Install](#-install) · [Quickstart](#-quickstart) · [Why](#-why-this-exists) · [What](#-what-argus-is) · [Numbers](#-the-numbers) · [Pricing](docs/pricing.md) · [Security](SECURITY.md)

---

## The problem is here. Now.

**Open Source is dying in 2026.** La confianza comunitaria se ahoga ante un +206% de scripts de Bash en proyectos AI¹, revisiones de PRs **4.6× más lentas**² y **15-18% más de vulnerabilidades**². Con **42% del código commiteado hoy siendo AI-generated o AI-assisted**³ y el **96% de los devs desconfiando de él**³, el AI slop -- *Palabra del Año 2025*⁴ -- ha forzado medidas extremas:

| Project | Response | Date |
|---|---|---|
| 🌐 **Ladybird** (browser) | Cerró sus PRs públicas. *"We will no longer accept public pull requests."* | Jun 2026⁵ |
| 🎨 **tldraw** (whiteboard) | Auto-close de PRs externas. *"Open source contribution has always been a gift economy held together by proof of work. AI has changed that."* | Jan 2026⁶ |
| 🎮 **RPCS3** (PS3 emulator) | Tuvo que **revertir múltiples PRs AI que causaron regresiones en producción**. | May 2026⁷ |
| 🌐 **cURL** (web infrastructure) | **Canceló su bug bounty** porque 19 de cada 20 reportes eran alucinaciones sintéticas. | Jan 2026⁸ |

**Fuentes:** ¹[GitHub Octoverse 2025](https://github.blog/ai-and-ml/generative-ai/how-ai-is-reshaping-developer-choice-and-octoverse-data-proves-it/) · ²[Opsera 2026 AI Coding Impact Report](https://opsera.ai/resources/report/ai-coding-impact-2026-benchmark-report/) · ³[Sonar State of Code Developer Survey 2026](https://www.sonarsource.com/blog/state-of-code-developer-survey-report-the-current-reality-of-ai-coding) · ⁴[Merriam-Webster Word of the Year 2025](https://www.globenewswire.com/news-release/2025/12/15/3205236/0/en/Merriam-Webster-Announces-Slop-as-the-2025-Word-of-the-Year.html) · ⁵[Ladybird blog](https://linuxiac.com/ladybird-browser-closes-public-pull-requests-ahead-of-first-alpha/) · ⁶[tldraw issue #7695](https://github.com/tldraw/tldraw/issues/7695) · ⁷[RPCS3 commit c0b3580](https://github.com/RPCS3/rpcs3/commit/c0b358003f813e28d7902cd65251c3506847619a) · ⁸[Daniel Stenberg, "The end of the curl bug-bounty"](https://daniel.haxx.se/blog/2026/01/26/the-end-of-the-curl-bug-bounty/)

> 🤖 *AI slop is a tragedy of the commons* ([arXiv:2603.27249](https://arxiv.org/abs/2603.27249)): individual productivity gains externalize costs onto reviewers and maintainers. **The bottleneck isn't generation. It's verification.**

---

## 💡 What ARGUS is

ARGUS = **AI Review & Governance for Undermining Slop** -- the trust layer for AI-generated code.

**One product. Three layers. Four specialists. One signed certificate per analysis.**

Built for **engineering managers, OSS maintainers, and CISOs** who need an audit-grade, EU AI Act-ready answer to the verification bottleneck. Pure Rust (15 crates, zero Python, zero Node.js in production). BYOK (your NVIDIA NIM key, never persisted). MIT licensed.

---

## 🛡️ The 3 layers (one worker each)

| Worker | When it runs | What it does | Latency |
|---|---|---|---|
| **Aegis Guard** | Pre-commit / pre-push | Hybrid scan on the staged diff: deterministic AST pre-flight (5 SLOP rules, regex, <100ms) + LLM semantic. Blocks critical issues. | <2s |
| **Aegis Verify** | PR review (webhook or one-shot) | 4 specialists in parallel via Tokio `join!` + **CordonEnforcer** (synthesizer never sees raw code). Emits a `fix_plan.json` for downstream coding agents. | 4-8s |
| **Aegis Lens** | Weekly digest | Aggregates findings across an org, ranks top offenders, generates an executive briefing (text + optional HeyGen video). | 5-15s |

---

## 🤖 The 4 specialists (run in parallel inside Verify)

| Specialist | Prompt | What it catches | Hybrid? |
|---|---|---|---|
| **Aegis Slop** | `slop-detector` | Narrative comments, swallowed errors, oversized fns (>80 LOC), `.unwrap()` outside tests, TODO stubs, unused `pub fn` | ✅ regex + LLM |
| **Aegis Security** | `redteam-security` | Hardcoded credentials, injection, unsafe panic, unhandled errors, OWASP Top 10 | LLM |
| **Aegis Arch** | `architecture-fit` | Repo coherence, pattern matching, idiom detection, separation of concerns | LLM |
| **Aegis Verdict** | `verdict-synthesizer` | Synthesizes the 3 above into Approved/ReviewRequired/Halted + `FixPlan` | LLM |

> **CordonEnforcer is the moat:** the verdict synthesizer in the pipeline **never sees raw code**. It only sees the structured outputs of the other three specialists. No competitor (CodeRabbit, Greptile, Qodo) has this constraint.

---

## ✨ The 7 things that make ARGUS different

### 1. Hybrid detection -- cheap + deep

```
SLOP-001 oversized fn (size)         ─► regex  < 1ms     catches 40-60% of slop
SLOP-002 swallowed error arm          ─► regex  < 1ms
SLOP-003 TODO stub                    ─► regex  < 1ms
SLOP-004 unwrap/expect outside tests  ─► regex  < 1ms
SLOP-005 unused pub fn                ─► regex  < 1ms
  + semantic reasoning               ─► LLM    2-4s     catches the rest
```

No competitor has this combination. The result: **60-80% LLM cost reduction** on typical PRs. **Measured: P=1.000, R=0.818, F1=0.900 on 40-PR benchmark** ([BENCHMARK.md](docs/BENCHMARK.md)).

### 2. EU AI Act Article 12 Level 2 ready by default

The 16-field `AuditEvent` is automatically emitted on every LLM call:

```json
{
  "audit_id": "...",
  "timestamp": "2026-06-12T19:00:00Z",
  "model_id": "deepseek-ai/deepseek-v4-flash",
  "prompt_template_version": "abc123",
  "prompt_fingerprint": "BLAKE3 hex (GDPR-safe)",
  "response_fingerprint": "BLAKE3 hex",
  "data_class": "source_code",
  "policy_version": "verify-worker-v1-policy",
  "decision": { "verdict": "warn", "findings_count": 2, "rationale": "..." },
  "prev_hash": "...", "signature": "Ed25519 hex"
}
```

Verifiable: `curl /audit/export?from=2026-01-01&to=2026-12-31` returns NDJSON with a BLAKE3 manifest footer. **No cleartext prompts, ever.** GDPR derivative-liability-safe by construction. Enforcement starts **Aug 2, 2026** -- 51 days from this README.

### 3. MCP server for Claude Code / Codex / Cursor

```json
// ~/.config/claude-code/mcp.json
{
  "mcpServers": {
    "argus": {
      "command": "argus-mcp",
      "env": { "ARGUS_NIM_KEY": "nvapi-..." }
    }
  }
}
```

Four tools land in your agent's toolbox:
- `aegis_slop` → AI slop signals
- `aegis_security` → adversarial review
- `aegis_arch` → architectural fit score
- `aegis_verdict` → final verdict + FixPlan

Your coding agent now has ARGUS on tap. It can run a slop check, a security check, and a verdict on its own draft PR -- **automatically, before it ever asks for human review**.

### 4. A2A AgentCards -- discoverable to Google's open protocol

```
GET /.well-known/agent-card.json
GET /a2a/message
```

Opt-in via `ARGUS_A2A_DISABLED=false`. Google A2A orchestrators can discover and message our 4 specialists.

### 5. BYOK economics -- $0.05/dev/month

- User provides the NVIDIA NIM key (`X-LLM-Key` header or `ARGUS_NIM_KEY` env)
- No telemetry, no tracking, no per-seat fees
- We don't see your diffs -- they go directly from your process to NIM
- 100× cheaper than CodeRabbit ($0.10-0.50/PR) at scale

### 6. Production resilience out of the box

- **LLM circuit breaker** with full-jitter exponential backoff (rolled our own, no `llm-retry` dep)
- **Idempotency-Key** support on `POST /analyze` (24h TTL)
- **Graceful shutdown** on SIGINT/SIGTERM (Axum `with_graceful_shutdown`)
- **OpenTelemetry** stdout exporter (env-gated via `ARGUS_OTEL_DISABLED`)
- **SQLite** audit persistence (`InMemoryAuditStore` for ephemeral, `SqliteAuditStore` for durable)

### 7. Pure Rust 100%, MSRV 1.88

- 15 crates, 4 binaries
- **194 tests passing** (no flaky)
- `cargo build --release` in 1m 27s
- Zero Python, zero Node.js in the production binary
- `RUSTFLAGS="-D warnings" cargo test` is the CI gate

---

## 🏛️ Architecture

```
                         ┌─────────────────────────────────────┐
                         │  GitHub PR / commit / org scan      │
                         └──────────────┬──────────────────────┘
                                        │
                                        ▼
        ┌───────────────────────────────────────────────────────────────┐
        │                       ARGUS -- Three Layers                    │
        │                                                               │
        │   Aegis Guard       Aegis Verify       Aegis Lens            │
        │   (pre-commit)  ──► (PR review)   ──► (weekly digest)       │
        │   <2s             4-8s              5-15s                    │
        │                   │                                            │
        │                   ▼                                            │
        │          4 specialists in parallel                            │
        │          (slop, security, arch, verdict)                      │
        │                   │                                            │
        │                   ▼                                            │
        │   ┌───────────────────────────────────────────────────┐       │
        │   │  AuditEvent (16 fields) -- EU AI Act L2 ready      │       │
        │   │  BLAKE3 chain + Ed25519 signature + BLAKE3 NDJSON  │       │
        │   │  manifest at /audit/export                          │       │
        │   └───────────────────────────────────────────────────┘       │
        │                       │                                         │
        │                       ▼                                         │
        │   SQLite (in-process)  ◄──►  Supabase Postgres (remote, opt.)  │
        └───────────────────────────────────────────────────────────────┘
                                        │
                                        ▼
                ┌───────────────────────────────────┐
                │  Dashboard  (axum + htmx + SSR)    │
                │  Weekly briefings (HeyGen deeplink)│
                │  Cohort view (CodeRabbit-style)   │
                │  + /audit/export for regulators    │
                └───────────────────────────────────┘

        External:
        ───────
        MCP server (apohara-argus-mcp) ──► Claude Code / Codex / Cursor
        A2A AgentCards         ──► Google A2A orchestrators
```

---

## 📦 Install (30 seconds)

Pick the path that matches your environment. All three ship the same MIT-licensed core.

| Path | Command | What you get |
|---|---|---|
| **npm** (no Rust needed) | `npx @apohara/argus --help` | The CLI + the MCP server. Downloads the right binary on first run. |
| **cargo** (Rust toolchain) | `cargo install apohara-argus-cli` | Just the CLI. Faster startup, no download step. |
| **Docker** | `docker run -e ARGUS_NIM_KEY=$YOUR_NIM_KEY SuarezPM/apohara-argus --help` | Full containerized ARGUS, no host dependencies. |

### Build from source

```bash
git clone https://github.com/SuarezPM/apohara-argus
cd apohara-argus
cargo build --release
./target/release/argus --help
```

### Verify the install

```bash
npx @apohara/argus health
# or
argus health
# or
docker run -e ARGUS_NIM_KEY=$YOUR_NIM_KEY SuarezPM/apohara-argus health
```

---

## 🚀 Quickstart (90 seconds end-to-end)

```bash
git clone https://github.com/SuarezPM/apohara-argus.git
cd apohara-argus

# 1. Get a free NVIDIA NIM key at https://build.nvidia.com/
export ARGUS_NIM_KEY=nvapi-xxx

# 2. Build everything (pure Rust, MSRV 1.88, ~1m 27s on a modern laptop)
cargo build --release

# 3. Pre-commit guard on a local diff
echo "+ user.password = 'hunter2'" | cargo run -p apohara-argus-cli -- guard --diff -

# 4. PR review (one-shot, with the 4 specialists)
cargo run -p apohara-argus-cli -- verify --pr-url https://github.com/owner/repo/pull/42

# 5. Weekly digest for an org
cargo run -p apohara-argus-cli -- lens --org acme --mock-prs "acme/api#1,acme/web#2"

# 6. Start the dashboard (SSR, port 3000)
cargo run -p argus-dashboard

# 7. Start the MCP server (for Claude Code / Codex)
cargo run -p apohara-argus-mcp

# 8. Verify EU AI Act compliance (BLAKE3 chain + manifest)
curl http://localhost:8080/audit/export?from=2026-01-01 | tail -1
# → { "# manifest: { "count": 47, "b3_hash": "...", ... } }
```

---

## 📊 The numbers

Numbers we **measured** ([BENCHMARK.md](docs/BENCHMARK.md)), not promised:

| Metric | Value | Why it matters |
|---|---|---|
| **Precision** | **1.000** on 40-PR dataset | Zero false positives on the deterministic layer |
| **Recall** | **0.818** on 40-PR dataset | Catches 82% of AI-slop patterns; the 2 FNs are documented as rule-scope gaps |
| **F1 score** | **0.900** | Above the 0.70 plan target |
| **Deterministic slop pass** | <100ms on 10k LOC | 60-80% of LLM cost saved |
| **`cargo build --release`** | 1m 27s | Fast iteration |
| **Tests** | **194** passing | Boring reliable |
| **Per-dev cost** | $0.05/month (BYOK) | 100× cheaper than CodeRabbit at scale |
| **EU AI Act Art. 12** | Level 2 ready | Regulators can verify via `curl /audit/export` |
| **Crates** | 15 | 4 binaries |
| **MSRV** | 1.88 | Compatible with stable Rust 2024 |
| **Pure Rust** | 100% | No Python, no Node.js in production |

---

## 🆚 Comparison

| | **ARGUS** | CodeRabbit | Greptile | Qodo |
|---|---|---|---|---|
| **BYOK** | ✅ NVIDIA NIM | ❌ SaaS only | ❌ SaaS only | ❌ SaaS only |
| **Per-dev cost** | $0.05/mo | $0.10-0.50/PR | $25/mo | $40-60/mo |
| **EU AI Act ready** | ✅ Art.12 L2 | ❌ | ❌ | ❌ |
| **Audit trail signed** | ✅ Ed25519 + BLAKE3 | ❌ | ❌ | ❌ |
| **MCP server** | ✅ 4 tools | ❌ | ❌ | ❌ |
| **A2A AgentCards** | ✅ | ❌ | ❌ | ❌ |
| **CordonEnforcer** (synthesizer doesn't see raw code) | ✅ | ❌ | ❌ | ❌ |
| **Hybrid detection** (deterministic + LLM) | ✅ | ❌ LLM-only | ❌ LLM-only | ❌ LLM-only |
| **Measured P/R/F1** | ✅ P=1.0, R=0.82 | ❌ | ❌ | ❌ |
| **Open source** | ✅ MIT | ❌ | ❌ | ❌ |
| **Pure Rust** | ✅ | ❌ TS/Node | ❌ TS/Node | ❌ TS/Node |

---

## 👥 For the [target user]

### For the **CISO** 👔

EU AI Act Art. 12 compliance is **one `curl`**, not a 6-month audit. The audit chain is **BLAKE3-hash-chained and Ed25519-signed** -- your regulator can verify it offline without trusting ARGUS. **BYOK + offline-first** means your code never leaves your host. No data residency issue. See [docs/for-ciso.md](docs/for-ciso.md) for the full pitch.

### For the **engineering manager** 📊

ARGUS pays for itself in week 1 of any team > 3 developers:

- **Per dev:** 25-40 min/PR saved in review (only edit the bot's draft) + ~15 min/week avoided in re-work
- **Per team of 10 devs:** 4-7 hrs/week in maintainer time + 5-10 AI slop bugs prevented/month
- **Per engineering manager:** 4-6 hrs/week in manual reporting → 0 with Aegis Lens

### For the **OSS maintainer** 🛠️

Stop drowning in AI slop. Add ARGUS as a pre-commit hook or a PR webhook. **P=1.0, R=0.82** on the deterministic layer means **zero false positives** for the rules we ship. The LLM semantic layer catches the rest. Triage in 4-8 seconds, not 40 minutes.

---

## 🗺️ Roadmap (what's shipped, what's next)

The 19 features shipped (1 of 20 deliberately not done):

| # | Feature | Status |
|---|---|---|
| 1.1 | Cohort view (dashboard) | ✅ Shipped |
| 1.2 | `fix_plan.json` hand-off | ✅ Shipped |
| 1.3 | aislop CI badge | ✅ Shipped (dogfooding virtuous loop) |
| 2.1 | `AuditEvent` (16 fields) BLAKE3 + Ed25519 | ✅ Shipped |
| 2.2 | NDJSON audit export | ✅ Shipped (regulator-ready) |
| 2.4 | Retention in `argus health` | ✅ Shipped (warns if <180d per Art. 19) |
| 3.1 | LLM circuit breaker | ✅ Shipped (no retry storms on NIM outage) |
| 3.2 | A2A AgentCards | ✅ Shipped (Google's open protocol) |
| 4 | EU AI Act L2 conformance | ✅ Shipped (default) |
| 4.1 | Per-role model registry | ✅ Shipped (deepseek-v4 / nemotron-3 / glm-5.1) |
| 5 | MCP server | ✅ Shipped (4 tools for Claude Code/Codex/Cursor) |
| 5.1 | Deterministic slop pre-flight | ✅ Shipped (5 SLOP rules, <100ms) |
| 6.1 | Graceful shutdown | ✅ Shipped (Axum `with_graceful_shutdown`) |
| 6.2 | Idempotency-Key | ✅ Shipped (24h TTL, no double-billing) |
| 6.3 | OpenTelemetry stdout | ✅ Shipped (env-gated) |
| 6.4 | SQLite audit persistence | ✅ Shipped (sqlx 0.7) |
| 7.1 | HeyGen deeplink | ✅ Shipped (url_encode, 0% cost) |
| 8.2 | SPIFFE primitives | ✅ Shipped (spiffe 0.16) |
| **7.2** | **BYVK opt-in** (HeyGen/D-ID video integration) | **⛔ Deliberately not done** -- the $78-460/yr cost kills the $0.05/dev/month story. 7.1 (deeplink) gives 80% of the value at 0% of the cost. |

### What's next (human-action items)

- 🔓 **crates.io publishing** -- 13 crates ready; awaiting `CARGO_REGISTRY_TOKEN` repo secret
- 🔓 **OpenSSF Best Practices Silver** -- evidence map ready at `docs/best-practices-silver.md`; awaiting form submission at `bestpractices.dev`
- 🔓 **First release on GitHub** with SLSA L3 attestation, SHA256 manifest, and distroless Docker image

---

## 🛠️ Use it. Fork it. Ship it.

```bash
git clone https://github.com/SuarezPM/apohara-argus.git
cd apohara-argus
export ARGUS_NIM_KEY=nvapi-xxx
cargo run -p apohara-argus-cli -- scan-diff ./your-pr.diff
```

**License: MIT.** Self-host, modify, redistribute. No telemetry, no phone-home.

Questions? Open an issue at `https://github.com/SuarezPM/apohara-argus/issues`.

---

### 📚 Read the docs

| Doc | What's in it |
|---|---|
| [docs/VERIFICATION.md](docs/VERIFICATION.md) | The 22-check local verification report |
| [docs/CI-VERIFICATION.md](docs/CI-VERIFICATION.md) | The 4 auto-trigger GitHub Actions workflows |
| [docs/HANDS-ON-QA.md](docs/HANDS-ON-QA.md) | 22/22 hands-on QA checks pass |
| [docs/SCOPE-FIDELITY.md](docs/SCOPE-FIDELITY.md) | 95/100 scope fidelity, 24/28 sub-tasks delivered |
| [docs/best-practices-silver.md](docs/best-practices-silver.md) | OpenSSF Best Practices Silver evidence map |
| [docs/BENCHMARK.md](docs/BENCHMARK.md) | P/R/F1 on 40 PRs + latency + cost |
| [docs/pricing.md](docs/pricing.md) | 3 tiers (Free / Team / Enterprise) |
| [docs/for-ciso.md](docs/for-ciso.md) | CISO-targeted EU AI Act pitch |
| [docs/branch-protection.md](docs/branch-protection.md) | Branch protection policy + `gh api` snippet |
| [SECURITY.md](SECURITY.md) | Threat model (covers / does NOT cover) |
| [GOVERNANCE.md](GOVERNANCE.md) | Roles, access continuity, fork-ability |
| [CONTRIBUTING.md](CONTRIBUTING.md) | DCO + coding standards + testing policy |
| [CHANGELOG.md](CHANGELOG.md) | Keep a Changelog format |

---

> *Built for the **Platzi Reto AI Academy** as 5 projects in one product:*
> *System of Prompts · Automate the Flow · Web App · The Agent · MVP with Real Intelligence.*
> *1 Cargo workspace, 15 crates, 194 tests, MIT license. The verification layer for the AI-generated code era.*
