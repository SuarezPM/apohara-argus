# ARGUS CI Verification Report

**Generated:** 2026-06-13
**Verifier:** Sisyphus-Junior (Atlas orchestrator)
**Test branch:** verify/ci-v2-1781389773 (pushed, then deleted — see "Methodology" below)
**Base commit (main):** 50a838cb260c3b11fbbdb83f748a0bd90e5a9282 (`docs(verification): add V.1 local verification report`)

## Summary

| Result | Workflows |
|---|---|
| Green (3/4 S.3 auto-trigger) | CodeQL, Bench, Scorecard |
| Red (1/4 S.3 auto-trigger) | **CI** — fails on `RUSTFLAGS="-D warnings"` for unused import + dead code in `crates/argus-llm/src/openai_compat.rs` |
| Intentionally not triggered (4) | release, publish, _attest, publish-npm — manual workflows per the V.2 spec |
| Out of S.3 scope (1) | aislop — also runs on push, also failing, tracked separately in `docs/VERIFICATION.md` V.1 findings |

**3 of 4 S.3 auto-trigger workflows are green. CI is the blocker.**

## Methodology (important deviation from the V.2 spec)

The V.2 task spec said: create a `verify/ci-v2-<ts>` test branch, push a `.ci-trace` marker, and the 4 auto-trigger workflows would run automatically. **They did not.** All 4 S.3 auto-trigger workflows have `branches: [main]` filters on their `push:` triggers:

| Workflow | `on.push` filter |
|---|---|
| `ci.yml` | `branches: [main]` |
| `scorecard.yml` | `branches: [main]` (plus weekly schedule + workflow_dispatch) |
| `codeql.yml` | `branches: [main]` (plus weekly schedule) |
| `bench.yml` | `branches: [main]` |

Verified via `gh run list --limit 50 --json name,headBranch --jq '.[] | select(.headBranch=="verify/ci-v2-1781389773")'` → **0 results**. The test-branch push left the workflows dormant.

The freshest data available is from the most recent push to `main` itself — the V.1 verification report commit (50a838c), which triggered the 4 S.3 auto-trigger workflows at `2026-06-13T22:27:58Z`. Those are the runs reported below. The test branch was pushed, registered, then deleted (the runs persist in the GitHub UI independent of the branch).

If the goal of V.2 is to trigger the auto-trigger workflows on demand in the future, the test-branch approach will not work. Two viable alternatives:
1. Push the trace marker directly to `main` (e.g., a `ci: trace` squash merge), observe the runs, then revert.
2. Open a PR from the test branch — `pull_request` triggers on any branch.

## Workflow results

| # | Workflow | Triggered by | Status | Conclusion | Run URL |
|---|---|---|---|---|---|
| 1 | ci.yml | push (main 50a838c) | completed | **failure** | <https://github.com/SuarezPM/apohara-argus/actions/runs/27480989320> |
| 2 | scorecard.yml | push (main 50a838c) | completed | success | <https://github.com/SuarezPM/apohara-argus/actions/runs/27480989315> |
| 3 | codeql.yml | push (main 50a838c) | completed | success | <https://github.com/SuarezPM/apohara-argus/actions/runs/27480989334> |
| 4 | bench.yml | push (main 50a838c) | completed | success | <https://github.com/SuarezPM/apohara-argus/actions/runs/27480989324> |
| 5 | release.yml | (not triggered) | N/A | workflow_dispatch + tag `v*` only | — |
| 6 | publish.yml | (not triggered) | N/A | workflow_dispatch only | — |
| 7 | _attest.yml | (not triggered) | N/A | workflow_call only (called from release.yml) | — |
| 8 | npm/.github/workflows/publish-npm.yml | (not triggered) | N/A | workflow_dispatch only | — |

## Failures (CI only)

### CI workflow — run 27480989320

**Outcome:** 3 of 5 jobs failed (test ubuntu, test macos, test windows, clippy); 2 succeeded (rustfmt, cargo-deny).
**Root cause:** CI sets `RUSTFLAGS="-D warnings"`, so all warnings become compile errors. `crates/argus-llm/src/openai_compat.rs` has 1 unused import + 4 dead struct fields.

