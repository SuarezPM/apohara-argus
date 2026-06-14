# Deploying the ARGUS dashboard to argus.apohara.dev

This document explains how to ship `argus-dashboard` to
Fly.io and bind it to the `argus.apohara.dev` custom domain.
The artifacts in this directory (the `Dockerfile` and
`fly.toml`) are tuned for the free tier — 256 MB RAM, 1
shared CPU, auto-stop on idle.

The dashboard is the public-facing entry point of the
apohara.dev product family. It serves the SSR landing
page, the live demo panel (powered by the 4 ARGUS
specialists), the API surface (`/api/health`, `/api/demo`,
`/submit`), and the static marketing assets.

---

## 1. Prerequisites

You need four things before you can deploy:

### 1.1 Fly.io account

Sign up at https://fly.io/app/sign-up. A free-tier
account is sufficient for the dashboard.

### 1.2 The `flyctl` CLI

Install per the official guide:
https://fly.io/docs/hands-on/install-flyctl/

Verify the install:

```sh
fly version          # should print 0.x.y or later
fly auth login       # browser-based OAuth flow
```

### 1.3 DNS access for `apohara.dev`

The custom domain `argus.apohara.dev` is a subdomain of
the parent brand. You need write access to the
`apohara.dev` DNS zone — typically through the registrar
where the domain is parked (Cloudflare, Porkbun,
Namecheap, etc.). Specifically, you must be able to add
a CNAME record pointing `argus.apohara.dev` at
`argus-dashboard.fly.dev` (see § 3 below).

### 1.4 (Optional) NIM BYOK key

The dashboard runs in **demo mode** by default
(`ARGUS_DEMO_MODE=true`), which returns a pre-computed
verdict from `static/demo-result.json`. No external
service is contacted.

If you want **live NIM-backed analysis** through
`/submit`, set a Fly secret:

```sh
fly secrets set ARGUS_NIM_KEY=nvapi-...
fly secrets unset ARGUS_DEMO_MODE   # back to default? no — re-set explicitly:
fly secrets set ARGUS_DEMO_MODE=false
```

The NIM key never leaves the Fly machine at runtime —
it is read from the secret store on boot and held in
process memory only.

---

## 2. One-command deploy

From the repo root:

```sh
cd crates/argus-dashboard
fly launch --copy-config --no-deploy
fly deploy
```

What each command does:

- **`fly launch --copy-config --no-deploy`** — first-time
  setup. Reads `fly.toml`, asks for an org name, creates
  the `argus-dashboard` app in `iad`, and **skips** the
  initial build (we trigger it explicitly with `fly
  deploy`). Safe to re-run.
- **`fly deploy`** — builds the multi-arch Docker image
  (linux/amd64 + linux/arm64) from the local `Dockerfile`,
  pushes it to the Fly registry, and rolls the new
  release out to the `argus-dashboard` app. Takes 4-8
  minutes the first time (Rust dependency cache is
  cold), < 1 minute on subsequent deploys.

The default URL after deploy is
`https://argus-dashboard.fly.dev`.

---

## 3. Custom domain setup (argus.apohara.dev)

### 3.1 Add the cert to Fly

```sh
fly certs add argus.apohara.dev
```

Fly responds with the exact DNS records you need to
add. They look like this (the actual values will be in
the output of the `certs add` command):

```dns
# A records (IPv4)
argus.apohara.dev.   300   IN  A     66.241.125.5
argus.apohara.dev.   300   IN  A     66.241.125.78
argus.apohara.dev.   300   IN  A     66.241.125.79

# AAAA records (IPv6)
argus.apohara.dev.   300   IN  AAAA  2a09:8280:1::5c:5d8e
argus.apohara.dev.   300   IN  AAAA  2a09:8280:1::3a:5e57
argus.apohara.dev.   300   IN  AAAA  2a09:8280:1::47:eb6c
```

The A/AAAA approach is what Fly recommends for apex
subdomains because CNAMEs cannot coexist with other
records at the same name.

### 3.2 If you prefer a CNAME (cleaner, less brittle)

You can also use:

```dns
argus.apohara.dev.   300   IN  CNAME  argus-dashboard.fly.dev.
```

