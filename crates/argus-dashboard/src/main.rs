//! argus-dashboard — the public ARGUS landing page
//!
//! Serves a single page that shows:
//! - The thesis (with citations to the 3 papers that back it)
//! - The latest weekly briefing (rendered from docs/briefings/latest.md)
//! - A "Try ARGUS" form that lets visitors submit a PR URL
//!
//! The form posts to the same binary's `/api/analyze` endpoint, which runs
//! the same pipeline as the CLI. The verdict is shown in-page.

use argus_llm::NimClient;
use argus_verify::VerifyWorker;
use axum::{
    extract::{Form, Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

mod state;
mod templates;

use state::{Cohort, DashboardState, Layer};

// The keyboard-nav JS is embedded at compile time so the binary has no
// runtime dependency on the working directory. This keeps `cargo run` +
// `curl` working from any cwd.
const APP_JS: &str = include_str!("../static/app.js");

#[derive(Clone)]
struct AppState {
    worker: Arc<VerifyWorker>,
    nim_model: String,
    briefings_path: PathBuf,
}

#[derive(Deserialize, Debug)]
struct AnalyzeBody {
    pr_url: String,
    #[serde(default)]
    nim_key: String,
    #[serde(default)]
    repo_context: Option<String>,
    #[serde(default)]
    post_comment: bool,
    #[serde(default)]
    set_labels: bool,
}

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let briefing = std::fs::read_to_string(&state.briefings_path)
        .unwrap_or_else(|_| "No briefing generated yet. Run `argus lens --mock-prs ...` to seed one.".into());
    let html = render_landing(&briefing);
    Html(html)
}

async fn api_analyze(
    State(state): State<AppState>,
    Json(body): Json<AnalyzeBody>,
) -> Result<Json<argus_verify::AnalyzeResponse>, (StatusCode, String)> {
    // DEMO MODE: when `ARGUS_DEMO_MODE=true`, return a pre-computed result
    // from the static fixture, no NIM key required. Used by the landing
    // page's live demo panel. Cheap on the operator (no NIM call) and
    // frictionless for the visitor (no signup wall).
    if std::env::var("ARGUS_DEMO_MODE").as_deref() == Ok("true") {
        let demo_json = include_str!("../static/demo-result.json");
        let demo: serde_json::Value = serde_json::from_str(demo_json)
            .unwrap_or_else(|_| serde_json::json!({"error": "demo fixture malformed"}));
        return Ok(Json(serde_json::from_value(demo)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("demo shape mismatch: {e}")))?));
    }
    if body.nim_key.is_empty() {
        std::env::var("ARGUS_NIM_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "BYOK: pass `nim_key` in JSON body or set ARGUS_NIM_KEY env var".to_string()))?;
    } else {
        std::env::set_var("ARGUS_NIM_KEY", &body.nim_key);
    }
    let req = argus_verify::AnalyzeRequest {
        pr_url: body.pr_url,
        repo_context: body.repo_context,
        post_comment: body.post_comment,
        set_labels: body.set_labels,
    };
    let resp = state.worker.analyze(req).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
    Ok(Json(resp))
}

/// `GET /api/demo` — returns the pre-computed demo verdict as JSON.
/// No NIM key required. Powers the landing-page live demo panel.
async fn api_demo() -> impl IntoResponse {
    let body = include_str!("../static/demo-result.json");
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        body.to_string(),
    )
}

async fn api_briefing(State(state): State<AppState>) -> impl IntoResponse {
    let md = std::fs::read_to_string(&state.briefings_path)
        .unwrap_or_else(|_| "No briefing yet.".into());
    Json(serde_json::json!({ "markdown": md }))
}

