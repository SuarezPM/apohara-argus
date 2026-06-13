# ARGUS pricing

ARGUS ships in three tiers. The engine is open source under MIT and stays
that way. The Team and Enterprise tiers pay for the dashboard, the audit
chain infrastructure, and the support that keeps the engine maintained.

If you came here from the CISO angle (EU AI Act Article 12, SOC 2, ISO
27001), see [docs/for-ciso.md](./for-ciso.md) for the compliance-framed
version of the same offer.

## Tiers

| Tier | Price | What you get |
|---|---|---|
| **Open Source** | Free | Public repos, BYOK, MIT license, community support |
| **Team** | $15 / user / month | Private repos, audit chain export, GitHub App, email support |
| **Enterprise** | $500 / month (up to 50 devs) | Multi-tenant org dashboards, custom policy packs, SIEM export (Splunk / Datadog / Elastic), SAML SSO, on-call support, EU AI Act Art. 12 L2 evidence pack |

The Open Source tier is the same binary you build from the repo. The paid
tiers add a hosted dashboard, the audit chain signing service, and a UI
for non-developer stakeholders. Your diffs never leave your host (see
"Honest posture" below).

## What's included in every tier

The MIT-licensed engine ships the same in every tier. The paid tiers add
the hosted surface and the support layer.

- The **four specialists**: `aegis_slop` (deterministic slop detector),
  `aegis_security` (red-team review), `aegis_arch` (architecture fit),
  `aegis_verdict` (verdict synthesizer with CordonEnforcer isolation).
- The **MCP server** (`argus-mcp`) that exposes the four specialists to
  Claude Code, Codex, and Cursor over stdio JSON-RPC.
- The **audit chain**: 15-field `AuditEvent`, BLAKE3 hash chained,
  Ed25519 signed, EU AI Act Article 12 Level 2 conformant by default.
- The **deterministic layer** (regex + AST, SLOP-001..005, 0-FP / 0-FN
  on the committed corpus, under 100ms, no network, no LLM call).
- The **LLM semantic layer** (BYOK via `ARGUS_NIM_KEY`, fail-soft on
  NIM errors, per-specialist latency budgets, no silent downgrades to
  "approved").

## Why we charge

The pricing is **per user, not per call** because the LLM cost is not
ours. BYOK means you bring your own NVIDIA NIM key. The $0.004-$0.005
per PR review in [docs/numbers.md](./numbers.md) is your NIM cost, not
ours. We charge for the infrastructure that is *not* the LLM call: the
**audit chain integrity** (BLAKE3, Ed25519, the regulator-verifiable
schema), the **hosted dashboard** for non-developer stakeholders, and
the **maintenance** of the engine, corpus, fixtures, and CI.

The Team tier covers costs with a thin margin. The Enterprise tier is
where the margin lives, and that margin funds the open source tier. If
we gate-kept the engine on the dashboard, the OSS tier would die the
first time the maintainer got tired.

Per-user pricing matches the way security budgets work: it scales with
the team that gets the protection, not with the volume of code.

## How to upgrade

From Open Source to Team: install the GitHub App, sign in with your org,
pick the tier on the dashboard. No sales call required.

From Team to Enterprise (or for a team larger than 50 developers, a
custom SLA, on-prem dashboard, or a procurement process):

- Book a 30-minute call: <https://calendly.com/apohara-argus/30min>
- Or email the maintainer directly. We answer within one business day.

The source is on
[GitHub](https://github.com/SuarezPM/apohara-argus) and the docs
(`docs/agent-spec.md`, `docs/iteration-roadmap.md`) describe what is
shipped. We do not gate the technical answer behind a sales call.

## FAQ

**What is BYOK?**

Bring Your Own Key. You provide an NVIDIA NIM key via `ARGUS_NIM_KEY`
and ARGUS calls your NIM endpoint directly. ARGUS does not proxy, log,
or retain the request. You trust the NIM provider the same way you
trust it for any other call. See `SECURITY.md` § *LLM semantic layer*.

**Can I use Claude or OpenAI instead of NIM?**

The shipped specialists target NVIDIA NIM. Swapping providers means
writing a small adapter against `argus-llm` and bumping `policy_version`
(a breaking change for the audit chain). Open-weight models behind an
OpenAI-compatible endpoint are the realistic target. See
`docs/iteration-roadmap.md`.

**Do you host my diffs?**

No. The diff leaves your host only when the LLM semantic layer makes a
BYOK NIM call. The audit chain records metadata by default, and raw diff
text only when you opt in (`include_diff = true`), after secret
redaction and 64 KiB truncation. The audit file is local, mode 0600, and
you can disable the audit feature entirely. See `SECURITY.md` §
*Audit log*.

**What's the difference between Tier 1 and Tier 2?**

Tier 1 is the deterministic regex + AST layer (under 100ms, no
network, no LLM call, 0-FP / 0-FN on the committed corpus). Tier 2
is the LLM semantic layer (BYOK, four specialists, latency-budgeted).
Both run on the same diff. Tier 1 catches the high-confidence SLOP
patterns; Tier 2 catches the things Tier 1 cannot, like a 60-LOC
function with good variable names that no rule fires on. Tier 1 is on
by default and does not need a NIM key.

**What does "EU AI Act Art. 12 L2" mean?**

Article 12 is the "logging" requirement for high-risk AI systems.
Level 2 is the conformance tier ARGUS targets by default: the
15-field `AuditEvent` schema, the `DataClass` enum, the
`policy_version` field, and the retention scaffolding are in place.
The operator is responsible for the actual retention policy and the
model's own Art. 12 posture. The Enterprise tier ships a pre-built
evidence pack. See `SECURITY.md` § *EU AI Act Art. 12 conformance
posture* for the covers / does-NOT-cover list.

**What if my team is bigger than 50 devs?**

The Enterprise tier caps at 50 developers per $500/month. Larger teams
get a custom quote. Book a call (<https://calendly.com/apohara-argus/30min>)
for a per-seat discount, on-prem dashboard, or whatever your
procurement needs.

**How do I cancel?**

Email the maintainer. We cancel the subscription the same day, and the
dashboard data is purged within 30 days. The audit chain you exported
yourselves is yours to keep. The MIT-licensed open source engine does
not require a subscription to keep working.

## Honest posture

ARGUS is **offline-first and BYOK**. The Team and Enterprise tiers are
**self-hosted SaaS, not a hosted service for your diffs**. Your diffs
never leave your host. The hosted surface is the dashboard, the audit
chain signing service, and SSO, not your code. The 0-FP / 0-FN guarantee
holds on the **committed corpus** for the deterministic layer; the LLM
semantic layer inherits the model's accuracy. The honest posture is
"high-confidence on the deterministic layer, semantically strong on the
LLM layer, never 100%." See `SECURITY.md` § *Non-goals*.

---

Last updated: 2026-06-13. Pricing in USD, exclusive of VAT. The maintainer
reserves the right to change pricing with 30 days' notice to paying
customers; the Open Source tier stays free under the MIT license.
