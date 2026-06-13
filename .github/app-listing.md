# ARGUS — AI slop defense for code review

ARGUS is a GitHub App that runs a deterministic + (optional) LLM-based
review on every pull request and posts a verdict + label. It catches the
mechanical slop patterns that human reviewers miss: oversized functions,
swallowed errors, AI-flavored test stubs, and the regex-detectable
remnants of LLM-generated code. The deterministic layer is fast
(<100ms, no network) and runs by default; the LLM layer is BYOK and
optional.

## What ARGUS does

- Posts an inline comment on every PR with the deterministic slop
  layer's verdict, score, and findings.
- Sets a label on the PR — `argus/approved`,
  `argus/needs-review`, or `argus/halted` — so the verdict is visible
  in the PR list at a glance.
- Audits every review to a BLAKE3 hash chain (operator-side; not
  uploaded anywhere — see [Security](#security)).
- BLAKE3 fingerprints the diff + summary, so the operator has a
  tamper-evident marker per review.

## What ARGUS does NOT do

- **No 100% guarantee.** The deterministic layer's precision/recall
  is benchmarked at P=1.000, R=0.818 (F1=0.900) on a labeled corpus
  of 40 PRs. The LLM layer inherits the model's accuracy. We are
  honest: ARGUS is a high-confidence signal, not an oracle.
- **No hosted service, no SaaS mode.** The App is your deployment —
  the webhook secret and installation token live in your Fly.io /
  Cloud Run / k8s cluster, not on our infrastructure.
- **No code stored on our side.** The diff is fetched, analyzed
  in-memory, and fingerprinted. The BLAKE3 hash of the diff and the
  summary is logged; the raw diff is not retained.
- **No telemetry, no analytics.** The only outbound network call
  is the one to GitHub's API (to fetch the diff and post the
  comment/label) and the optional one to NVIDIA NIM (when
  `ARGUS_NIM_KEY` is set).

## Install

1. Click **Install** on this page.
2. Select the repositories you want ARGUS to review.
3. After the install, GitHub redirects you to a callback URL on
   your deployment (configured via `fly.toml` /
   `redirect_url` in the manifest). The callback receives the App's
   credentials (`ARGUS_APP_ID`, `ARGUS_APP_PRIVATE_KEY`,
   `ARGUS_APP_WEBHOOK_SECRET`, `ARGUS_APP_INSTALL_TOKEN`) — store
   them as secrets in your hosting platform.
4. Open a PR on a selected repo. ARGUS comments + labels within ~5s
   (deterministic) or ~20s (deterministic + LLM).

## Screenshots

<!--
  The three screenshots will be added in a follow-up commit once
  they exist. The paths below are placeholders so the README
  shape is locked in.
-->
1. PR comment showing the deterministic verdict:
   ![ARGUS PR comment](docs/screenshots/argus-pr-comment.png)
2. PR list view with the `argus/needs-review` label:
   ![ARGUS label on PR](docs/screenshots/argus-pr-label.png)
3. The GitHub App settings page after install:
   ![ARGUS App settings](docs/screenshots/argus-app-settings.png)

## Permissions

The App requests the minimum permissions required:

| Scope            | Access | Why                                                         |
| ---------------- | ------ | ----------------------------------------------------------- |
| `pull_requests`  | read   | To fetch the diff at review time.                           |
| `issues`         | write  | To post the verdict comment.                                |
| `metadata`       | read   | Required for every GitHub App — for repo enumeration.       |

The App does **not** request `contents: write`, `actions: write`, or
any other write scope beyond posting comments. The install token is
the only credential the App uses — there is no code path for a user
PAT.

## Pricing

- **Open source / public repos**: free. The deterministic layer
  alone is useful and runs in <100ms without any external service.
- **Private repos**: see the [pricing page](https://argus.apohara.dev/pricing)
  (P.3 in the roadmap). The paid tier adds the LLM layer (BYOK)
  and the EU AI Act Article 12 audit chain.

## Build / test

```sh
git clone https://github.com/SuarezPM/apohara-argus
cd apohara-argus
cargo build -p argus-github-app
cargo test  -p argus-github-app
```

## Security

ARGUS is a security tool, and the App respects that:

- The webhook signature is verified with constant-time compare
  (HMAC-SHA256, `subtle::ConstantTimeEq`).
- Payload size is capped at 10 MiB; oversize requests return 413.
- The CordonEnforcer scans every URL in the payload and rejects
  any non-GitHub host (SSRF defense).
- The App refuses to act on events outside the operator's
  allowlist (configurable via `ARGUS_APP_ALLOWED_REPOS`).

See the [SECURITY.md](https://github.com/SuarezPM/apohara-argus/blob/main/SECURITY.md)
threat model for the full list of covers / does NOT cover.

## License

MIT — see [LICENSE](https://github.com/SuarezPM/apohara-argus/blob/main/LICENSE).

## Support

- File an issue at <https://github.com/SuarezPM/apohara-argus/issues>
- Read the [AGENTS.md](https://github.com/SuarezPM/apohara-argus/blob/main/CLAUDE.md)
  for the project's working agreements.
- Private security disclosures: see `SECURITY.md` for the
  vulnerability disclosure process.
