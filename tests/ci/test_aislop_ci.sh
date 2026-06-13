#!/usr/bin/env bash
# Tests for the aislop CI integration (dogfooding virtuous loop, roadmap 1.3).
# Run: bash tests/ci/test_aislop_ci.sh
# Exits 0 on success, 1 on first failure.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="$REPO_ROOT/.github/workflows/aislop.yml"
BADGE_JSON="$REPO_ROOT/aislop-score.json"
README="$REPO_ROOT/README.md"

pass() { echo "  PASS: $1"; }
fail() { echo "  FAIL: $1"; exit 1; }

echo "test_aislop_ci:"

# Test 1: workflow YAML is valid and has the required triggers + steps
echo "  [1] workflow YAML is valid"
yq '.' "$WORKFLOW" >/dev/null \
  || fail "yq could not parse $WORKFLOW"
[ "$(yq '.name' "$WORKFLOW")" = '"aislop"' ] \
  || fail "workflow name != aislop"
[ "$(yq '.on.push.branches[0]' "$WORKFLOW")" = '"main"' ] \
  || fail "push trigger branch != main"
[ "$(yq '.on.pull_request.branches[0]' "$WORKFLOW")" = '"main"' ] \
  || fail "pull_request trigger branch != main"
grep -q "actions/checkout@v4" "$WORKFLOW" \
  || fail "workflow missing actions/checkout@v4"
grep -q "npx --yes aislop@latest" "$WORKFLOW" \
  || fail "workflow missing npx --yes aislop@latest"
grep -q "peter-evans/create-or-update-comment@v4" "$WORKFLOW" \
  || fail "workflow missing PR comment step"
pass "workflow YAML valid + required steps present"

# Test 2: badge JSON is valid shields.io endpoint format
echo "  [2] aislop-score.json is valid shields.io endpoint"
jq -e . "$BADGE_JSON" >/dev/null || fail "$BADGE_JSON is not valid JSON"
jq -e '.schemaVersion == 1 and .label == "aislop" and (.color | type) == "string"' "$BADGE_JSON" >/dev/null \
  || fail "$BADGE_JSON missing required shields.io fields (schemaVersion, label, color)"
pass "aislop-score.json valid + shields.io compatible"

# Test 3: README.md shows the badge and the URL points at the JSON file
echo "  [3] README.md badge is well-formed and points at the JSON"
grep -q "img.shields.io/endpoint" "$README" \
  || fail "README.md has no shields.io endpoint badge"
grep -q "aislop-score.json" "$README" \
  || fail "README.md badge does not reference aislop-score.json"
grep -q "raw.githubusercontent.com/SuarezPM/apohara-argus/main/aislop-score.json" "$README" \
  || fail "README.md badge URL does not point at the main branch JSON"
pass "README.md badge is well-formed and points at aislop-score.json"

echo "test_aislop_ci: all tests passed"
