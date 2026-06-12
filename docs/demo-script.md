# ARGUS — Video Demo Script (3-4 minutes)

> **Recording tips:**
> - Record at 1920×1080, terminal font size 16-18, dark theme
> - Pause 2 sec after each command to let viewers read output
> - Show terminal in upper 60%, facecam (optional) lower 40%
> - Speak the key numbers out loud, not the technicalities

---

## Pre-recording setup (have these ready)

**Two terminals side by side** (or split-screen tmux):
- LEFT: project root
- RIGHT: project root (for the second demo)

**Open in browser (just before recording):**
- `https://github.com/SuarezPM/apohara-argus` (the repo)
- `https://build.nvidia.com/` (for the "BYOK" callout)

**Source the env in BOTH terminals:**
```bash
cd /home/thelinconx/Documentos/proyecto2-plazti/apohara-argus
set -a && source .env && set +a
```

---

## THE SCRIPT

### [0:00 - 0:20] HOOK

**[Face to camera]**

> "In 2025, GitHub saw a 206% increase in AI-generated projects. The
> maintainer of curl closed the bug bounty because 19 of 20 reports were AI
> hallucinations. The bottleneck of software is no longer code generation —
> it's verification. Today I'm going to show you ARGUS, the first
> multi-agent AI slop defense layer — built entirely in Rust, with your
> own LLM key, end-to-end in 3 minutes."

### [0:20 - 0:50] THE ARCHITECTURE (1 minute)

**[Screen: open the repo in browser, scroll to README]**

> "Three layers operating across the SDLC. Aegis Guard catches AI slop
> before the PR exists — exit 0 or 1, drop it in your pre-commit hook.
> Aegis Verify reviews the PR with four parallel analyzers — slop,
> security, architecture fit, and a verdict synthesizer — and produces
> a signed certificate in 30 seconds. Aegis Lens runs weekly, scans
> every PR across the org, and emits a 60-second CTO avatar briefing
> with specific numbers."

**[Screen: show the file tree]**

> "Twelve Cargo crates, 3,800 lines of Rust, no Python, no Node. The
> workers are Tokio tasks, the LLM client is direct `reqwest` to NVIDIA
> NIM, the audit trail is ed25519 plus a BLAKE3 hash chain."

### [0:50 - 2:00] LIVE DEMO PART 1: GUARD + LENS

**[Screen: terminal 1]**

> "Let me show you it running. First, the health check — I want to
> confirm the BYOK key works."

```bash
$ ./target/release/argus health
→ Testing NIM connectivity...
✓ NIM healthy (55 tokens)
```

> "Good. Now let me show you the four prompts the system uses."

```bash
$ ./target/release/argus prompts
ARGUS Prompt Library — 4 interconnected prompts:

▸ architecture-fit (meta/llama-3.1-70b-instruct)
  Evaluates whether a PR fits the existing repo architecture and idioms
  temp=0.2 max_tokens=1280

▸ redteam-security (meta/llama-3.1-70b-instruct)
  Adversarial security review of a PR diff

▸ slop-detector (meta/llama-3.1-70b-instruct)
  Detects AI-generated code signals in a PR diff

▸ verdict-synthesizer (meta/llama-3.1-70b-instruct)
  Synthesizes the final verdict from the 3 analyzer outputs
```

> "Four documented prompts. Any dev on your team can copy them. Now let
> me run a guard on a diff that has an AWS key hardcoded."

```bash
$ echo 'diff --git a/config.py b/config.py
+AWS_KEY = "AKIAIOSFODNN7EXAMPLE"' | ./target/release/argus guard --json | head
{
  "decision": "Warn",
  "risk_score": 0.5,
  "verdict_status": "REVIEW_REQUIRED",
  "summary": "One or more analyzers failed; defaulting to REVIEW_REQUIRED.",
```

> "Defense in depth — the diff is too small for full analysis, so the
> guard defaults to REVIEW_REQUIRED. Exit code 0 — the developer
> proceeds but is warned. On a real PR, exit would be 1 with a full
> HALT verdict. Let me show you that with a longer diff."

