# ARGUS for the CISO

## The audit chain your regulator will accept.

ARGUS is the AI slop defense layer for code review that produces the
evidence pack your compliance team needs on day one of the EU AI Act.
If you are a CISO whose engineering org is shipping AI-generated code
in production today, this page is for you. For the developer-focused
view, see [docs/pricing.md](./pricing.md).

## The three risks you are carrying

### 1. AI-generated code is in your repo today.

GitHub saw a 206% increase in AI-generated projects in 2025. A 2026
Opsera benchmark of 250,000 developers found AI-generated code carries
15-18% more security vulnerabilities than human-written code. A 2026
Sonar survey of 1,100 developers found 96% do not fully trust the
AI-generated code they review. The risk is not hypothetical. The curl
maintainer closed the public bug bounty in 2025 because 19 of 20
reports were AI hallucinations: invented stack traces, functions that
do not exist, imaginary vulnerabilities. arXiv followed with one-year
bans for researchers submitting AI-generated papers. "AI slop" was
Merriam-Webster's Word of the Year 2025.

### 2. Your existing code review tools do not catch slop.

CodeRabbit, Snyk, SonarQube, and Semgrep are designed for
human-written code. They catch syntax issues and known CVE patterns.
They do not catch the LLM-specific failure modes: silent hallucination
in a function that compiles and looks reasonable, unverified imports
from a model that confidently references a non-existent crate, prompt
injection in commit messages steering the reviewer toward approval.
ARGUS is the layer that catches what your existing stack was never
designed to look for.

### 3. EU AI Act Article 12 is a hard deadline, not a soft one.

Article 12 of the EU AI Act is the "logging" requirement for high-risk
AI systems: automatic event recording, tamper-evident retention,
6-24 month retention windows, and a regulator-readable audit trail.
The audit chain requirement is enforceable. Penalties for non-compliance
reach up to 7% of global annual revenue or 35 million EUR, whichever
is higher, under Article 99. A CISO who treats this as a "we will get
to it in 2027" item is exposing the company to a fine that
reorganizes the balance sheet.

## The ARGUS approach

### 1. BLAKE3 hash-chained audit trail

Every verdict is cryptographically chained to the previous one. A
modified or deleted event breaks the chain. A regulator pulling the
chain gets a single, verifiable, immutable story of what was reviewed
and what was approved. No retroactive edits are possible without
detection.

### 2. Ed25519 signed AuditEvents

Every entry in the chain is signed with an Ed25519 key. A regulator can
verify the chain offline, with their own tooling, without trusting
ARGUS. The signing key is process-local; the operator is responsible
for key custody. The verifier does not need to phone home.

### 3. EU AI Act Article 12 Level 2 conformant by default

The 15-field `AuditEvent` schema, the `DataClass` enum
(`None`, `SourceCode`, `Pii`, `Phi`, `Contract`, `Mixed`, `Unknown`),
the `policy_version` field, and the retention scaffolding are
pre-configured. Your team does not retrofit the schema to fit a
regulator's question. ARGUS is conformant on day one, off-by-default
in the open source engine, on-by-default in the Enterprise tier.

### 4. BYOK and offline-first

Your code never leaves your host. The LLM call goes to the
user-provided NVIDIA NIM endpoint, keyed by `ARGUS_NIM_KEY`. The audit
chain is local, mode 0600, owned by the operator. There is no data
residency issue because there is no data residency. The hosted
surface is the dashboard, the audit chain signing service, and SSO,
not your code.

## The evidence pack

When your team buys the Enterprise tier, your compliance team gets a
pre-built evidence pack you can hand to a regulator or an external
auditor without writing a single line of glue code.

- **Daily signed audit chain export.** PDF for human readers, JSONL
  for the auditor's verifier. The chain is signed end-to-end.
- **Quarterly compliance report.** Auto-generated from the chain,
  covers the retention window, the policy version, the
  classifications, and the verdicts. No manual aggregation.
- **On-demand query: "show me every PR that approved slop."** The
  query is immutable, signed, and reproducible by any third party
  holding the chain. The answer is the answer, not an interpretation
  of it.
- **SIEM export.** Splunk, Datadog, and Elastic integrations for SOC 2
  audit trails. The audit chain events flow into the SIEM your SOC
  team already operates; no new dashboard to learn.

## Talk to your CTO

The CISO does not buy this alone. The CTO and the platform team own
the integration, the GitHub App install, the policy pack, and the
rollout plan. ARGUS is the audit layer; it sits on top of the code
review your team is already doing.

- The tier-by-tier pricing: [docs/pricing.md](./pricing.md).
- Book a 30-minute call: <https://calendly.com/apohara-argus/30min>.
  Bring the CTO and the head of compliance.
- The source, the threat model, the audit schema, and the corpus are
  on [GitHub](https://github.com/SuarezPM/apohara-argus). The full
  "covers / does NOT cover" statement is in `SECURITY.md`. There is
  no demo behind a sales call; the technical answer is the technical
  answer.

## What we do not do

This section mirrors `SECURITY.md` § *Non-goals*. If a guarantee is
not on this list, do not assume it.

- We do not host your code. The diff leaves your host only when the
  LLM semantic layer makes a BYOK NIM call, and the NIM endpoint is
  yours.
- We do not train a model. ARGUS is the review layer, not the
  generation layer. The NIM model you choose is your decision and
  your responsibility.
- We do not ship to your customers' repos. ARGUS writes findings to
  your PR review; it does not push to a customer's codebase.
- We are the audit layer, not the build pipeline. We do not compile,
  test, deploy, or release your code. We produce the signed, hash
  chained, regulator-readable record of what was reviewed and why.

The 0-FP / 0-FN guarantee holds on the **committed corpus** for the
deterministic layer. The LLM semantic layer inherits the model's
accuracy. The honest posture is "high-confidence on the deterministic
layer, semantically strong on the LLM layer, never 100%." A
defense-in-depth posture, not a silver bullet.

---

Last updated: 2026-06-13. The compliance posture described on this
page reflects ARGUS v0.1 and the EU AI Act guidance published through
June 2026. Material changes to the audit chain schema, the signing
scheme, or the conformance claims are reflected in `SECURITY.md` and
`CHANGELOG.md` under `[Unreleased]`.