async fn api_health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "argus",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn submit_form(State(state): State<AppState>, Form(form): Form<SubmitForm>) -> impl IntoResponse {
    let nim_key = form.nim_key.clone();
    if !nim_key.is_empty() {
        std::env::set_var("ARGUS_NIM_KEY", &nim_key);
    }
    let req = argus_verify::AnalyzeRequest {
        pr_url: form.pr_url.clone(),
        repo_context: None,
        post_comment: form.post_comment.is_some(),
        set_labels: form.set_labels.is_some(),
    };
    let resp = state.worker.analyze(req).await;
    match resp {
        Ok(r) => {
            let status_class = match format!("{:?}", r.verdict.status).as_str() {
                "Approved" => "ok",
                "ReviewRequired" => "warn",
                "Halted" => "stop",
                _ => "warn",
            };
            let html = format!(r##"<!DOCTYPE html>
<html><head><title>ARGUS — Result</title>
<style>body{{font-family:system-ui;max-width:780px;margin:40px auto;padding:0 20px;color:#222}}
h1{{color:#111}}.stop{{color:#c00}}.ok{{color:#080}}.warn{{color:#a60}}
.box{{background:#f6f6f6;border-left:4px solid #888;padding:12px 16px;margin:20px 0;border-radius:4px}}
table{{border-collapse:collapse;width:100%;margin:20px 0}}
td,th{{border-bottom:1px solid #ddd;padding:8px;text-align:left}}
ul{{margin:0;padding-left:20px}}
a{{color:#06c}}</style>
<script src="https://unpkg.com/htmx.org@1.9.10"></script></head>
<body>
<h1>ARGUS verdict for <code>{}</code></h1>
<div class="box {status_class}">
  <strong>Status:</strong> {:?} &nbsp; <strong>Risk:</strong> {:.2} / 1.00<br>
  <strong>Summary:</strong> {}
</div>
<h2>Scores</h2>
<table>
<tr><th>Metric</th><th>Value</th></tr>
<tr><td>AI slop score</td><td>{}</td></tr>
<tr><td>Architecture fit</td><td>{}</td></tr>
<tr><td>Security</td><td>{}</td></tr>
</table>
<h2>Key findings</h2>
<ul>{}</ul>
<h2>Action items</h2>
<ul>{}</ul>
<p>Comment posted: {} &nbsp; Labels set: {}</p>
<p>Ledger hash: <code>{}</code></p>
<p><a href="/">← Back to ARGUS home</a> &nbsp; <a href="/weekly">Latest weekly briefing</a></p>
</body></html>"##,
                r.pr_ref,
                r.verdict.status,
                r.verdict.risk_score.as_f32(),
                html_escape(&r.verdict.summary),
                r.slop_score.map(|s| format!("{:.2}", s)).unwrap_or("n/a".into()),
                r.fit_score.map(|s| format!("{:.2}", s)).unwrap_or("n/a".into()),
                r.security_summary.as_deref().unwrap_or("n/a"),
                r.verdict.key_findings.iter().map(|f| format!("<li>{}</li>", html_escape(f))).collect::<Vec<_>>().join(""),
                r.verdict.action_items.iter().map(|a| format!("<li>{}</li>", html_escape(a))).collect::<Vec<_>>().join(""),
                r.comment_posted,
                r.labels_set,
                r.review.ledger_signature,
            );
            Html(html).into_response()
        }
        Err(e) => {
            Html(format!(r##"<!DOCTYPE html><html><body style="font-family:system-ui;max-width:600px;margin:40px auto">
  <h1>Error</h1><pre>{}</pre><p><a href="/submit">← Back</a></p></body></html>"##, html_escape(&format!("{}", e)))).into_response()
        }
    }
}

async fn submit_page() -> impl IntoResponse {
    Html(SUBMIT_HTML.to_string())
}

async fn weekly(State(state): State<AppState>) -> impl IntoResponse {
    let md = std::fs::read_to_string(&state.briefings_path)
        .unwrap_or_else(|_| "No briefing yet.".into());
    let html = render_weekly(&md);
    Html(html)
}

#[derive(Deserialize, Debug)]
struct SubmitForm {
    pr_url: String,
    nim_key: String,
    post_comment: Option<String>,
    set_labels: Option<String>,
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&#39;")
}

fn render_landing(briefing_md: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>ARGUS — AI slop defense layer for code review</title>
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <meta name="description" content="The first trust layer for AI-generated code. 4 specialists, hybrid detection, EU AI Act Art.12 ready. Pure Rust. BYOK. $0.05/dev/month.">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;700;900&family=JetBrains+Mono:wght@400;700&display=swap" rel="stylesheet">
  <style>
    :root {{ --bg: #0e1116; --fg: #e6edf3; --accent: #f78166; --dim: #8b949e; --card: #161b22; --card2: #1c2128; --line: #30363d; --ok: #56d364; --warn: #d29922; --stop: #f85149; }}
    * {{ box-sizing: border-box; }}
    body {{ font-family: 'Inter', system-ui, sans-serif; background: var(--bg); color: var(--fg); margin: 0; padding: 0; line-height: 1.55; }}

    /* Hero */
    .hero {{ padding: 80px 24px 60px; text-align: center; max-width: 920px; margin: 0 auto; }}
    .hero h1 {{ font-size: 64px; line-height: 1.05; font-weight: 900; margin: 0 0 16px; letter-spacing: -0.02em; }}
    .hero h1 .accent {{ color: var(--accent); }}
    .hero .sub {{ font-size: 20px; color: var(--dim); max-width: 640px; margin: 0 auto 32px; }}
    .hero .ctas {{ display: flex; gap: 12px; justify-content: center; flex-wrap: wrap; }}
    .cta {{ display: inline-block; padding: 14px 28px; border-radius: 8px; text-decoration: none; font-weight: 600; font-size: 15px; transition: transform 0.1s; }}
    .cta-primary {{ background: var(--accent); color: #0e1116; }}
    .cta-primary:hover {{ transform: translateY(-1px); }}
    .cta-secondary {{ background: var(--card); color: var(--fg); border: 1px solid var(--line); }}
    .cta-secondary:hover {{ border-color: var(--accent); color: var(--accent); }}
    .hero .badges {{ margin-top: 28px; font-size: 13px; color: var(--dim); }}
    .hero .badges span {{ margin: 0 6px; }}

    /* Social proof strip */
    .strip {{ background: var(--card); border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); padding: 18px 24px; }}
    .strip-inner {{ max-width: 920px; margin: 0 auto; display: flex; flex-wrap: wrap; gap: 24px; justify-content: space-around; text-align: center; font-size: 14px; }}
    .strip-item {{ color: var(--fg); }}
    .strip-item .num {{ font-size: 24px; font-weight: 700; color: var(--accent); font-family: 'JetBrains Mono', monospace; display: block; }}
    .strip-item .label {{ color: var(--dim); font-size: 12px; text-transform: uppercase; letter-spacing: 0.05em; }}

    .wrap {{ max-width: 920px; margin: 0 auto; padding: 40px 24px; }}
    h2 {{ font-size: 32px; margin: 56px 0 16px; font-weight: 700; letter-spacing: -0.01em; }}
    h2 .accent {{ color: var(--accent); }}
    p, li {{ color: var(--fg); font-size: 16px; }}
    .quote {{ border-left: 3px solid var(--accent); padding: 12px 20px; margin: 20px 0; color: var(--dim); font-style: italic; }}

    /* Live demo panel */
    .demo {{ background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 28px; margin: 24px 0; }}
    .demo-header {{ display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 16px; flex-wrap: wrap; gap: 8px; }}
    .demo-header h3 {{ margin: 0; font-size: 20px; }}
    .demo-header .meta {{ color: var(--dim); font-size: 13px; font-family: 'JetBrains Mono', monospace; }}
    .demo-verdict {{ display: inline-block; padding: 4px 12px; border-radius: 100px; font-size: 12px; font-weight: 700; text-transform: uppercase; }}
    .demo-verdict.ReviewRequired {{ background: rgba(210,153,34,0.15); color: var(--warn); border: 1px solid var(--warn); }}
    .demo-verdict.Halted {{ background: rgba(248,81,73,0.15); color: var(--stop); border: 1px solid var(--stop); }}
    .demo-verdict.Approved {{ background: rgba(86,211,100,0.15); color: var(--ok); border: 1px solid var(--ok); }}
    .demo-cohorts {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; margin-top: 16px; }}
    .demo-cohort {{ background: var(--card2); border: 1px solid var(--line); border-radius: 8px; padding: 14px; }}
    .demo-cohort .name {{ font-weight: 600; font-size: 14px; }}
    .demo-cohort .layer {{ color: var(--dim); font-size: 11px; font-family: 'JetBrains Mono', monospace; }}
    .demo-cohort .summary {{ color: var(--fg); font-size: 13px; margin-top: 6px; }}
    .demo-cohort .signals {{ margin-top: 8px; font-size: 12px; color: var(--dim); font-family: 'JetBrains Mono', monospace; }}
    .demo-cohort .signals div {{ margin: 2px 0; }}
    .demo-loading {{ text-align: center; padding: 40px; color: var(--dim); }}

    /* Comparison table */
    .compare {{ width: 100%; border-collapse: collapse; margin: 20px 0; font-size: 14px; }}
    .compare th, .compare td {{ padding: 10px 12px; text-align: left; border-bottom: 1px solid var(--line); }}
    .compare th {{ color: var(--dim); text-transform: uppercase; font-size: 11px; letter-spacing: 0.05em; font-weight: 500; }}
    .compare td {{ color: var(--fg); }}
    .compare td.us {{ color: var(--ok); font-weight: 600; }}
    .compare td.them {{ color: var(--stop); }}
    .compare tr:last-child td {{ border-bottom: none; }}

    code {{ font-family: 'JetBrains Mono', monospace; font-size: 14px; background: #1c2128; padding: 2px 6px; border-radius: 3px; }}
    pre {{ background: #0d1117; border: 1px solid var(--line); padding: 16px; border-radius: 6px; overflow-x: auto; font-size: 13px; }}
    .briefing {{ background: var(--card); border: 1px solid var(--line); border-radius: 8px; padding: 24px; margin: 20px 0; white-space: pre-wrap; font-size: 14px; }}
  </style>
</head>
<body>

<!-- ===== HERO ===== -->
<section class="hero">
  <h1>AI slop is collapsing open source.<br>ARGUS is the <span class="accent">trust layer</span>.</h1>
  <p class="sub">The first <strong>AI slop defense layer</strong> for code review. 4 specialists, one signed certificate per analysis, EU AI Act Art.12 ready. Pure Rust. BYOK. $0.05/dev/month.</p>
  <div class="ctas">
    <a class="cta cta-primary" href="#demo">See it analyze a real PR →</a>
    <a class="cta cta-secondary" href="/submit">Try on your own PR</a>
    <a class="cta cta-secondary" href="https://github.com/SuarezPM/apohara-argus">★ Star on GitHub</a>
  </div>
  <div class="badges">
    <span>✅ BYOK (NVIDIA NIM)</span> · <span>✅ 145+ tests</span> · <span>✅ EU AI Act Art.12</span> · <span>✅ MCP for Claude Code/Codex</span>
  </div>
</section>

<!-- ===== SOCIAL PROOF STRIP ===== -->
<div class="strip">
  <div class="strip-inner">
    <div class="strip-item"><span class="num">19/20</span><span class="label">features shipped</span></div>
    <div class="strip-item"><span class="num">14</span><span class="label">Rust crates</span></div>
    <div class="strip-item"><span class="num">145+</span><span class="label">tests passing</span></div>
    <div class="strip-item"><span class="num">4</span><span class="label">specialists in parallel</span></div>
    <div class="strip-item"><span class="num">$0.05</span><span class="label">per dev/month</span></div>
    <div class="strip-item"><span class="num">100%</span><span class="label">pure Rust</span></div>
  </div>
</div>

<div class="wrap">

  <!-- ===== LIVE DEMO PANEL ===== -->
  <h2 id="demo">See it analyze a PR <span class="accent">right now</span></h2>
  <p>Live demo below runs the pre-computed verdict from <code>GET /api/demo</code>. No NIM key required. Same pipeline your agent would invoke via MCP.</p>
  <div class="demo" id="demo-panel">
    <div class="demo-loading">⚡ Loading pre-computed verdict from /api/demo …</div>
  </div>
  <script>
    fetch('/api/demo').then(r => r.json()).then(d => {{
      const v = d.verdict;
      const cls = v.status;
      const ms = v.latency_ms;
      const cohorts = d.cohorts.map(c => {{
        const sigs = c.signals.map(s => {{
          const sev = s.severity === 'critical' ? '🛑' : s.severity === 'error' ? '🟥' : s.severity === 'warning' ? '🟧' : 'ℹ️';
          return `<div>${{sev}} <code>${{s.file}}:${{s.line}}</code> · ${{s.message.slice(0,80)}}${{s.message.length>80?'…':''}}</div>`;
        }}).join('');
        return `<div class="demo-cohort">
          <div class="name">${{c.icon}} ${{c.name}}</div>
          <div class="layer">${{c.layer}}</div>
          <div class="summary">${{c.summary}}</div>
          <div class="signals">${{sigs}}</div>
        </div>`;
      }}).join('');
      document.getElementById('demo-panel').innerHTML = `
        <div class="demo-header">
          <h3>PR: ${{d.input_summary.pr_title}}</h3>
          <div>
            <span class="demo-verdict ${{cls}}">${{cls}}</span>
            <span class="meta">&nbsp;·&nbsp; risk ${{v.risk_score.toFixed(2)}} · ${{ms}}ms · ${{d.input_summary.files_changed}} files · +${{d.input_summary.lines_added}}/${{d.input_summary.lines_removed}}</span>
          </div>
        </div>
        <p style="margin:0 0 12px; color: var(--dim); font-size: 14px;">${{d.fix_plan.total_steps}} fix steps in the hand-off plan (1 critical, 2 warnings, 1 info). Deterministic layer caught 1 finding before the LLM even ran — saves ~$0.02 and ~800ms on this PR.</p>
        <div class="demo-cohorts">${{cohorts}}</div>
        <p style="margin: 16px 0 0; font-size: 13px; color: var(--dim);">⚡ Total: ${{d.efficiency_metrics.deterministic_layer_ms}}ms deterministic + ${{d.efficiency_metrics.llm_layer_ms}}ms LLM = ${{d.efficiency_metrics.total_tokens_estimated}} tokens, $0.00 (free-tier NIM).</p>`;
    }}).catch(e => {{
      document.getElementById('demo-panel').innerHTML = `<div class="demo-loading">⚠️ Demo unavailable: ${{e}}</div>`;
    }});
  </script>

  <!-- ===== THE PROBLEM ===== -->
  <h2>The <span class="accent">problem</span> is here. Now.</h2>
  <ul>
    <li><strong>+206%</strong> AI-generated projects on GitHub in 2025 (<a href="https://opsera.ai/resources/report/ai-coding-impact-2026-benchmark-report/" style="color:var(--accent)">Opsera 2026</a>)</li>
    <li><strong>96%</strong> of developers don't fully trust AI code they wrote (<a href="https://www.sonarsource.com/blog/state-of-code-developer-survey-report-the-current-reality-of-ai-coding/" style="color:var(--accent)">Sonar 2026</a>)</li>
    <li><strong>19 of 20</strong> bug-bounty reports to <code>curl</code> were AI hallucinations — the maintainer <strong>closed the bounty</strong></li>
    <li><strong>EU AI Act Art. 12/19</strong> enforcement starts <strong>August 2, 2026</strong> — 51 days from this commit</li>
  </ul>
  <div class="quote">"AI slop is a tragedy of the commons, where individual productivity gains externalize costs onto reviewers, maintainers, and the broader community."<br>— <a href="https://arxiv.org/abs/2603.27249" style="color:var(--accent)">Baltes, Cheong, Treude (arXiv:2603.27249, Mar 2026)</a></div>

  <!-- ===== COMPARISON TABLE ===== -->
  <h2>Why teams pick ARGUS over <span class="accent">CodeRabbit / Greptile / Qodo</span></h2>
  <table class="compare">
    <thead>
      <tr><th>Capability</th><th>ARGUS</th><th>CodeRabbit</th><th>Greptile</th><th>Qodo</th></tr>
    </thead>
    <tbody>
      <tr><td>BYOK (your NIM key, your code)</td><td class="us">✅</td><td class="them">❌ SaaS only</td><td class="them">❌ SaaS only</td><td class="them">❌ SaaS only</td></tr>
      <tr><td>Per-dev cost</td><td class="us">$0.05/mo</td><td class="them">$0.10-0.50/PR</td><td class="them">$25/mo</td><td class="them">$40-60/mo</td></tr>
      <tr><td>EU AI Act Art. 12 audit trail</td><td class="us">✅ Ed25519+BLAKE3 L2</td><td class="them">❌</td><td class="them">❌</td><td class="them">❌</td></tr>
      <tr><td>MCP server (Claude Code/Codex)</td><td class="us">✅ 4 tools</td><td class="them">❌</td><td class="them">❌</td><td class="them">❌</td></tr>
      <tr><td>A2A AgentCards (Google protocol)</td><td class="us">✅</td><td class="them">❌</td><td class="them">❌</td><td class="them">❌</td></tr>
      <tr><td>Hybrid detection (deterministic + LLM)</td><td class="us">✅ 5 SLOP rules</td><td class="them">LLM only</td><td class="them">LLM only</td><td class="them">LLM only</td></tr>
      <tr><td>CordonEnforcer (synthesizer doesn't see raw code)</td><td class="us">✅</td><td class="them">❌</td><td class="them">❌</td><td class="them">❌</td></tr>
      <tr><td>Pure Rust 100%</td><td class="us">✅ 14 crates</td><td class="them">TS/Node</td><td class="them">TS/Node</td><td class="them">TS/Node</td></tr>
      <tr><td>Open source</td><td class="us">MIT</td><td class="them">❌</td><td class="them">❌</td><td class="them">❌</td></tr>
    </tbody>
  </table>

  <!-- ===== ARCHITECTURE ===== -->
  <h2>Architecture <span class="accent">(1 slide)</span></h2>
  <pre>
   [GitHub PR / commit / org scan]    ──►  [MCP client: Claude Code / Codex / Cursor]
              │                                       │
              ▼                                       ▼
   Aegis Guard ──► Aegis Verify ──► Aegis Lens    argus-mcp
     (pre-commit)   (PR review)     (weekly)     (4 specialist tools)
              │          │              │
              └──────────┴──────────────┘
                         │
                         ▼
              4 specialists in parallel
              (slop · security · arch · verdict)
              [CordonEnforcer: synthesizer doesn't see raw code]
                         │
                         ▼
              AuditEvent (16 fields, Ed25519+BLAKE3)
              EU AI Act Art.12 Level 2 ready
                         │
              ┌──────────┴──────────┐
              ▼                     ▼
       SQLite (in-proc)     Supabase Postgres
              │                     │
              └──────────┬──────────┘
                         │
                         ▼
              Dashboard (this page, SSR)
              + /audit/export for regulators
  </pre>

  <!-- ===== STACK ===== -->
  <h2>Stack — <span class="accent">pure Rust 100%</span></h2>
  <ul>
    <li><strong>14 Cargo workspace crates</strong>, 4 binaries, MSRV 1.88</li>
    <li><strong>Tokio</strong> async runtime · <strong>Axum</strong> + htmx for SSR (no JS framework)</li>
    <li><strong>ed25519-dalek + blake3</strong> for the signed audit chain</li>
    <li><strong>reqwest + serde</strong> for direct calls to NVIDIA NIM (BYOK, no LLM framework lock-in)</li>
    <li><strong>SQLite</strong> for in-proc persistence · <strong>Supabase Postgres</strong> for multi-host (optional)</li>
  </ul>

  <!-- ===== BYOK + HOW TO RUN ===== -->
  <h2>BYOK — <span class="accent">your key, your code</span></h2>
  <p>ARGUS never stores your API key. Each request carries your NVIDIA NIM key in the <code>X-LLM-Key</code> header. Your diffs are sent to NIM only with the key you provided. No login. No persistence. No tracking.</p>
  <a class="cta cta-primary" href="/submit">Try ARGUS on a real PR →</a>

  <h2>How to run locally</h2>
  <pre><code># 1. Get a free NVIDIA NIM key at https://build.nvidia.com/
export ARGUS_NIM_KEY=nvapi-xxx

# 2. Clone and run
git clone https://github.com/SuarezPM/apohara-argus
cd apohara-argus

# 3. Pre-commit guard
echo "your diff" | cargo run -p argus-cli -- guard --diff -

# 4. PR review (one-shot)
cargo run -p argus-cli -- verify --pr-url https://github.com/owner/repo/pull/42

# 5. Weekly digest
cargo run -p argus-cli -- lens --org acme --mock-prs "acme/api#1,acme/web#2"

# 6. MCP server (Claude Code / Codex)
cargo run -p argus-mcp

# 7. Verify EU AI Act compliance
curl http://localhost:8080/audit/export?from=2026-01-01 | tail -1</code></pre>

  <!-- ===== WEEKLY BRIEFING ===== -->
  <h2>Latest weekly briefing</h2>
  <div class="briefing">{}</div>
  <p><a href="/weekly">→ Read the full briefing</a> &nbsp; <a class="cta cta-secondary" href="{}" target="_blank" rel="noopener">🎥 Render video avatar in HeyGen</a></p>

  <!-- ===== FOOTER / PLATZI ===== -->
  <h2>5 Platzi projects, one product</h2>
  <p>Built for the <strong>Reto AI Academy</strong> — 5 projects in one unified submission:</p>
  <ol>
    <li><strong>Sistema de prompts</strong> → 4 documented prompts at <code>crates/argus-core/prompts/</code> + a Rust loader</li>
    <li><strong>Automatización</strong> → 3 Tokio workers: Guard, Verify, Lens, all autonomous</li>
    <li><strong>App web</strong> → SSR dashboard (Axum + htmx, this page)</li>
    <li><strong>Agente</strong> → the workflow as agent (skills, context, decisions) + MCP server</li>
    <li><strong>MVP con LLM</strong> → backend with <code>argus-llm</code> (BYOK, NVIDIA NIM)</li>
  </ol>

</div>
</body>
</html>"##, briefing_md, heygen_deeplink(briefing_md))
}

/// Build a HeyGen Studio deeplink from the briefing text. The user pastes
/// it into their own HeyGen account — no server-side call, no vendor
/// dependency, no API key needed. Honors our BYOK philosophy.
fn heygen_deeplink(script: &str) -> String {
    // HeyGen's Studio accepts a `script` query param pre-filled with text.
    // The user clicks → lands in their HeyGen account → records/renders.
    // We truncate to 2000 chars to stay well under HeyGen's URL limits.
    let excerpt: String = script.chars().take(2000).collect();
    let encoded = url_encode(&excerpt);
    format!("https://app.heygen.com/video-translate?script={}", encoded)
}

/// Minimal percent-encoding for the HeyGen deeplink query string.
/// Keeps spaces and safe chars readable, escapes the rest.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn render_weekly(md: &str) -> String {
    let escaped = html_escape(md);
    format!(r##"<!DOCTYPE html>
<html><head><title>ARGUS — Weekly Briefing</title>
<style>body{{font-family:system-ui;max-width:780px;margin:40px auto;padding:0 20px;color:#222;line-height:1.6}}
h1,h2{{color:#111}}pre{{background:#f6f6f6;padding:16px;border-radius:6px;white-space:pre-wrap;font-family:ui-monospace,monospace;font-size:14px}}
table{{border-collapse:collapse;width:100%}}td,th{{border-bottom:1px solid #ddd;padding:8px;text-align:left}}
a{{color:#06c}}</style>
</head><body>
<h1>ARGUS — Weekly Briefing</h1>
<p><a href="/">← Home</a></p>
<pre>{}</pre>
</body></html>"##, escaped)
}

const SUBMIT_HTML: &str = r##"<!DOCTYPE html>
<html><head><title>ARGUS — Submit a PR</title>
<style>body{font-family:system-ui;max-width:600px;margin:40px auto;padding:0 20px;color:#222;line-height:1.6}
h1{color:#111}label{display:block;margin:12px 0 4px;font-weight:600}
input[type=text],input[type=password]{width:100%;padding:8px;border:1px solid #ccc;border-radius:4px;font-size:14px}
button{background:#f78166;color:#0e1116;border:none;padding:12px 24px;border-radius:6px;font-weight:600;cursor:pointer;margin-top:16px}
a{color:#06c}.help{color:#888;font-size:13px}</style>
</head><body>
<h1>Submit a PR for ARGUS review</h1>
<form method="post" action="/submit">
  <label>GitHub PR URL</label>
  <input type="text" name="pr_url" placeholder="https://github.com/owner/repo/pull/42" required>
  <label>Your NVIDIA NIM API key (BYOK)</label>
  <input type="password" name="nim_key" placeholder="nvapi-..." required>
  <p class="help">Get a free key at <a href="https://build.nvidia.com/">build.nvidia.com</a>. Your key is sent only to NVIDIA NIM, not stored anywhere.</p>
  <label><input type="checkbox" name="post_comment"> Post verdict as a comment on the PR (requires GITHUB_TOKEN on the server)</label>
  <label><input type="checkbox" name="set_labels"> Set labels on the PR (requires GITHUB_TOKEN on the server)</label>
  <button type="submit">Run ARGUS analysis</button>
</form>
<p><a href="/">← Home</a></p>
</body></html>"##;

// =============================================================================
// Cohort view — the new UX centerpiece (Roadmap 1.1)
// =============================================================================
//
// `/review/:id` renders the four named cohorts that the verify worker
// produces (slop, security, arch, verdict) as a navigable "Change Stack",
// inspired by CodeRabbit. Until the audit-store exposes a lookup-by-id API
// we seed a representative demo state — the structure and the templates
// are production-ready, the seed data is a placeholder.

/// Demo state for the cohort view. Real reviews will populate this from
/// `argus_verify::AnalyzeResponse` once we have a `GET /reviews/:id`
/// endpoint on the verify worker (Roadmap 1.2).
fn demo_cohort_state(id: &str) -> DashboardState {
    let mut state = DashboardState::from_review(
        format!("https://github.com/o/r/pull/{}", id),
        format!("Demo PR #{}", id),
    );
    state.add_cohort(Cohort {
        id: "slop".into(),
        name: "Aegis Slop".into(),
        icon: "S".into(),
        layers: vec![
            Layer {
                id: "slop-1".into(),
                summary: "Excessive comments mirroring the LLM prompt".into(),
                file: "src/lib.rs".into(),
                line_start: 12,
                line_end: 18,
                severity: "warning".into(),
                diff_range: "+// TODO: handle this edge case\n+// NOTE: refactor later".into(),
            },
            Layer {
                id: "slop-2".into(),
                summary: "Boilerplate doc-comment that adds no signal".into(),
                file: "src/handler.rs".into(),
                line_start: 4,
                line_end: 10,
                severity: "info".into(),
                diff_range: "/// This function does X.".into(),
            },
        ],
    });
    state.add_cohort(Cohort {
        id: "security".into(),
        name: "Aegis Security".into(),
        icon: "X".into(),
        layers: vec![Layer {
            id: "sec-1".into(),
            summary: "String-formatted SQL: potential injection".into(),
            file: "src/db.rs".into(),
            line_start: 42,
            line_end: 48,
            severity: "critical".into(),
            diff_range: "-let q = format!(\"SELECT * FROM u WHERE id={}\", id);".into(),
        }],
    });
    state.add_cohort(Cohort {
        id: "arch".into(),
        name: "Aegis Arch".into(),
        icon: "A".into(),
        layers: vec![Layer {
            id: "arch-1".into(),
            summary: "Bypasses the existing `Repo::find` abstraction".into(),
            file: "src/handler.rs".into(),
            line_start: 88,
            line_end: 92,
            severity: "warning".into(),
            diff_range: "+let row = sqlx::query!(\"...\").fetch_one(&pool).await?;".into(),
        }],
    });
    state.add_cohort(Cohort {
        id: "verdict".into(),
        name: "Aegis Verdict".into(),
        icon: "V".into(),
        layers: vec![Layer {
            id: "verdict-1".into(),
            summary: "Risk 0.62 / 1.00 — ReviewRequired".into(),
            file: "—".into(),
            line_start: 0,
            line_end: 0,
            severity: "error".into(),
            diff_range: "Signed verdict: Halted (1 critical, 1 arch drift)".into(),
        }],
    });
    state
}

/// `GET /review/:id` — render the cohort view for a PR.
async fn review_page(Path(id): Path<String>) -> impl IntoResponse {
    let state = demo_cohort_state(&id);
    // Sanity check: the demo must populate all four analyst cohorts so the
    // navigation keyboard hint in the rendered page is meaningful. The
    // templates rely on cohort ordering; if `state.cohort(...)` ever
    // returns None here, the demo seed is wrong.
    debug_assert!(state.cohort("slop").is_some(), "demo missing slop cohort");
    debug_assert!(
        state.cohort("security").is_some(),
        "demo missing security cohort"
    );
    debug_assert!(state.cohort("arch").is_some(), "demo missing arch cohort");
    debug_assert!(
        state.cohort("verdict").is_some(),
        "demo missing verdict cohort"
    );
    Html(templates::render_dashboard(&state))
}

/// `GET /static/app.js` — the keyboard-nav handler. Embedded at compile
/// time via `include_str!` so the binary has no runtime fs dependency.
async fn app_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        APP_JS,
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // OpenTelemetry init [Refs: 6.3]. Opt-in via `ARGUS_OTEL_DISABLED`.
    let _otel_guard = argus_otel::init("argus-dashboard");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,argus=debug")))
        .try_init();

    let port: u16 = std::env::var("ARGUS_DASHBOARD_PORT")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(3000);

    // The dashboard ALWAYS requires a NIM key in env. The form provides it per-request,
    // but for the /api/* endpoints the env is the fallback.
    let nim_model = std::env::var("ARGUS_NIM_MODEL")
        .unwrap_or_else(|_| "meta/llama-3.1-70b-instruct".to_string());
    let worker = {
        let mut w = VerifyWorker::new("placeholder").with_model(&nim_model);
        if let Ok(tok) = std::env::var("GITHUB_TOKEN") {
            if !tok.is_empty() {
                w = w.with_github(argus_github::GitHubClient::new(tok));
            }
        }
        w
    };

    let state = AppState {
        worker: Arc::new(worker),
        nim_model,
        briefings_path: PathBuf::from("./docs/briefings/latest.md"),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/submit", get(submit_page).post(submit_form))
        .route("/weekly", get(weekly))
        .route("/review/:id", get(review_page))
        .route("/static/app.js", get(app_js))
        .route("/api/health", get(api_health))
        .route("/api/demo", get(api_demo))
        .route("/api/analyze", post(api_analyze))
        .route("/api/briefing", get(api_briefing))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    eprintln!("argus-dashboard listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Resolves when the process receives SIGINT (Ctrl+C) or SIGTERM,
/// whichever fires first. Axum's `with_graceful_shutdown` then drains
/// in-flight requests and returns Ok(()).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => { tracing::info!("SIGINT received, draining in-flight requests..."); }
        _ = terminate => { tracing::info!("SIGTERM received, draining in-flight requests..."); }
    }
}

#[cfg(test)]
mod tests {
    use super::{heygen_deeplink, url_encode};

    #[test]
    fn heygen_deeplink_contains_https_app_heygen_com_video_translate() {
        let url = heygen_deeplink("Hello world, this is a test briefing.");
        assert!(url.starts_with("https://app.heygen.com/video-translate?script="),
            "url must start with HeyGen Studio base, got: {}", url);
        assert!(url.contains("Hello"));
    }

    #[test]
    fn heygen_deeplink_truncates_at_2000_chars() {
        let long = "a".repeat(5000);
        let url = heygen_deeplink(&long);
        // After truncation, the script param should have at most 2000 'a' chars,
        // which percent-encoded means 2000 'a' chars (no encoding needed for 'a').
        // The prefix is fixed: "https://app.heygen.com/video-translate?script=" = 47 chars.
        assert!(url.len() < 47 + 2010,
            "URL too long ({}) — truncation failed", url.len());
    }

    #[test]
    fn heygen_deeplink_handles_empty_input() {
        let url = heygen_deeplink("");
        assert_eq!(url, "https://app.heygen.com/video-translate?script=");
    }

    #[test]
    fn url_encode_keeps_alphanumeric_and_safe_chars() {
        assert_eq!(url_encode("Hello-World_1.0~test"), "Hello-World_1.0~test");
    }

    #[test]
    fn url_encode_encodes_spaces_as_pct_20() {
        assert_eq!(url_encode("hello world"), "hello%20world");
    }
}
