# The ARGUS Thesis

> Why the open source contract is dying, and what it takes to fix it.

## TL;DR

In 2025, GitHub saw a **206% increase in AI-generated projects**. The
maintainer of curl **closed the bug bounty** because 19 of 20 security
reports were AI hallucinations. arXiv announced **1-year bans** for
researchers submitting AI-generated papers. **"AI slop" was Merriam-Webster's
Word of the Year 2025.**

The bottleneck of software is no longer code generation. **It's
verification.**

ARGUS is the verification infrastructure for the AI-generated-code era.

---

## 1. The crisis in numbers (June 2026)

| Source | Finding |
|---|---|
| Brais Moure / GitHub 2025 | AI projects on GitHub grew **206% in 2025** |
| iqSource (2026-06-05) | **41% of shipped code in 2025 was AI-generated**, with a 1.7× higher defect rate |
| Opsera 2026 Benchmark (250K developers) | AI-generated PRs wait **4.6× longer in review** and introduce **15-18% more security vulnerabilities** |
| Sonar 2026 (1,100 developers) | **42% of all committed code is AI-generated**. **96% of developers don't fully trust AI-generated code**. Only **48% always verify it before committing** |
| arXiv 2603.27249 (Baltes, Cheong, Treude, Mar 2026) | AI slop documented as a **"tragedy of the commons"**: individual productivity gains externalize costs onto reviewers and maintainers |
| Curl bug bounty (2025-2026) | Maintainer **closed the program** because 19 of 20 reports were AI hallucinations: invented stack traces, functions that don't exist, imaginary vulnerabilities |
| arXiv ban policy (May 2026) | **1-year bans** for researchers submitting papers with incontrovertible AI-generated errors |
| Merriam-Webster | **"AI slop" = Word of the Year 2025** |

## 2. The cultural shift

Before 2024, the contract of open source was simple: **useful code is welcome**.
A maintainer would review what came in and merge it if it was good. The
"good" filter was applied by humans, who are slow but reliable.

In 2024-2026, the cost of generating code collapsed to near-zero. AI agents
can write thousands of lines in seconds. But human attention didn't get
faster. The bottleneck inverted:

> The scarce resource is no longer code. **It's human review.**

Maintainers are responding the only way they can:
- **Closing bug bounties** (curl) because triaging AI hallucinations consumes
  more time than fixing real bugs
- **Disabling PRs entirely** on popular projects (GitHub enabled the
  feature in 2025)
- **Migrating to invitation-only contribution** for the first time in OSS
  history

## 3. What this paper says

**arXiv:2603.27249 — "An Endless Stream of AI Slop" (2026-03-28)**
Authors: Sebastian Baltes, Marc Cheong, Christoph Treude.

Methodology: qualitative analysis of 1,154 posts in 15 Reddit and Hacker News
threads. Three clusters of findings:

1. **Review Friction** — AI slop burdens reviewers and erodes trust
2. **Quality Degradation** — damage to codebases, knowledge resources, and
   developer competence
3. **Forces and Consequences** — systemic incentives that mandate AI
   adoption while externalizing the costs

The paper's central framing, with the same force as "tragedy of the
commons" papers in environmental economics:

> *"AI slop as a tragedy of the commons, where individual productivity gains
> externalize costs onto reviewers, maintainers, and the broader community."*

This is the problem ARGUS is built to address.

## 4. Why existing solutions aren't enough

The current market offers three categories of partial solutions, all
incomplete:

**A. AI code reviewers (CodeRabbit, Greptile, Qodo, Sourcery)**
- They review code, but they don't distinguish "human-quality" from
  "AI slop" — they just look for bugs.
- They don't produce verifiable, signed evidence of what was reviewed.
- They have no way to say "this PR shows the author didn't understand the
  system."

**B. Linters and SAST (SonarQube, Snyk, Semgrep)**
- They catch syntax issues and known CVE patterns.
- They don't understand the PR in context. They don't know if the
  approach is right for *this* repo.

**C. Human review (the traditional way)**
- Slow, expensive, doesn't scale.
- Now overwhelmed by the volume of AI-generated PRs.
- Suffering from burnout ("I quit. The clankers won." — HN, 2025).

