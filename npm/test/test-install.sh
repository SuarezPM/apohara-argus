#!/usr/bin/env bash
# Bash smoke test for the @apohara/argus install flow.
#
# Validates the end-to-end path that `npx @apohara/argus` would take:
#   - mock GitHub release is built on disk with all 5 platform assets
#     and a real SHA256SUMS file (so the test exercises the integrity
#     path install.js enforces)
#   - ARGUS_NPM_TEST_MODE / ARGUS_NPM_TEST_RELEASE_DIR env vars are
#     exported, mirroring the contract the node test would observe
#     if install.js grew native test-mode support
#   - HOME / ARGUS_CACHE / XDG_CACHE_HOME are pointed at the tmpdir
#     so the cache the install writes is hermetic
#   - `node --test test/` runs the node:test suite, which monkey-
#     patches https.get to serve the mock release, exercises the
#     happy path / missing SHA256SUMS / mismatched SHA256SUMS, and
#     asserts the cache layout for all 5 platforms
#
# The script exits 0 iff the node test suite exits 0. It is meant
# to be runnable in CI without a real GitHub release or a real
# network round trip.

set -euo pipefail

# Move to the npm/ directory so the test paths resolve regardless
# of the caller's CWD.
cd "$(dirname "$0")/.."

# Sanity: a working `node` is required.
if ! command -v node >/dev/null 2>&1; then
  echo "node not found in PATH" >&2
  exit 1
fi

# Built-in `node:test` requires Node 18+. Older versions won't run
# the suite at all.
NODE_MAJOR="$(node -p 'process.versions.node.split(".")[0]')"
if [ "${NODE_MAJOR}" -lt 18 ]; then
  echo "node >= 18 required (got ${NODE_MAJOR})" >&2
  exit 1
fi

TMP_BASE="$(mktemp -d -t apohara-argus-smoke-XXXXXX)"
MOCK_RELEASE="${TMP_BASE}/mock-release"
MOCK_VERSION_DIR="${MOCK_RELEASE}/v0.1.0"
MOCK_HOME="${TMP_BASE}/home"
MOCK_CACHE="${TMP_BASE}/cache"

cleanup() {
  rm -rf "${TMP_BASE}"
}
trap cleanup EXIT

mkdir -p "${MOCK_VERSION_DIR}" "${MOCK_HOME}" "${MOCK_CACHE}"

# 5 platform assets. The 5th is the Windows .exe.
ASSETS=(
  "argus-x86_64-unknown-linux-gnu"
  "argus-aarch64-unknown-linux-gnu"
  "argus-x86_64-apple-darwin"
  "argus-aarch64-apple-darwin"
  "argus-x86_64-pc-windows-msvc.exe"
)

# Populate the mock release with placeholder binaries + a real
# SHA256SUMS (computed against the placeholder bytes). Distinct
# content per asset so the hashes are unique, mirroring a real
# release where each binary differs.
for asset in "${ASSETS[@]}"; do
  printf 'mock-binary-%s\n' "${asset}" > "${MOCK_VERSION_DIR}/${asset}"
done

# sha256sum is in coreutils on every platform the CI matrix covers.
( cd "${MOCK_VERSION_DIR}" && sha256sum "${ASSETS[@]}" > SHA256SUMS )

echo "Mock release built at ${MOCK_RELEASE}:"
ls -l "${MOCK_VERSION_DIR}"
echo
echo "SHA256SUMS:"
cat "${MOCK_VERSION_DIR}/SHA256SUMS"

# Export the test-mode env vars. The node test doesn't read them
# today (it stubs https.get directly) but exporting them keeps the
# smoke test aligned with the documented contract, so a future
# install.js that grows test-mode support will Just Work.
export ARGUS_NPM_TEST_MODE=1
export ARGUS_NPM_TEST_RELEASE_DIR="${MOCK_RELEASE}"

# Hermetic cache: the test must NEVER write to the real ~/.cache.
export HOME="${MOCK_HOME}"
export ARGUS_CACHE="${MOCK_CACHE}"
export XDG_CACHE_HOME="${MOCK_CACHE}"

echo
echo "Running node --test test/*.js ..."
node --test test/*.js

echo
echo "Smoke test passed."
