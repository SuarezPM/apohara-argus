# Branch protection for `main`

This document specifies the branch-protection rules that should be enabled on
the `main` branch of `apohara-argus`, with the threat-model reasoning for each
rule. The settings here are the GitHub-side complement to the in-repo
`SECURITY.md` threat model: the policy says *what* the code guarantees, the
branch protection ensures *how* code reaches `main`.

These rules close the OpenSSF Scorecard **`Code-Review`** check: a
default-branch protection rule that requires at least one human review before
merging is the strongest single signal Scorecard uses for that check. The
`maintainer` here is **Pablo Suarez** (GitHub: `@SuarezPM`).

## Why these settings

ARGUS's load-bearing guarantees are *not* just code — they're code that
regulators and downstream users can verify. The threat model is dominated by
three risk classes, and each rule below defends against at least one of them:

1. **Audit-chain integrity** (`crates/argus-crypto/`). The 15-field
   `AuditEvent` is BLAKE3-chained and Ed25519-signed, and the chain is the
   regulator-facing artifact for EU AI Act Art. 12 Level 2 conformance. A
   commit that bypasses review or rewrites history can break the chain's
   verifiability even if the code compiles. The "signed commits" and
   "linear history" rules defend this directly.
2. **CordonEnforcer isolation** (`crates/argus-verify/src/cordon.rs`). The
   `VerdictSynthesizer` is isolated from raw code at the *type level* (it
   receives `RedactedSpecialistReport`, never `SpecialistReport` or `String`).
   The "Code Owners review" rule ensures any change to the isolation type
   gets a second pair of expert eyes before merge.
3. **Deterministic guarantees** (`crates/argus-slop/` and the
   `tests/benchmark.rs` 0-FP / 0-FN contract). The "status checks required"
   rule makes the precision contract a merge gate, not a hope.

The rest of the rules (1-review-required, dismiss-stale, no force-push, no
deletions, admins included) defend the *process* that defends the *code*:
they keep the human-in-the-loop, the history append-only, and the maintainer
themselves honest.

## The rules

| # | Rule | Setting | Why |
|---|------|---------|-----|
| 1 | Require a pull request before merging | Required approving reviews: **1** | A single human review is the minimum bar Scorecard's `Code-Review` check requires, and the smallest step that catches a careless mistake without becoming a coordination bottleneck for a single-maintainer project. |
| 2 | Require review from Code Owners | **Yes** | Defends the CordonEnforcer and audit-chain code paths: changes to `crates/argus-crypto/`, `crates/argus-verify/src/cordon.rs`, and `crates/argus-slop/rules/` should require owner sign-off because weakening them is silent. *Note: a `CODEOWNERS` file is not yet committed; the rule takes effect once a CODEOWNERS file lists those paths. Track this as a follow-up — see "Maintenance" below.* |
| 3 | Dismiss stale pull request approvals when new commits are pushed | **Yes** | A reviewer approved commit `abc123`; the PR was force-updated to commit `def456`. Without this rule, the old approval would still gate the merge. New code, new review. |
| 4 | Require status checks to pass before merging | See check list below | The CI + Scorecard surface is the contract: `clippy -D warnings` is the no-warning-debt rule, the OS-matrix `test` is the cross-platform gate, `cargo-deny` enforces the license allowlist and the RUSTSEC advisory list (the 6 acknowledged advisories in S.2 are documented in `deny.toml`), and Scorecard is the OpenSSF supply-chain posture. |
| 5 | Require branches to be up to date before merging | **Yes** | A green status check on `commit A` does not guarantee the same check is green on `commit B` after a push. "Up to date" forces the merge commit to be re-validated against the latest `main`, closing the "I had green CI 10 commits ago" bypass. |
| 6 | Require signed commits | **Yes** | The audit chain is git-history-visible. An unsigned commit can be silently rewritten at the local-dev level before push; a signed commit carries a verifiable identity claim. Together with linear history, this is what makes the chain regulator-defensible. |
| 7 | Require linear history | **Yes** (no merge commits) | A merge commit introduces two parents and two possible "true" histories. The BLAKE3 chain is anchored on a single linear sequence of commits, so a merge commit confuses both humans (`git log --first-parent` no longer matches the chain) and tools (the `prev_hash` field in `AuditEvent` references a single predecessor). Squash- or rebase-merge only. |
| 8 | Include administrators | **Yes** (no admin bypass) | The maintainer is also a contributor. "Include administrators" means the maintainer cannot push a hotfix that bypasses review. The trade-off (slower emergency response) is acceptable: there is no operational SLA that requires a bypass for a self-hosted offline tool. |
| 9 | Restrict who can push to matching branches | Users: **only the maintainer** (`@SuarezPM`) | The only push path to `main` is via PR; the maintainer is the only person authorized to bypass a deny-side block. No teams, no apps — single point of accountability. |
| 10 | Allow force pushes | **No** | A force-push rewrites history. Combined with signed commits, a force-push invalidates the signatures of all subsequent commits and breaks the audit chain. Force-pushes on `main` are not a feature we need; feature work happens on branches. |
| 11 | Allow deletions | **No** | `main` is the only canonical history. Deleting it would orphan every clone's `main` reference and require a coordinated re-clone. There is no legitimate operational reason to delete `main`. |

