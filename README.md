# ARGUS — AI Slop Defense Layer

![aislop score](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/SuarezPM/apohara-argus/main/aislop-score.json)
![Rust](https://img.shields.io/badge/rust-100%25-orange)
![EU AI Act](https://img.shields.io/badge/EU%20AI%20Act-Art.%2012%20ready-blue)

> The first trust layer for AI-generated code. 5 Platzi Reto projects in one product.
> Pure Rust 100%. BYOK (NVIDIA NIM compatible). 11 crates, one binary.

## The thesis

In 2025, GitHub saw a **206% increase in AI-generated projects**. AI-generated PRs
**wait 4.6× longer in review** and introduce **15-18% more security vulnerabilities**
([Opsera 2026](https://opsera.ai/resources/report/ai-coding-impact-2026-benchmark-report/)).
**96% of developers don't fully trust AI code** they wrote
([Sonar 2026](https://www.sonarsource.com/blog/state-of-code-developer-survey-report-the-current-reality-of-ai-coding)).
The maintainer of curl **closed the bug bounty** because 19 of 20 reports were
AI hallucinations. arXiv is banning researchers for AI slop. The contract of
open source — *useful code is welcome* — has collapsed under the weight of
zero-marginal-cost AI-generated noise.

**AI slop is a tragedy of the commons** ([Baltes, Cheong, Treude, arXiv:2603.27249](https://arxiv.org/abs/2603.27249)):
individual productivity gains externalize costs onto reviewers and maintainers.
The bottleneck isn't generation. **It's verification.**

ARGUS is the verification infrastructure. Three layers operating across the
SDLC, one shared ledger, one signed certificate per analysis.

## The 5 Platzi projects, one product

| # | Platzi project | ARGUS component | Crate |
|---|---|---|---|
| 1 | **System of prompts** (3+ interconnected prompts) | The Argus Prompt Library — 4 documented prompts any team can use | `crates/argus-core/prompts/*.md` |
| 2 | **Automate the flow nobody wants to do** (Make/n8n) | 3 autonomous Tokio workers: Guard, Verify, Lens | `argus-guard`, `argus-verify`, `argus-lens` |
| 3 | **Web app that solves a real problem** (Lovable/v0) | SSR dashboard with Axum + htmx, hosted on Vercel as static export | `argus-dashboard` |
| 4 | **The agent your company needs** (OpenClaw/Claude Code) | The agent is the workflow itself: skills, context, decisions, MCPs | `argus-agent` + `docs/agent-spec.md` |
| 5 | **MVP with real intelligence** (LLM via API) | Backend with `argus-llm` (NVIDIA NIM via OpenAI-compatible API, BYOK) | `argus-api` + `argus-llm` |

## Architecture

```
                        GitHub PR / commit / org scan
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                     ARGUS — Three Layers                          │
│                                                                   │
│   Aegis Guard  ───►  Aegis Verify  ───►  Aegis Lens              │
│   (pre-commit)      (PR review)         (weekly digest)         │
│                                                                   │
│            4 analyzers (slop, security, arch, verdict)           │
│                       │                                           │
│                       ▼                                           │
│              Signed ledger (Supabase Postgres)                  │
└─────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
              Public dashboard (Vercel, static SSR from Rust)
```

## Stack (pure Rust 100%)

- **Runtime:** Tokio
- **Web:** Axum + askama + htmx (no JS framework)
- **DB:** sqlx + PostgreSQL (Supabase)
- **LLM client:** `reqwest` + `serde` (no LangChain, no Rig)
- **LLM provider:** NVIDIA NIM via OpenAI-compatible API (BYOK)
- **Crypto:** ed25519-dalek + blake3 (signed audit chain)
- **Image (optional):** DALL-E 3 or Flux via `reqwest`
- **Video (optional):** HeyGen or D-ID via `reqwest`
- **Deploy:** Vercel (static UI) + Fly.io (Rust binary) + Supabase (DB) + GitHub (source)

## The 4 prompts (P1 deliverable)

1. **`01-slop-detector.md`** — detects AI-generated code signals
2. **`02-redteam-security.md`** — adversarial security review (hardcoded secrets, RCE, injection)
3. **`03-architecture-fit.md`** — verifies PR coherence with the existing repo
4. **`04-verdict-synthesizer.md`** — synthesizes the final verdict from the 3 findings

Each prompt is a `.md` file with frontmatter (model, temperature, expected output
format) and is loaded at runtime by `argus-core::prompts`.

## Quickstart

```bash
git clone https://github.com/SuarezPM/apohara-argus.git
cd apohara-argus

# Workspace bootstrap (already done)
cargo build --workspace

# Run the CLI to analyze a local diff (needs your BYOK NIM key)
export ARGUS_NIM_KEY=nvapi-xxx
cargo run -p argus-cli -- scan-diff ./tests/fixtures/clean-pr.diff

# Start the dashboard (SSR-rendered, talks to Supabase)
cargo run -p argus-dashboard -- --port 3000

# Start the API
cargo run -p argus-api -- --port 8080
```

## Time saved (conservative)

- **Per dev:** 25-40 min/PR saved in review (only edit the bot's draft) + ~15 min/week avoided in re-work
- **Per team of 10 devs:** 4-7 hrs/week in maintainer time + 5-10 AI slop bugs prevented/month
- **Per engineering manager:** 4-6 hrs/week in manual reporting → 0 with Aegis Lens

## License

MIT — use it, fork it, ship it.
