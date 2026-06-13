# Governance

This document describes how **ARGUS** is governed: how decisions are
made, who holds which roles, and how the project continues if the
maintainer becomes unavailable. It is intentionally lightweight and
honest about the project's current size (a single maintainer with
outside contributors welcome).

## Governance model

ARGUS follows a **single-maintainer (BDFL-style) model** with open,
consensus-seeking discussion:

- **Proposals and decisions happen in the open.** Features, changes,
  and bug reports are discussed in GitHub
  [Issues](https://github.com/SuarezPM/apohara-argus/issues) and
  [Pull Requests](https://github.com/SuarezPM/apohara-argus/pulls).
  Anyone may open an issue or PR.
- **The maintainer is the final decision-maker** on what is merged
  and released, but seeks consensus with contributors and prefers
  the least-surprising, best-justified option. Disagreements are
  resolved by discussion in the relevant issue / PR; the
  maintainer's decision is final if consensus is not reached.
- **Non-negotiable design principles** constrain every decision; a
  change that weakens them is rejected on principle:
  - **Offline-first, hybrid detection.** The deterministic slop
    layer runs first (<100ms, no network, no LLM call) and is
    the load-bearing guarantee of the pre-commit guard. The LLM
    layer is opt-in per call and BYOK for the NIM endpoint.
    Same input ⇒ same deterministic verdict ⇒ same bytes out —
    auditable and reproducible.
  - **EU AI Act Art. 12 Level 2 by default.** The 15-field
    `AuditEvent` (BLAKE3 chained, Ed25519 signed) is the
    regulator-facing artifact. A change that weakens the chain,
    drops a required field, or stops failing closed on
    classification is rejected.
  - **Honesty over hype.** Precision is **measured, not
    asserted**: a committed CI gate asserts `0` false positives
    and `0` false negatives on the curated slop corpus
    ([`crates/argus-slop/tests/benchmark.rs`](crates/argus-slop/tests/benchmark.rs)),
    and `SECURITY.md` publishes a per-component "covers / does
    NOT cover" threat model naming exactly what is still out
    of scope. No claim ships that a test cannot back.
  - **Lean, one workspace, no hosted service.** 14 Rust crates
    in a single workspace, no required external service,
    daemon, or account; the MCP form is a short-lived stdio
    process, not a daemon. The audit store defaults to
    **off**, and the SQLite / OpenTelemetry / A2A surfaces
    are all **opt-in** so the default build stays
    self-contained.

## Roles and responsibilities

| Role | Who | Responsibilities |
|------|-----|------------------|
| **Maintainer** | [@SuarezPM](https://github.com/SuarezPM) (Pablo) | Reviews and merges changes; cuts releases; triages issues and security reports; owns the GitHub / crates.io / npm credentials; final decision-maker. |
| **Security contact** | the maintainer, via [`SECURITY.md`](SECURITY.md) | Receives and responds to vulnerability reports (private GitHub Security Advisories); 5-day ack SLA; coordinates disclosure and the fix-or-won't-fix decision. |
| **Code of Conduct moderator** | the maintainer, via [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) | Receives and acts on conduct reports; runs the enforcement ladder from warning to permanent ban as the situation requires. |
| **Contributors** | anyone | Open issues / PRs; contributions are accepted per [`CONTRIBUTING.md`](CONTRIBUTING.md) and licensed under MIT. DCO sign-off (`git commit -s`) is required. |

There is currently **one maintainer**; the project actively welcomes
additional maintainers. A contributor with a sustained track record
of high-quality, on-principle contributions may be invited by the
maintainer to become a co-maintainer (gaining merge / release
rights and credential access under the continuity plan below).

## Access continuity (bus factor)

The project must be able to continue — create and close issues,
accept changes, and publish releases — within about a week even if
the maintainer becomes unavailable. The continuity plan:

- **Credential custody.** The credentials required to operate the
  project — the GitHub account (and repository admin), the
  crates.io API token, and any npm publish token — are stored in
  the maintainer's password manager, **with recovery / break-glass
  copies kept off-site** so a designated trusted party can recover
  access if the maintainer is incapacitated.
- **No on-site secret is load-bearing for users or releases.**
  Release binaries are signed via **keyless** Sigstore attestation
  (SLSA Build L3 provenance via GitHub OIDC — *no long-lived
  signing key to lose or rotate*; see [`SECURITY.md`](SECURITY.md)
  and the README). A downstream user keeps building from source,
  verifying, and running the tool regardless of the project's
  operational state.
- **Reproducible from source.** The repository is the single
  source of truth; anyone with the credentials can rebuild and
  re-publish from a clean checkout (`cargo build --release`, then
  a `vMAJOR.MINOR.PATCH` tag that drives the release + attestation
  workflows). `Cargo.lock` + `rust-toolchain.toml` pin the
  dependency graph and channel.
- **Fork-ability.** Under the permissive MIT license, the
  community can fork and continue the project without the
  maintainer's involvement if ever required.

> Maintainer action (kept current out-of-band): ensure the
> break-glass recovery copies are held by a trusted second party.
> This is the human half of the bus factor and is not something
> the repository can enforce on its own.

## Releases

Releases follow [Semantic Versioning](https://semver.org); each
release is a git tag (`vMAJOR.MINOR.PATCH`) that drives the release
workflow (`.github/workflows/release.yml`) — per-target prebuilt
binaries on a GitHub Release — and the crates.io publish workflow
(`.github/workflows/publish.yml`,
`cargo install apohara-argus`). The release **binaries** carry a
**SLSA Build L3** provenance attestation (Sigstore keyless),
generated by an isolated reusable workflow
(`.github/workflows/_attest.yml`) that holds the signing
permissions the build jobs do not — so a build job cannot forge
its own provenance. The attestation is verifiable with
`gh attestation verify --signer-workflow …`; the git tags
themselves are not GPG-signed. The changes per release are
recorded in [`CHANGELOG.md`](CHANGELOG.md).

## Changing this document

Changes to governance are proposed via pull request and decided
by the maintainer in the open, like any other change.