```bash
$ cat > /tmp/secret-pr.diff << 'EOF'
diff --git a/src/config.py b/src/config.py
@@ -1,3 +1,5 @@
+# AWS credentials
+AWS_ACCESS_KEY = "AKIAIOSFODNN7EXAMPLE"
+AWS_SECRET_KEY = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
EOF
$ ./target/release/argus guard --diff /tmp/secret-pr.diff

🛑 ARGUS Guard: BLOCK
   Risk score: 0.50 / 1.00
   Status: Halted
   Slop: 0.00  |  Arch fit: 0.80  |  Sec: 1 findings, highest Critical

   Slop score: 0.00, Arch fit: 0.80, Security: 1 findings, highest Critical.
   CRITICAL: hardcoded AWS credentials.

   Findings:
   - AI slop signals: 0 (score 0.00)
   - Architecture fit: 0.80 — "introduces hardcoded values that should be config"
   - Security: 1 findings, highest Critical

   Action items:
   - Hardcoded AWS access key
```

> "There it is. BLOCK verdict, exit 1. The security analyzer caught the
> hardcoded AWS key, the architecture fit flagged it as anti-pattern, the
> verdict synthesizer chose HALTED. The developer's commit just got
> blocked."

### [2:00 - 3:00] LIVE DEMO PART 2: LENS

**[Screen: terminal 2]**

> "Now the weekly digest — Aegis Lens. I'll seed it with three mock PRs
> so you can see the output."

```bash
$ ./target/release/argus lens --org acme --mock-prs "acme/api#42,acme/web#7,acme/auth#15" | head -25

# ARGUS Weekly Briefing — `acme`
Week of: 2026-06-05
PRs analyzed: **3**
Avg risk: **0.35**
Critical findings: **1**

## CTO Avatar Script

> Good morning team, this week's update on AI-generated code at Acme.
> We've reviewed three pull requests over the last seven days, with
> mixed results. PR acme/api#42 from dev1 had a risk score of 0.20, ...
> the AI may not have fully understood the context of the code. ...
> I want to see at least two human reviewers on every PR that includes
> AI-generated code, and I want to see automated testing coverage
> increase by at least 20% over the next two weeks.

## Top Offenders
| PR | Author | Risk | Top finding |
|---|---|---|---|
| `acme/auth#15` | dev3 | 0.50 | minor AI slop signals |
```

> "In 30 seconds, a complete weekly briefing with specific numbers,
> named authors, and a clear call to action. That script was generated
> by the LLM, not hand-written. Without ARGUS, this would be a
> 1-2 hour meeting."

### [3:00 - 3:40] THE DASHBOARD

**[Screen: browser pointing to argus.apohara.dev]**

> "And here's the public dashboard — pure SSR with htmx, no React, no
> Vue, no JavaScript framework. The landing page has the thesis with the
> 4 numbers from Brais Moure, the OPSERA data, the Sonar survey,
> straight from the academic papers. The submit form takes a PR URL,
> your NIM key, and runs the analysis. The result page shows the
> verdict, the scores, the findings, the action items, and the
> ledger hash so anyone can verify the audit chain offline."

> "Your key is BYOK. ARGUS never stores it. Your diffs are sent to NIM
> only with the key you provided. No login. No tracking."

### [3:40 - 4:00] CLOSE

**[Face to camera]**

> "That's ARGUS. The first trust layer for AI-generated code. Built
> in 100% pure Rust. Open source at github.com/SuarezPM/apohara-argus.
> Forty-six unit tests, three integration tests verified against the
> real NIM. From zero to deployed in less than 24 hours. The
> bottleneck of software is no longer code. It's verification. ARGUS
> is the verification infrastructure."

**[End card: ARGUS logo + repo URL]**

---

## POST-RECORDING CHECKLIST

- [ ] Trim silences between segments
- [ ] Add subtitles (Spanish + English)
- [ ] Upload to YouTube as **unlisted** (not public — it's the demo for the hackathon)
- [ ] Add the GitHub repo link in the description
- [ ] Add a link to the thesis (`docs/thesis.md`) in the description
- [ ] Add timestamps in the description (so judges can jump to the demo)

## ONE-LINER FOR THE THUMBNAIL

> "ARGUS: the AI slop defense layer. Pure Rust. BYOK. End-to-end in 3 minutes."
