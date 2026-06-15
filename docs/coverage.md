# Test coverage report

**Last updated**: 2026-06-15
**Tool**: `cargo-llvm-cov --workspace --summary-only` (cargo-llvm-cov 0.6)

## Total: 

## Per-crate breakdown (line coverage)

| Crate | LoC total | Uncovered | % covered |
|---|---|---|---|
| argus-crypto | 62 | 0 | 100% |
| argus-mcp | 215 | 0 | 100% |
| argus-dashboard | 350 | 0 | 100% |
| argus-verify | ~1,500 | ~330 | ~78% |
| argus-otel | 143 | 63 | 55.94% |
| argus-slop | 659 | 350 | ~47% (5 of 6 source files at 0%) |
| argus-llm | ~465 | ~200 | ~57% (HTTP mocking needed) |

## Critical paths at 100%

- **argus-crypto**: BLAKE3 hash, Ed25519 sign/verify, key derivation
- **argus-mcp**: 4 specialist tools (slop, security, architecture, analyzer)
- **argus-dashboard**: 5 SSR pages

## Plan to reach 80% (v0.2, 2026-Q3)

1. **argus-slop unit tests for 5 zero-coverage files** (295 LoC, ~10 simple tests)
2. **argus-llm HTTP mock tests with wiremock-rs** (~150 LoC, ~15 tests)
3. **argus-verify CLI spawn tests with assert_cmd** (~200 LoC, ~10 tests)
4. **argus-otel error path tests** (~80 LoC, ~8 tests)

Total: ~725 LoC new coverage, ~43 new tests, estimated 2 days of focused work.

## CI integration

- `cargo llvm-cov --workspace --summary-only` is run locally on every commit
- CI gate (fail if coverage drops) is on the roadmap for v0.2
- Per-crate coverage threshold is NOT enforced at v0.1 (would block the 0.1 release)

## References

- Tool: https://github.com/taiki-e/cargo-llvm-cov
- The criterion this addresses: `test_statement_coverage80` in bestpractices.dev
- ROADMAP: https://github.com/SuarezPM/apohara-argus/blob/main/docs/ROADMAP.md
