# ARGUS Agent Spec

> The workflow as agent: skills, context, decisions, MCPs.

## Identity

- **Name:** ARGUS Aegis (the multi-agent collective)
- **Trust domain:** `spiffe://apohara.dev/argus/`
- **Per-instance ID:** `spiffe://apohara.dev/argus/aegis-{role}/instance/{uuid}`
- **Ed25519 keypair:** generated fresh per instance, all actions signed

## The 4 specialist agents

| # | Role | Internal name | Skill | LLM |
|---|---|---|---|---|
| 1 | Slop detector | `aegis-slop` | Detect AI-generated code signals (10 heuristics) | `meta/llama-3.1-70b-instruct` (default) |
| 2 | Security reviewer | `aegis-security` | Adversarial security analysis (15 categories) | same |
| 3 | Architecture checker | `aegis-arch` | Coherence with existing repo patterns | same |
| 4 | Verdict synthesizer | `aegis-verdict` | Combine the 3 outputs into an actionable verdict | same |

Plus:
- `aegis-orchestrator` (the runner, doesn't make LLM calls)
- `aegis-lens` (the weekly digest generator, makes 1 long LLM call)

## Context each agent receives

```
aegis-slop:
  input:  PR diff only
  output: { slop_score: 0-1, signals: [...], examples: [...], confidence: 0-1 }
  context: nothing (purely signal-based)

aegis-security:
  input:  PR diff only
  output: { highest_severity: None..Critical, findings: [...] }
  context: nothing (purely pattern-based)

aegis-arch:
  input:  PR diff + sample of repo files
  output: { fit_score: 0-1, verdict: ..., positives: [...], concerns: [...] }
  context: the sample provides the patterns to compare against

aegis-verdict:
  input:  3 structured JSON outputs (the above)
  output: Verdict { status, risk_score, summary, key_findings, action_items, reasoning }
  context: nothing else (Cordon Principle: no raw code access for the synthesizer)
```

## Decision rules

The verdict-synthesizer prompt encodes this logic:

```text
if (security highest_severity in {CRITICAL, HIGH}):
    verdict = HALTED
elif (slop_score > 0.7 AND fit_score > 0.5):
    verdict = HALTED  # likely AI slop that doesn't fit
elif (slop_score > 0.85 OR fit_score > 0.7):
    verdict = HALTED
elif (slop_score > 0.5 OR fit_score > 0.5 OR security has any MEDIUM):
    verdict = REVIEW_REQUIRED
else:
    verdict = APPROVED
```

The agent code (`crates/argus-slop/src/pipeline.rs::synthesize`) also implements
the same logic in Rust as a defense-in-depth fallback: if the LLM call fails, the
heuristic synthesis still runs.

## Tools (MCPs and direct integrations)

| Integration | Type | Used for | Status |
|---|---|---|---|
| NVIDIA NIM | HTTP (OpenAI-compatible) | All 4 LLM calls | Active (BYOK) |
| GitHub REST API | HTTP (`reqwest`) | PR fetching, comment posting, label setting | Active (optional, requires `GITHUB_TOKEN`) |
| Supabase Postgres | SQL (`sqlx`) | Audit ledger persistence | Types defined, runtime uses in-memory store (TODO) |
| HeyGen / D-ID | HTTP | AI avatar video rendering (Lens) | NOT IMPLEMENTED — the script is generated, video rendering skipped |

## Failure handling

- **LLM call fails:** the agent defaults to a conservative `Halted` verdict
  (defense in depth — better to over-block than under-block)
- **GitHub fetch fails:** the agent returns a clear error to the caller, no
  partial review is emitted
- **JSON parse fails:** the raw LLM response is logged for debugging; the agent
  returns a "no report" for that analyzer, the verdict still synthesizes from the
  others

## What the agent is NOT

- Not a code generator — the agent does not write or modify code
- Not a linter — the agent does not check formatting or syntax
- Not a CI replacement — the agent's verdict is a recommendation, not a gate
  (though it can be used as a gate via pre-commit hooks or PR merge policies)
- Not a code review substitute for humans — every verdict is a draft for a human
  to review and override
- Not magic — LLM-based analyzers can miss things; the verdict is a
  probabilistic recommendation, not a guarantee

## Operational characteristics

- **Cold call latency:** 4-12 seconds per analyzer (parallel: ~12-15 sec total for 3)
- **Total per PR review:** 30-50 seconds for 3-4 analyzers in parallel
- **Tokens per review:** ~3K-5K (estimate; exact depends on diff size)
- **Cost per review:** ~$0.004-0.005 (Llama 3.1 70B on NIM free tier)
- **Concurrent reviews:** Tokio handles thousands; no per-instance rate limit

## Security properties

- **BYOK:** no API keys are persisted; the user provides them per-request via
  HTTP header or CLI flag
- **No data exfiltration:** the diffs go only to NIM (with the user's key), no
  analytics, no telemetry
- **Signed audit chain:** every analysis is signed with ed25519; the chain is
  BLAKE3-hashed; offline-verifiable
- **No code execution:** the agent does not execute or modify any code; it only
  reads diffs and emits verdicts