This works because `argus.apohara.dev` is a subdomain
(not the apex), and the parent zone already has its
own A/AAAA records that won't conflict.

**Recommendation:** use A/AAAA. It's what `fly certs
add` prints, it's the default path, and Fly's edge
will rotate IPs for HA without any work on your side.

### 3.3 Add the records at the registrar

Log in to wherever `apohara.dev` is hosted and create
the records from § 3.1. Propagation takes 30 seconds
to 10 minutes depending on TTL. Check with:

```sh
dig +short argus.apohara.dev        # should return Fly IPs
dig +short AAAA argus.apohara.dev   # should return Fly v6 IPs
```

### 3.4 Wait for Fly to provision the cert

```sh
fly certs show argus.apohara.dev
```

When the status flips from `Pending` to `Issued` (a
few minutes after DNS propagates), TLS is live.
`https://argus.apohara.dev` should now serve the
dashboard.

---

## 4. Verification (smoke test the live URL)

Run these checks after the first deploy:

```sh
# 1. HTTP 200 from the health endpoint
curl -fsSL https://argus.apohara.dev/api/health
# expected: {"status":"ok","service":"argus","version":"..."}

# 2. Landing page renders
curl -fsSL https://argus.apohara.dev/ | head -c 200
# expected: HTML starting with "<!DOCTYPE html>"

# 3. Demo endpoint returns the pre-computed verdict
curl -fsSL https://argus.apohara.dev/api/demo | head -c 200
# expected: JSON with `verdict`, `cohorts`, `chain_id` keys

# 4. Custom domain (not just fly.dev)
curl -fsSL https://argus-dashboard.fly.dev/ -o /dev/null -w "%{http_code}\n"
curl -fsSL https://argus.apohara.dev/        -o /dev/null -w "%{http_code}\n"
# both should be 200
```

From a browser, the site should show:

- Dark theme (`#0d1117` background, `#f78166` accent)
- 5 expandable PR-demo cards under the "See it analyze a
  PR right now" section
- The CordonEnforcer diagram in the architecture
  section
- The audit-chain preview in the footer

---

## 5. Common issues

### 5.1 "App not found" on first deploy

You skipped `fly launch --copy-config`. Run it once
from this directory; it registers the `argus-dashboard`
name with Fly and provisions the empty app.

### 5.2 Demo mode shows "demo fixture malformed"

The `static/demo-result.json` file failed to parse at
runtime. Re-check that the JSON in the image is valid
(it was the source of truth at build time):

```sh
fly ssh console -C "cat /usr/local/bin/../static/demo-result.json" \
  --app argus-dashboard
```

If the file is missing entirely, the `COPY
crates/argus-dashboard/static` line in the Dockerfile
isn't picking it up. Rebuild with `fly deploy
--no-cache`.

### 5.3 Health check fails (5xx on `/api/health`)

Two likely causes:

1. **The container is in a crash loop.** Stream the
   logs:
   ```sh
   fly logs --app argus-dashboard
   ```
   The most common cause is a missing environment
   variable the binary expects.

2. **The cold-start grace period hasn't elapsed.** The
   first request after `auto_stop_machines = "stop"`
   triggers a cold start. The `grace_period = "10s"`
   in `fly.toml` is the safety net — Fly won't fail
   the health check for the first 10s after boot.

### 5.4 NIM key not set, but demo mode is `false`

Set the secret and redeploy:

```sh
fly secrets set ARGUS_NIM_KEY=nvapi-...
fly secrets set ARGUS_DEMO_MODE=true   # safer than leaving it false
fly deploy
```

If you want the live mode intentionally, leave
`ARGUS_DEMO_MODE=false` and set the key.

### 5.5 Custom domain shows "Connection refused" or wrong cert

DNS hasn't propagated, or the cert is still `Pending`.
Re-check with:

```sh
dig +short argus.apohara.dev
fly certs show argus.apohara.dev
```

If the IPs are correct but the cert is still pending
for > 10 minutes, Fly's ACME challenge is failing.
Run `fly certs remove argus.apohara.dev` then
`fly certs add argus.apohara.dev` to re-issue.

### 5.6 Build OOM during `fly deploy`

The free-tier build machine has 8 GB of RAM. A clean
release build of the 15-crate workspace can spike to
~6 GB of link-time memory. If you hit OOM, the fix is
to pre-build locally with `mold` or `lld` and push the
binary as a builder image — but in practice this almost
never happens with the current crate set.

---

## 6. Rollback

Fly keeps the last several releases. To revert:

```sh
# List the most recent releases (newest first)
fly releases --app argus-dashboard

