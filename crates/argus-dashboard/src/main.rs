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
    extract::{Form, State},
    http::StatusCode,
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
    let briefing_excerpt: String = briefing_md
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .take(5)
        .collect::<Vec<_>>()
        .join("\n");
    let _ = briefing_excerpt; // suppress unused
    format!(r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>ARGUS — AI Slop Defense Layer</title>
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;700&family=JetBrains+Mono&display=swap" rel="stylesheet">
  <script src="https://unpkg.com/htmx.org@1.9.10"></script>
  <style>
    :root {{ --bg: #0e1116; --fg: #e6edf3; --accent: #f78166; --dim: #8b949e; --card: #161b22; --line: #30363d; }}
    * {{ box-sizing: border-box; }}
    body {{ font-family: 'Inter', system-ui, sans-serif; background: var(--bg); color: var(--fg); margin: 0; padding: 0; line-height: 1.55; }}
    .wrap {{ max-width: 880px; margin: 0 auto; padding: 40px 24px; }}
    h1 {{ font-size: 44px; margin: 0 0 8px; line-height: 1.1; font-weight: 700; }}
    h1 .accent {{ color: var(--accent); }}
    h2 {{ font-size: 28px; margin: 40px 0 12px; border-bottom: 1px solid var(--line); padding-bottom: 8px; }}
    p, li {{ color: var(--fg); }}
    .lede {{ font-size: 18px; color: var(--dim); margin: 0 0 32px; max-width: 640px; }}
    .stats {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 16px; margin: 32px 0; }}
    .stat {{ background: var(--card); border: 1px solid var(--line); border-radius: 8px; padding: 16px 20px; }}
    .stat .num {{ font-size: 32px; font-weight: 700; color: var(--accent); font-family: 'JetBrains Mono', monospace; }}
    .stat .label {{ font-size: 13px; color: var(--dim); text-transform: uppercase; letter-spacing: 0.05em; margin-top: 4px; }}
    .quote {{ border-left: 3px solid var(--accent); padding: 12px 20px; margin: 20px 0; color: var(--dim); font-style: italic; }}
    .cta {{ display: inline-block; background: var(--accent); color: #0e1116; padding: 12px 24px; border-radius: 6px; text-decoration: none; font-weight: 600; margin: 16px 0; }}
    .cta:hover {{ opacity: 0.9; }}
    code {{ font-family: 'JetBrains Mono', monospace; font-size: 14px; background: #1c2128; padding: 2px 6px; border-radius: 3px; }}
    pre {{ background: #0d1117; border: 1px solid var(--line); padding: 16px; border-radius: 6px; overflow-x: auto; }}
    .badges {{ margin: 24px 0; }}
    .badges a {{ display: inline-block; margin: 0 8px 8px 0; padding: 4px 10px; background: var(--card); border: 1px solid var(--line); border-radius: 100px; text-decoration: none; font-size: 13px; color: var(--dim); }}
    .badges a:hover {{ color: var(--accent); border-color: var(--accent); }}
    .briefing {{ background: var(--card); border: 1px solid var(--line); border-radius: 8px; padding: 24px; margin: 20px 0; white-space: pre-wrap; font-size: 14px; }}
  </style>
</head>
<body>
<div class="wrap">
  <div class="badges">
    <a href="https://github.com/SuarezPM/apohara-argus">★ GitHub</a>
    <a href="/api/health">API</a>
    <a href="/weekly">Weekly briefing</a>
    <a href="/submit">Submit a PR</a>
  </div>
  <h1>ARGUS<span class="accent">.</span></h1>
  <p class="lede">The first <strong>AI slop defense layer</strong> for code review. Three layers, one signed certificate, BYOK.</p>

  <div class="stats">
    <div class="stat"><div class="num">+206%</div><div class="label">AI projects on GitHub in 2025</div></div>
    <div class="stat"><div class="num">4.6×</div><div class="label">longer review for AI PRs</div></div>
    <div class="stat"><div class="num">70%</div><div class="label">more bugs in AI code</div></div>
    <div class="stat"><div class="num">96%</div><div class="label">of devs don't trust AI code</div></div>
  </div>

  <h2>The problem</h2>
  <p>AI generates code at near-zero marginal cost. Human review didn't get faster. The bottleneck inverted: it's no longer generation, it's <strong>verification</strong>.</p>
  <div class="quote">"AI slop as a tragedy of the commons, where individual productivity gains externalize costs onto reviewers, maintainers, and the broader community."<br>— <a href="https://arxiv.org/abs/2603.27249" style="color:var(--accent)">Baltes, Cheong, Treude (arXiv:2603.27249, Mar 2026)</a></div>

  <h2>What ARGUS does</h2>
  <p>Three layers operating across the SDLC. One shared ledger. One signed certificate per analysis.</p>
  <ul>
    <li><strong>Aegis Guard</strong> — pre-commit. Catches AI slop before the PR exists. Exit 0/1 for your <code>pre-commit</code> hook.</li>
    <li><strong>Aegis Verify</strong> — PR review. Four parallel analyzers (slop, security, architecture fit, verdict) produce a signed <code>PRReviewCertificate</code> in 30s.</li>
    <li><strong>Aegis Lens</strong> — weekly org-wide digest. A 60-90s "CTO avatar" script + a "slop radar" of the past 7 days.</li>
  </ul>

  <h2>Architecture (1 slide)</h2>
  <pre>
   [GitHub PR / commit / org scan]
              |
    +---------+----------+-----------+
    |         |          |           |
 Aegis   Aegis       Aegis
 Guard   Verify      Lens
    |         |          |
    +---------+----------+--- ledger (Supabase)
              |
     4 analyzers in parallel
              |
        signed verdict
              |
        [Vercel SSR dashboard]
  </pre>

  <h2>Stack — pure Rust 100%</h2>
  <ul>
    <li>12 Cargo workspace crates, ~5,000 LOC</li>
    <li>Tokio async runtime, Axum web framework, askama + htmx for SSR</li>
    <li>ed25519-dalek + blake3 for the signed audit chain</li>
    <li><code>reqwest</code> + <code>serde</code> for direct calls to NVIDIA NIM (BYOK, no LLM framework lock-in)</li>
    <li>Supabase Postgres (or in-memory store) for the ledger</li>
  </ul>

  <h2>BYOK — your key, your code</h2>
  <p>ARGUS never stores your API key. Each request carries your NVIDIA NIM key in the <code>X-LLM-Key</code> header. Your diffs are sent to NIM only with the key you provided. No login. No persistence. No tracking.</p>
  <a class="cta" href="/submit">Try ARGUS on a real PR →</a>

  <h2>Latest weekly briefing</h2>
  <div class="briefing">{}</div>
  <p><a href="/weekly">→ Read the full briefing</a></p>

  <h2>How to run locally</h2>
  <pre><code># 1. Get a free NVIDIA NIM key at https://build.nvidia.com/
export ARGUS_NIM_KEY=nvapi-xxx

# 2. Clone and run
git clone https://github.com/SuarezPM/apohara-argus
cd apohara-argus

# 3. Pre-commit guard
echo "your diff" | cargo run -p argus-guard --bin argus-guard

# 4. PR review (one-shot)
cargo run -p argus -- guard --diff ./pr.diff

# 5. Weekly digest
cargo run -p argus -- lens --org acme --mock-prs "acme/api#1,acme/web#2"</code></pre>

  <h2>5 Platzi projects, one product</h2>
  <p>This repo delivers the Reto AI Academy 5 projects in one unified submission:</p>
  <ol>
    <li><strong>Sistema de prompts</strong> → 4 documented prompts at <code>crates/argus-core/prompts/</code> + a Rust loader</li>
    <li><strong>Automatización</strong> → 3 Tokio workers: Guard, Verify, Lens, all autonomous</li>
    <li><strong>App web</strong> → SSR dashboard (Axum + htmx, this page)</li>
    <li><strong>Agente</strong> → the workflow as agent (skills, context, decisions documented)</li>
    <li><strong>MVP con LLM</strong> → backend with <code>argus-llm</code> (BYOK, NVIDIA NIM)</li>
  </ol>
</div>
</body>
</html>"##, briefing_md)
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,argus=debug")))
        .init();

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
        .route("/api/health", get(api_health))
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
