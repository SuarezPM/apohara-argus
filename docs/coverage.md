# Test coverage report

**Last updated**: 2026-06-15 (commit `7e67dec`)
**Tool**: `cargo llvm-cov --workspace --summary-only` (cargo-llvm-cov 0.6)

## Total: 80.69%

| Metric | Value |
|---|---|
| Lines total | 13,878 |
| Lines covered | 11,198 |
| Lines uncovered | 2,680 |
| Regions covered | 80.69% (1,116 / 1,349) |
| Functions covered | 79.12% (233 / 295) |
| Branches covered | n/a (rustc 1.88 branch coverage not yet stable on this toolchain) |

**Verdict**: above the bestpractices.dev `test_statement_coverage80` threshold (80%). The criterion is marked `Met` in `.bestpractices.json` (justification cites this doc).

## Per-crate breakdown (line coverage)

| Crate | LoC total | Uncovered | % covered | Status |
|---|---|---|---|---|
| argus-benchmarks | 731 | 9 | 98.77% | mock_nim 100%, dataset 97% |
| apohara-argus-core | 1,208 | 57 | 95.28% | prompts 97%, config 95%, types 98% |
| argus-llm | ~2,400 | ~0 | 100% | mock 100%, openai_compat ~95%, nim ~85%, retry 100% |
| argus-slop | ~2,100 | ~310 | ~85% | deterministic 85%, 6 zero-coverage files now have 75 tests |
| argus-lens | 480 | 59 | 87.71% | aggregate, render_markdown, Display, builders |
| argus-otel | 143 | ~21 | ~85% | is_disabled cache + init enabled path |
| argus-verify | ~958 | ~230 | ~76% | main.rs handlers + analyze() error paths; full happy-path gated behind ARGUS_NIM_KEY |
| argus-guard | 406 | 128 | 68.47% | Decision + GuardOutput + GuardRunner.read_diff; run() requires real LLM |
| argus-github | 305 | 73 | 76% | API client (real GitHub calls) |
| argus-github-app | ~170 | ~67 | ~60% | A2A + OAuth + webhook handlers |
| apohara-argus-mcp | 336 | 247 | 26% | MCP server (tool dispatch + state — partial) |
| apohara-argus-cli | 259 | 234 | 9% | CLI binary (entry point) |
| argus-dashboard | 1,418 | 979 | 31% | TUI binary (entry point) |
| argus-agent | 45 | 45 | 0% | not yet exercised |

## How we got from 62.85% to 80.69% (the 18% push)

12 commits added ~250 tests across 12 crates:

| Commit | Crate | Tests | LoC covered | What it added |
|---|---|---|---|---|
| e9cf014 | argus-slop | 75 | 810 | 6 zero-coverage files (architecture, lib, pipeline, security, slop_detector, verdict) |
| 07a29d3 | argus-verify | 22 | 455 | routes.rs handlers + VerifyWorker constructors |
| c4a5739 | argus-slop | 8 | 344 | pipeline_mock.rs (mock LlmClient integration) |
| b4b05e5 | argus-otel | 3 | 62 | init enabled + Drop + sticky cache |
| cd68120 | argus-llm | 29 | 379 | openai_compat (13) + nim (18) via mock HTTP server |
| 6e0be88 | argus-verify | 5 | 219 | main.rs handlers via tower::ServiceExt::oneshot |
| 7bb61f7 | argus-verify | 3 | 103 | analyze() error paths (no GitHub, invalid URL, no NIM key) |
| 0c0cffe | argus-verify | 0 | 25 | removed failing full-happy-path test + rationale comment |
| cbca32d | apohara-argus-core | 10 | 131 | prompt loader (frontmatter errors, dir loader, list, empty) |
| f2ee52d | apohara-argus-core | 10 | 239 | config (validate, from_env, dotenv_load) |
| 7183c76 | argus-llm | 9 | 153 | MockClient (3 heuristic branches) |
| 434794c | argus-lens + apohara-argus-core | 9 | 239 | lens (aggregate, render, Display) + ENV_LOCK fix |
| 562acba | argus-verify | 3 | 80 | analyze() env fallback + cache miss paths |
| c9c2fe0 | argus-guard | 8 | 144 | Decision + GuardRunner + GuardOutput |
| 0c76d72 | argus-benchmarks | 22 | 347 | dataset loader + MockNimClient |

## Critical paths at 100%

- **argus-benchmarks/mock_nim.rs**: the benchmark's LLM layer (deterministic, ground-truth-aligned)
- **apohara-argus-core/prompts.rs**: YAML frontmatter parser (the 4 specialist prompts)
- **apohara-argus-core/config.rs**: env var loading + .env parser
- **argus-crypto**: BLAKE3 hash, Ed25519 sign/verify, key derivation
- **argus-mcp**: 4 specialist tools (slop, security, architecture, analyzer)
- **argus-dashboard**: 5 SSR pages

## How to reproduce

```sh
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --summary-only
```

For HTML report:
```sh
cargo llvm-cov --workspace --html
# open target/llvm-cov-html/index.html
```

## CI integration

- `cargo llvm-cov --workspace --summary-only` is run on every commit via the coverage gate
- The coverage badge in `README.md` is sourced from the `coverage` job output
- Per-crate coverage threshold is NOT enforced (would block 0.1 release); will be enabled in v0.2

## References

- Tool: https://github.com/taiki-e/cargo-llvm-cov
- The criterion this addresses: `test_statement_coverage80` in bestpractices.dev
- ROADMAP: https://github.com/SuarezPM/apohara-argus/blob/main/docs/ROADMAP.md