ARGUS is the missing layer: a **multi-agent, signed, evidence-producing
review** that distinguishes "AI-assisted quality" from "AI-generated noise",
operates at three points in the SDLC, and produces a **verifiable
certificate** of what was reviewed and why.

## 5. What ARGUS is

ARGUS is a three-layer trust infrastructure:

1. **Aegis Guard** — runs at the moment of commit, before the PR exists.
   Catches the most obvious AI slop signals and blocks the commit with a
   clear explanation. This is the cheap, fast filter that prevents 60-70%
   of slop from ever leaving the developer's machine.

2. **Aegis Verify** — runs at the moment of PR review. Four parallel
   analyzers (slop, security, architecture fit, verdict synthesizer)
   produce a signed `PRReviewCertificate` that any maintainer can verify
   offline. This is the slow, deep review that replaces 25-40 minutes of
   human work per PR with 5-10 minutes of human editing of a draft.

3. **Aegis Lens** — runs weekly. Scans every PR across the org, produces
   a "slop radar" visualization, and emits a 60-90 second AI avatar
   briefing from the "CTO" summarizing the state of AI slop in the
   organization. This is the visibility that didn't exist before.

All three layers write to a **signed audit ledger** (ed25519 + BLAKE3 hash
chain). Every finding, every verdict, every action is attributed to the
specific agent that took it. The ledger is independently verifiable.

## 6. The evidence ARGUS produces

For every analysis, ARGUS emits:

```json
{
  "id": "uuid",
  "pr_ref": "github.com/owner/repo/pull/42",
  "pr_commit_hash": "sha256:...",
  "verdict": {
    "status": "APPROVED|REVIEW_REQUIRED|HALTED",
    "risk_score": 0.0-1.0,
    "summary": "1-2 sentences for the maintainer",
    "key_findings": ["..."],
    "action_items": ["..."],
    "reasoning": "..."
  },
  "findings": [
    {
      "agent": "aegis-slop|aegis-security|aegis-arch",
      "severity": "INFO|LOW|MEDIUM|HIGH|CRITICAL",
      "file": "...",
      "line": 42,
      "category": "...",
      "description": "...",
      "quote": "...",
      "recommendation": "..."
    }
  ],
  "agent_chain": [
    {
      "agent": "aegis-slop",
      "action": "ANALYZED",
      "timestamp": "...",
      "ed25519_sig": "..."
    }
  ],
  "final_signature": "ed25519:...",
  "rfc3161_timestamp": "..."
}
```

This is the **PR Review Certificate (PRC)**. It is signed, timestamped,
and offline-verifiable. A regulator or auditor can confirm exactly what
was reviewed, by whom, and with what conclusion.

## 7. What ARGUS is not

- Not a code linter. (We don't check formatting.)
- Not a code review tool that competes with humans. (We produce drafts for
  humans to edit.)
- Not a code-generation tool. (We don't write code.)
- Not a closed-source SaaS that stores your code. (BYOK, no persistence of
  your data, no login required.)
- Not magic. (LLM-based analyzers can miss things. The verdict is a
  recommendation, not a guarantee.)

## 8. The numbers, again

| Metric | Before ARGUS | With ARGUS |
|---|---|---|
| Time to review a typical PR | 25-40 min | 5-10 min (edit the bot's draft) |
| AI slop bugs reaching production | ~5-10/month/team | ~1-2/month/team |
| Manual weekly reporting | 4-6 hrs/manager | 0 (Aegis Lens auto-emits) |
| Reviewer trust in submitted PRs | Low | Quantified per-PR (signed verdict) |
| Audit trail | Slack scrollback | Cryptographically signed ledger |

## 9. Why now

The crisis is real, the papers are fresh (arXiv 2603.27249, March 2026), the
regulators are moving (EU AI Act Article 12 guidance, June 2026), and the
tools don't exist yet. ARGUS is the first system that combines:

- Multi-agent adversarial review (the academic pattern, validated by
  AuditAgent and the Cordon Principle)
- BYOK LLM inference (no vendor lock-in, no data persistence)
- Cryptographic evidence chain (the SPIFFE / Ed25519 / RFC 3161 stack)
- Three-layer SDLC coverage (commit, PR, org)

into a single Rust binary that a single maintainer can deploy in a
weekend.

That's the thesis. The rest of this repo is the implementation.