#### Job 1: test (ubuntu-latest) — `Run workspace tests` — exit 101

```
error: unused import: `Role`
  --> crates/argus-llm/src/openai_compat.rs:13:82
   |
13 | use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError, Message, Role, Usage};
   |                                                                                  ^^^^^
   = note: `-D unused-imports` implied by `-D warnings`

error: field `id` is never read
  --> crates/argus-llm/src/openai_compat.rs:52:5
   |
51 | struct ApiResponse {
   |        ----------- field in this struct
52 |     id: Option<String>,
   |     ^^^^^^^^^^^^^^^^^

error: fields `index` and `finish_reason` are never read
  --> crates/argus-llm/src/openai_compat.rs:60:5
   |
59 | struct Choice {
   |        ------ fields in this struct
60 |     index: u32,
   |     ^^^^^^
   ...
62 |     finish_reason: Option<String>,
   |     ^^^^^^^^^^^^^^

error: field `role` is never read
  --> crates/argus-llm/src/openai_compat.rs:67:5
   |
66 | struct ChoiceMessage {
   |        ------------- field in this struct
67 |     role: String,
   |     ^^^^^

error: fields `error_type` and `code` are never read
  --> crates/argus-llm/src/openai_compat.rs:87:5
   |
84 | struct ApiErrorBody {
   |        ------------ field in this struct
   ...
87 |     error_type: Option<String>,
   |     ^^^^^^^^^^^
88 |     code: Option<String>,
   |     ^^^^^

error: could not compile `argus-llm` (lib) due to 5 previous errors
```

The same 5 errors appear in the macOS and Windows `Run workspace tests` jobs and in the `clippy (-D warnings)` job.

#### Why local `cargo test` passes but CI fails

V.1 verified that `cargo test --workspace` and `cargo clippy --all-targets` both PASS locally (see `docs/VERIFICATION.md` V.1 report). The discrepancy is the CI-only `RUSTFLAGS="-D warnings"` (env var in `.github/workflows/ci.yml` line 19) — local invocations don't set it, so warnings are warnings. The fix is on the source side, not the CI side (the `-D warnings` policy is intentional: see CLAUDE.md "the 'no warning debt' rule").

#### Suggested fix (for the follow-up task, not done here)

```rust
// crates/argus-llm/src/openai_compat.rs

// Line 13: drop the unused `Role`
- use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError, Message, Role, Usage};
+ use super::{CompletionRequest, CompletionResponse, LlmClient, LlmError, Message, Usage};

// Lines 50-89: annotate the deserialization structs/fields that are
// needed for serde but not used after parsing.
- #[derive(Debug, Deserialize)]
- struct ApiResponse {
+ #[derive(Debug, Deserialize)]
+ #[allow(dead_code)]  // fields populated by serde, read by debug formatting
+ struct ApiResponse {
      id: Option<String>,
      ...
  }
- #[derive(Debug, Deserialize)]
- struct Choice {
+ #[derive(Debug, Deserialize)]
+ #[allow(dead_code)]
+ struct Choice {
      index: u32,
      ...
  }
- #[derive(Debug, Deserialize)]
- struct ChoiceMessage {
+ #[derive(Debug, Deserialize)]
+ #[allow(dead_code)]
+ struct ChoiceMessage {
      role: String,
      ...
  }
- #[derive(Debug, Deserialize)]
- struct ApiErrorBody {
+ #[derive(Debug, Deserialize)]
+ #[allow(dead_code)]
+ struct ApiErrorBody {
      message: String,
      ...
  }
```

Alternative: prepend `#![allow(dead_code)]` at the top of the file (or scope it via a module-level allow). The struct-level annotation is more surgical and survives future tightening.

## What's ready for the final commit