### Required status checks (exact strings)

The strings below are the **display names** GitHub shows in the branch
protection UI: `<workflow-file-name> / <job-display-name>`. They are case-
and-whitespace sensitive. Use them verbatim when filling the "Status checks
that are required" field.

| Check name | Workflow file | Job ID | What it gates |
|------------|---------------|--------|---------------|
| `CI / clippy (-D warnings)` | `.github/workflows/ci.yml` | `clippy` | The no-warning-debt rule. `clippy --all-targets -- -D warnings` fails on any lint, not just an opinion. |
| `CI / test (ubuntu-latest)` | `.github/workflows/ci.yml` | `test` (matrix: `ubuntu-latest`) | Linux build + 145+ unit/integration tests + the `--benches` ReDoS guard. |
| `CI / test (macos-latest)` | `.github/workflows/ci.yml` | `test` (matrix: `macos-latest`) | macOS build + the same test set, ensures no `cfg(target_os)`-specific regression. |
| `CI / test (windows-latest)` | `.github/workflows/ci.yml` | `test` (matrix: `windows-latest`) | Windows build + the same test set, same rationale. |
| `CI / cargo-deny (licenses + advisories)` | `.github/workflows/ci.yml` | `deny` | The license allowlist (`MIT`, `Apache-2.0`, …, `MPL-2.0`) and the 6 acknowledged RUSTSEC advisories. A new transitive dep with a copyleft license breaks the merge. |
| `Scorecard / Scorecard analysis` | `.github/workflows/scorecard.yml` | `analysis` | OpenSSF Scorecard weekly supply-chain posture; SARIF published to the Security tab. |

`CodeQL / Analyze (rust)`, `aislop / aegis-slop check`, `Release / build`,
`Release / gh-release`, `Release / attest-provenance`, and `Publish / publish`
are deliberately **not** required as merge gates: they are either triggered
on schedule (`codeql.yml` weekly cron), or on tag push (`release.yml` /
`publish.yml`), or are a CI decoration (`aislop.yml` badge feed). Required
status checks should be the *blocking* checks only — the ones that say
"this PR is not mergeable" — not the informational or post-merge runs.

## How to apply

### Option A — GitHub UI (recommended for the first apply)

1. Open `https://github.com/SuarezPM/apohara-argus/settings/branches`.
2. Click **Add rule** (or edit the existing `main` rule).
3. Branch name pattern: `main`.
4. Toggle the rules per the table above.
5. In "Status checks that are required", search for and add each of the
   six check names verbatim.
