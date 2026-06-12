#!/usr/bin/env bash
# Build a static-export of the dashboard for Vercel/Netlify hosting.
# Reads the latest briefing from docs/briefings/latest.md and embeds it
# into a static index.html that can be served from any CDN.

set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR/.."

mkdir -p deploy/dist

# We need to render the index.html from the dashboard binary first.
# We do this by running the dashboard briefly, curling /, then killing it.

if [[ -z "${ARGUS_NIM_KEY:-}" ]]; then
    echo "WARNING: ARGUS_NIM_KEY not set. Static export will show empty briefing." >&2
    export ARGUS_NIM_KEY="placeholder-for-build"
fi

PORT=${STATIC_EXPORT_PORT:-18765}
echo "→ Building static export via dashboard on port $PORT..."

# Start dashboard in background
ARGUS_DASHBOARD_PORT=$PORT cargo run --release -p argus-dashboard --bin argus-dashboard > /tmp/argus-dashboard.log 2>&1 &
PID=$!

# Wait for it to be ready
for i in {1..30}; do
    if curl -sf http://localhost:$PORT/ > /dev/null 2>&1; then break; fi
    sleep 0.5
done

# Fetch the rendered index
curl -sf http://localhost:$PORT/ > deploy/dist/index.html
curl -sf http://localhost:$PORT/weekly > deploy/dist/weekly.html
curl -sf http://localhost:$PORT/submit > deploy/dist/submit.html

# Kill the dashboard
kill $PID 2>/dev/null || true

echo "✓ Static export complete:"
ls -la deploy/dist/