- The 4 auto-triggered workflows (ci, scorecard, codeql, bench) MUST be green before the final commit.
- **3 of 4 are green right now. CI is not.**
- The 4 manual-trigger workflows (release, publish, _attest, publish-npm) are intentionally not triggered here — they're tested in V.4 (the actual release + publish flow).

## CI-specific issues found (not in V.1)

1. **CI fails on every push to main** because of the `openai_compat.rs` unused import + 5 dead-code lints. This is a NEW CI-only issue (V.1 local `cargo test` and `cargo clippy` both pass, but local runs do not use `RUSTFLAGS="-D warnings"`). The same fix unblocks all 4 failing CI jobs (ubuntu, macos, windows, clippy).
2. **Test branch pushes do not trigger the 4 auto-trigger workflows** — the `branches: [main]` filter blocks them. The V.2 spec assumed otherwise. Either the spec should be updated, or V.3/V.4 should use a main-push or PR approach instead of a feature branch.

## Regressions

NONE in the green workflows (CodeQL, Bench, Scorecard all match the V.1 push behaviour).

The CI failure is not a regression — it has been failing on every main push since at least `2026-06-13T21:16Z` (the earliest run in the visible history), which is consistent with the file having been written before the `-D warnings` policy was enforced. It is a pre-existing V.1 finding that surfaced only when CI actually ran.

## Total time

- CodeQL: **1m 46s** (started 22:27:58Z, ended 22:29:44Z)
- Bench: **0m 43s** (started 22:27:58Z, ended 22:28:41Z)
- Scorecard: **0m 23s** (started 22:27:58Z, ended 22:28:21Z)
- CI: **2m 42s** (started 22:27:58Z, ended 22:30:40Z) — failed
- **Total wall time for the 4 auto-trigger workflows: 2m 42s** (all start at 22:27:58Z, last ends at 22:30:40Z)

## V.2 verification commands (executed, in chronological order)

```sh
# 1. Confirm gh CLI is authenticated
gh auth status 2>&1 | head -5
#   -> Logged in to github.com account SuarezPM, scopes: gist, read:org, repo, workflow

# 2-4. Create test branch, add .ci-trace, commit, push
git checkout main && git pull origin main
git checkout -b verify/ci-v2-1781389773
echo "ci-trace: 2026-06-13T22:29:37Z" > .ci-trace
git add .ci-trace
git -c user.name=Pablo -c user.email=pablo@example.com commit -s -m "ci(trace): V.2 CI verification trace marker"
git -c user.name=Pablo -c user.email=pablo@example.com push -u origin HEAD
#   -> Branch pushed: d7fe025c492ef33e3ed5fbef46e99adbd339be1f

# 5. Sleep 30s + check for runs on the test branch
gh run list --limit 50 --json headBranch --jq '.[] | select(.headBranch=="verify/ci-v2-1781389773")'
#   -> 0 results. Test branch push did not trigger any workflows.

# 6-9. Use the most recent main push (V.1 docs commit 50a838c) as the verification source
gh run list --limit 50 --json name,status,conclusion,createdAt,headBranch,event,databaseId
#   -> 5 workflows triggered at 22:27:58Z: CodeQL success, Bench success, aislop failure,
#      CI failure, Scorecard success

# 10. Inspect the CI failure
gh run view 27480989320 --json jobs
gh run view 27480989320 --job 81228618634 --log  # ubuntu test job
#   -> 5 compile errors in crates/argus-llm/src/openai_compat.rs

# 12-13. Clean up
git checkout main
git push origin --delete verify/ci-v2-1781389773
git branch -D verify/ci-v2-1781389773
#   -> Test branch fully removed; .ci-trace not on main; working tree clean
```

## Post-cleanup verification

```sh
git branch -a | grep "verify/ci-v2"     # -> empty (test branch deleted, good)
git branch --show-current                # -> main
test -f .ci-trace                        # -> .ci-trace not on main (good)
git status --short                       # -> empty (working tree clean)
gh run list --limit 4 --json name,conclusion
#   -> 3 success (Scorecard, CodeQL, Bench) + 1 failure (CI) — as reported
```