6. Click **Create** (or **Save changes**).

The UI walkthrough is the same one documented in
[GitHub's "Managing branch protection rules" guide][gh-bp].

[gh-bp]: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/managing-a-branch-protection-rule

### Option B — `gh api` from the CLI (re-runnable, reviewable)

The maintainer can apply the same rule via the GitHub REST API. The command
is two calls because `required_signatures` is a separate toggle endpoint
(not a field of the main protection object):

```bash
# Step 1: apply the main protection rule.
gh api \
  --method PUT \
  -H "Accept: application/vnd.github+json" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  /repos/SuarezPM/apohara-argus/branches/main/protection \
  --input - <<'EOF'
{
  "required_status_checks": {
    "strict": true,
    "contexts": [
      "CI / clippy (-D warnings)",
      "CI / test (ubuntu-latest)",
      "CI / test (macos-latest)",
      "CI / test (windows-latest)",
      "CI / cargo-deny (licenses + advisories)",
      "Scorecard / Scorecard analysis"
    ]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": true,
    "required_approving_review_count": 1
  },
  "restrictions": {
    "users": ["SuarezPM"],
    "teams": [],
    "apps": []
  },
  "required_linear_history": true,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_conversation_resolution": true,
  "lock_branch": false,
  "allow_fork_syncing": false
}
EOF

# Step 2: enable required signed commits (separate endpoint, not a field
# of the main protection object in the REST API).
gh api \
  --method POST \
  -H "Accept: application/vnd.github+json" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  /repos/SuarezPM/apohara-argus/branches/main/protection/required_signatures
```

The `gh api` path requires `repo` scope on the token. Verify the rule
with:

```bash
gh api /repos/SuarezPM/apohara-argus/branches/main/protection | jq .
```

The JSON response should echo back the body above (plus the timestamps
GitHub injects). The `required_signatures` endpoint returns 204 on success
and 404 if disabled; check it with:

```bash
gh api /repos/SuarezPM/apohara-argus/branches/main/protection/required_signatures
# → 204 No Content if enabled
```

## Maintenance

- **When a new workflow is added** to `.github/workflows/`, if the new job
  is a *blocking* gate (PR-blocking, not tag- or schedule-triggered), add
  its display name to the "Required status checks" list above *and* update
  the `gh api` body in this doc so the two stay in sync.
- **When a CODEOWNERS file is committed** (out of scope for S.5; tracked as
  a follow-up), the "Require review from Code Owners" rule becomes
  meaningful — until then, the rule is set but inert. The intent is to
  require owner sign-off on `crates/argus-crypto/`,
  `crates/argus-verify/src/cordon.rs`, and `crates/argus-slop/`.
- **When a new maintainer joins**, add their GitHub handle to the
  `restrictions.users` array in the `gh api` body. The "Restrict who can
  push" rule should be the *only* way the new maintainer gets push access
  to `main` — the `CODEOWNERS` review right is granted separately.
- **When the Scorecard or CI workflow renames a job**, update the "Required
  status checks" table. A wrong check name is a silent failure: GitHub
  will list it as required, but no PR will ever satisfy it, blocking all
  merges.

## References

- `SECURITY.md` — the authoritative "covers / does NOT cover" threat model
  this rule set defends.
- `GOVERNANCE.md` § *Access continuity* — the human-side bus factor
  (off-site break-glass credential custody, fork-ability). Branch
  protection is the technical half; the human half is out-of-band.
- `deny.toml` — the license allowlist and the 6 acknowledged RUSTSEC
  advisories the `cargo-deny` check enforces.
- `.github/workflows/ci.yml`, `.github/workflows/scorecard.yml` — the
  source of truth for the status check display names.
- OpenSSF Scorecard `Code-Review` check — the
  [Scorecard documentation](https://github.com/ossf/scorecard/blob/main/docs/checks.md#code-review)
  describes the heuristic that this rule set satisfies.