# Roll back to a specific version
fly releases rollback v<number> --app argus-dashboard
```

Or to roll back to the previous release immediately:

```sh
fly releases rollback --app argus-dashboard
```

The rollback is instant — Fly swaps the running machine
to the previous image. No DNS or TLS changes needed.

If the bad release left a broken image that won't
boot, you can also force a redeploy of an earlier tag:

```sh
fly deploy --image registry.fly.io/argus-dashboard:<sha>
```

Use `fly releases` to find the image SHA associated
with each version.

---

## 7. Monitoring

### 7.1 Logs

Live tail:

```sh
fly logs --app argus-dashboard
```

The binary emits structured `tracing` logs at
`info,argus=debug` (set in `fly.toml`). The
`tracing-subscriber` JSON formatter wraps them as
newline-delimited JSON for easy parsing.

Filter for errors only:

```sh
fly logs --app argus-dashboard | grep -i '"level":"ERROR"'
```

### 7.2 Metrics

Fly exposes per-app metrics in the dashboard:
https://fly.io/apps/argus-dashboard/metrics

Key panels:

- **Requests / sec** — overall traffic shape
- **CPU / memory** — `shared-cpu-1x` machines should
  stay under 50% CPU and 180 MB RAM at idle
- **Cold starts** — count of `auto_start_machines`
  events. If this is > 5/day, raise
  `min_machines_running` to 1 to keep a warm
  instance.

For a long-term metrics view, the `argus-otel` crate
exposes OpenTelemetry traces that can be scraped
through `/metrics` (when `ARGUS_OTEL_ENABLED=true`
is set as a secret). Pipe them to Honeycomb, Grafana
Cloud, or any OTLP-compatible backend.

### 7.3 Uptime checks

Fly's `[[http_service.checks]]` block in `fly.toml`
hits `/api/health` every 15 seconds. If the
endpoint 5xx's three times in a row, Fly marks the
machine unhealthy and restarts it. The full history
is in the Fly dashboard under
**Monitoring → Health checks**.

For external monitoring (independent of Fly's view),
point a service like UptimeRobot, BetterStack, or
Cronitor at `https://argus.apohara.dev/api/health`
with a 60-second interval.

### 7.4 Alarms

Set Fly metric alarms (CLI):

```sh
fly metrics alerts create \
  --app argus-dashboard \
  --name "argus-cpu-high" \
  --metric cpu \
  --threshold 80 \
  --window 5m \
  --notify your-email@example.com
```

Or use the dashboard's **Alerts** tab to do the same
through the UI.

---

## 8. Updating the deploy

To push a new version of the dashboard:

```sh
# 1. Bump the version in the workspace (optional, but
#    keep the dashboard's "version" in sync with
#    /api/health responses).
# 2. Build locally to catch obvious errors:
cargo build --release -p argus-dashboard
# 3. Run tests:
cargo test -p argus-dashboard
# 4. Deploy:
cd crates/argus-dashboard
fly deploy
```

The CI workflow should mirror steps 2-3 on every PR.
`fly deploy` does not run your CI — it builds the
Docker image directly on Fly's build machines.

---

## 9. Tear down

To remove the app entirely (irreversible, the URL is
freed for re-use by anyone):

```sh
fly apps destroy argus-dashboard
```

The free tier allows up to 3 full apps; the
`argus-dashboard` slot can be reclaimed if needed.

---

## 10. References

- [Fly.io configuration reference](https://fly.io/docs/reference/configuration/)
- [Fly.io custom domains](https://fly.io/docs/app-guides/custom-domains/)
- [Fly.io secrets](https://fly.io/docs/reference/secrets/)
- [Fly.io health checks](https://fly.io/docs/reference/health-checks/)
- ARGUS threat model: [`SECURITY.md`](../../../SECURITY.md)
- ARGUS changelog: [`CHANGELOG.md`](../../../CHANGELOG.md)
- apohara.dev: <https://apohara.dev>
