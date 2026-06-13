# argus-github-app

GitHub App front door for [ARGUS](https://github.com/SuarezPM/apohara-argus) — receives
`pull_request` webhooks and posts the deterministic slop layer's verdict as a
comment + label on the PR.

This crate is the binary that runs behind the App install. It is the 15th
crate in the [ARGUS workspace](../../Cargo.toml) and the front door for the
`argus-verify` analyzer.

## What it does

1. Receives a GitHub webhook for `pull_request.opened` /
   `.synchronize` / `.reopened`.
2. Verifies the HMAC-SHA256 signature (constant-time compare, defends
   against timing attacks).
3. Runs the deterministic slop layer (5 regex rules, <100ms, no
   network) on the PR diff.
4. Posts a comment summarising the verdict and sets a label
   (`argus/approved` / `argus/needs-review` / `argus/halted`).

The LLM layer is **optional** (BYOK; set `ARGUS_NIM_KEY` to enable).
Without it, only the deterministic layer runs and only the
`argus/approved` / `argus/needs-review` labels are emitted.

## Endpoints

| Method | Path      | Purpose                                      |
| ------ | --------- | -------------------------------------------- |
| GET    | `/`       | Landing page (plain text)                    |
| GET    | `/health` | Liveness probe (`{"ok": true}`)              |
| GET    | `/version`| Service name + version + git SHA             |
| GET    | `/setup`  | GitHub App manifest JSON + install URL       |
| POST   | `/webhook`| Receives GitHub events (HMAC-signed)         |

## One-click deploy

The included `Dockerfile` + `fly.toml` let you deploy to Fly.io's
free tier (256 MB RAM, 1 shared CPU) in three commands:

```sh
fly auth login
fly launch --copy-config --no-deploy   # uses ./fly.toml
fly secrets set \
  ARGUS_APP_WEBHOOK_SECRET=... \
  ARGUS_APP_INSTALL_TOKEN=...
fly deploy
```

The image is multi-stage (`rust:1.88-slim` → `distroless/cc-debian12`),
runs as the `nonroot` user, and weighs in at < 100 MB.

## Configuration

All configuration is via environment variables. See
[`AppConfig::from_env`](src/app_state.rs) for the full list. The
required vars:

- `ARGUS_APP_WEBHOOK_SECRET` — HMAC secret
- `ARGUS_APP_INSTALL_TOKEN` — GitHub App installation token
  (the App gets this from GitHub after the user installs it; we
  do **not** support user PATs).

Optional vars:

- `PORT` — bind port (default 8080)
- `ARGUS_APP_LABEL_PASS` / `ARGUS_APP_LABEL_WARN` / `ARGUS_APP_LABEL_FAIL`
- `ARGUS_APP_ALLOWED_REPOS` — comma-separated `owner/repo` allowlist
- `ARGUS_APP_EVENTS` — comma-separated event names
- `ARGUS_NIM_KEY` — BYOK for the LLM layer

## Security

The crate's [CordonEnforcer](src/cordon.rs) enforces:

- 10 MiB payload cap (Cordon rejects with 413)
- HMAC-SHA256 signature verification (constant-time compare)
- Repo allowlist (configurable; empty = "allow all installs")
- Event allowlist (default: `pull_request` only)
- SSRF defense: any URL in the payload must point at a GitHub
  host (`github.com`, `api.github.com`, `githubusercontent.com`)

See [`SECURITY.md`](../../SECURITY.md) for the full threat model.

## License

MIT — same as the parent workspace.
