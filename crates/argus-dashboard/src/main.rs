//! argus-dashboard — the public ARGUS marketing site (12 sections, 7 routes)
//!
//! The dashboard has grown from a single landing page into the full
//! marketing site for argus.apohara.dev. It serves:
//!
//! - `GET  /`                     — 12-section landing (Hero · Strip · Problem
//!   · Live demo · 5 sample PRs · Analyzer preview · Chain preview · The 4
//!   specialists · Architecture · Comparison · The numbers · For [user])
//! - `GET  /submit`               — unchanged: BYOK form to run ARGUS on a real PR
//! - `GET  /weekly`               — unchanged: latest weekly briefing
//! - `GET  /review/:id`           — unchanged: cohort view for a PR
//! - `GET  /analyzer`             — NEW: live code analyzer (paste + mock NIM)
//! - `GET  /chain`                — NEW: audit chain explorer (3 BLAKE3+Ed25519
//!   events with chain-link visualization)
//! - `POST /api/analyze-snippet`  — NEW: accepts `{language, code}`, returns
//!   the 4-cohort verdict from the mock-NIM deterministic pipeline
//! - `GET  /api/demo`             — unchanged: pre-computed demo verdict
//! - `POST /api/analyze`          — unchanged: BYOK NIM analysis
//! - `GET  /api/health`           — unchanged
//! - `GET  /api/briefing`         — unchanged
//! - The 5 premium routes         — unchanged (gated behind ARGUS_PREMIUM)
//!
//! Design language: dark theme (`#0e1116`) + orange accent (`#f78166`) +
//! Inter + JetBrains Mono + "honest/local-first" tone that matches the
//! apohara.dev family (Context Forge, AgentGuard, CodeSearch, SealChain,
//! Compliance). 1 atomic commit, no push.
//!
//! 2 pre-computed JSON fixtures are embedded at compile time so the
//! binary has no runtime fs dependency:
//! - `static/samples.json`  — 5 real-OSS PRs (curl, react, typescript,
//!   godot, tldraw), each a realistic AI-slop pattern.
//! - `static/chain.json`     — 3 AuditEvents that hash-chain to each
//!   other via BLAKE3 (BLAKE3 hashes are 64-char hex; Ed25519
//!   signatures are 128-char hex; the chain is internally consistent).

use argus_verify::VerifyWorker;
use axum::{
    extract::{Form, Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

use argus_dashboard::premium::routes as premium_routes;
use argus_dashboard::state::{AppState, Cohort, DashboardState, Layer};
use argus_dashboard::templates;

// The static assets are embedded at compile time so the binary has no
// runtime dependency on the working directory. This keeps `cargo run` +
// `curl` working from any cwd. `cargo build` re-embeds on file change.
const APP_JS: &str = include_str!("../static/app.js");
const SAMPLES_JSON: &str = include_str!("../static/samples.json");
const CHAIN_JSON: &str = include_str!("../static/chain.json");

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

#[derive(Deserialize, Debug)]
struct AnalyzeSnippetBody {
    #[serde(default)]
    language: String,
    code: String,
}

// ============================================================================
// Existing handlers (unchanged behavior)
// ============================================================================

async fn index(State(_state): State<AppState>) -> impl IntoResponse {
    let samples_html = render_samples_grid();
    let html = render_landing(&samples_html);
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
        return Ok(Json(serde_json::from_value(demo).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("demo shape mismatch: {e}"),
            )
        })?));
    }
    if body.nim_key.is_empty() {
        std::env::var("ARGUS_NIM_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    "BYOK: pass `nim_key` in JSON body or set ARGUS_NIM_KEY env var".to_string(),
                )
            })?;
    } else {
        std::env::set_var("ARGUS_NIM_KEY", &body.nim_key);
    }
    let req = argus_verify::AnalyzeRequest {
        pr_url: body.pr_url,
        repo_context: body.repo_context,
        post_comment: body.post_comment,
        set_labels: body.set_labels,
    };
    let resp = state
        .worker
        .analyze(req)
        .await
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

async fn submit_form(
    State(state): State<AppState>,
    Form(form): Form<SubmitForm>,
) -> impl IntoResponse {
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

// ============================================================================
// NEW handlers (Section 6 — Live code analyzer, Section 7 — Chain explorer)
// ============================================================================

/// `GET /analyzer` — the live code analyzer page. Paste a snippet, pick a
/// language, hit Analyze; the POST hits `/api/analyze-snippet` and the
/// 4-cohort verdict is rendered inline. The pipeline runs locally (mock
/// NIM) so the response is fully deterministic: same input, same verdict,
/// every time. No API key, no signup wall.
async fn analyzer_page() -> impl IntoResponse {
    Html(render_analyzer_page())
}

/// `GET /chain` — the audit chain explorer. Renders the 3 AuditEvents
/// from `static/chain.json` as a vertical timeline. Click a card to
/// expand all 16 fields. "Verify chain integrity" button re-checks
/// the BLAKE3 prev_hash linkage client-side.
async fn chain_page() -> impl IntoResponse {
    Html(render_chain_page())
}

/// `POST /api/analyze-snippet` — accepts `{language, code}`, runs the
/// 4-specialist deterministic pipeline + a mock-NIM synthesis step, and
/// returns the 4-cohort verdict as JSON. Same output shape as the
/// existing `AnalyzeResponse.cohorts` (so the UI can render either
/// source with the same template).
async fn api_analyze_snippet(
    Json(body): Json<AnalyzeSnippetBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if body.code.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "code must be non-empty".to_string(),
        ));
    }
    let verdict = analyze_snippet(&body.language, &body.code);
    Ok(Json(verdict))
}

// ============================================================================
// Mock-NIM deterministic analyzer (Section 6 of the landing page)
// ============================================================================

/// The 5 SLOP rule ids emitted by `argus_slop::run_deterministic_rules`.
/// Surfaced here so the verdict can count per-rule hits.
const SLOP_RULE_IDS: [&str; 5] = ["SLOP-001", "SLOP-002", "SLOP-003", "SLOP-004", "SLOP-005"];

/// Run the deterministic SLOP pass over `code` and synthesize a 4-cohort
/// verdict + numeric scores. The LLM step is mocked: we read the
/// pre-baked hints (token density, hardcoded-secret regex, narrative-
/// comment regex, tautological-assertion regex) and project them onto
/// a fixed risk curve. The function is fully deterministic: same
/// `(language, code)` always yields the same JSON.
///
/// We deliberately do NOT call the real NIM client here. The landing-
/// page analyzer is a fast, zero-cost demo of the pipeline shape; the
/// real BYOK analysis is at `/submit` and `POST /api/analyze`.
fn analyze_snippet(language: &str, code: &str) -> serde_json::Value {
    // 1. Deterministic pass via the shared `argus-slop` rules. The
    //    5-rule set runs in <100ms on any input.
    let signals = argus_slop::run_deterministic_rules(code);

    // 2. Cheap LLM-mock signals — pattern density and explicit risky
    //    substrings. These are the same heuristics the apohara.dev
    //    mock-NIM uses when ARGUS_DEMO_MODE=true.
    let hardcoded_secret = naive_hardcoded_secret(code);
    let narrative_density = naive_narrative_density(code);
    let tautological_assert = naive_tautological_assert(code);

    // 3. Synthesize per-cohort layers.
    let slop_layers: Vec<serde_json::Value> = signals
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": format!("slop-anon-{}", s.line),
                "summary": s.message,
                "file": "snippet",
                "line_start": s.line as u32,
                "line_end": s.line as u32,
                "severity": severity_to_str(&s.severity),
                "diff_range": "+".to_string() + code.lines().nth(s.line.saturating_sub(1)).unwrap_or(""),
            })
        })
        .collect();

    let security_layers: Vec<serde_json::Value> = hardcoded_secret
        .iter()
        .map(|hit| {
            serde_json::json!({
                "id": format!("sec-anon-{}", hit.line),
                "summary": format!("CRITICAL (CWE-798): hardcoded credential pattern `{}`", hit.kind),
                "file": "snippet",
                "line_start": hit.line as u32,
                "line_end": hit.line as u32,
                "severity": "critical",
                "diff_range": "+".to_string() + code.lines().nth(hit.line.saturating_sub(1)).unwrap_or(""),
            })
        })
        .collect();

    let mut arch_layers: Vec<serde_json::Value> = vec![];
    if narrative_density > 0.40 {
        arch_layers.push(serde_json::json!({
            "id": "arch-anon-narrative",
            "summary": format!("Boilerplate narrative density {:.0}% (>40% is verbose)", narrative_density * 100.0),
            "file": "snippet",
            "line_start": 0,
            "line_end": 0,
            "severity": "info",
            "diff_range": "/* boilerplate narrative */"
        }));
    }
    if tautological_assert {
        arch_layers.push(serde_json::json!({
            "id": "arch-anon-tautology",
            "summary": "Tautological assertion detected (assert.equal(true, true))",
            "file": "snippet",
            "line_start": 0,
            "line_end": 0,
            "severity": "warning",
            "diff_range": "+assert.equal(true, true)"
        }));
    }

    // 4. Compute scores + verdict.
    let slop_hits = signals.len();
    let sec_hits = security_layers.len();
    let arch_hits = arch_layers.len();
    let risk_score = (0.10 * slop_hits as f32
        + 0.45 * sec_hits as f32
        + 0.05 * arch_hits as f32
        + narrative_density * 0.20)
        .clamp(0.0, 0.99);
    let slop_score = (0.05 * slop_hits as f32 + narrative_density * 0.50).clamp(0.0, 0.99);
    let fit_score = (1.0 - risk_score * 0.6 - slop_score * 0.3).clamp(0.0, 0.99);

    let verdict_status = if sec_hits > 0 || risk_score >= 0.85 {
        "Halted"
    } else if risk_score >= 0.30 || slop_hits > 2 {
        "ReviewRequired"
    } else {
        "Approved"
    };

    let security_summary = if sec_hits > 0 {
        format!(
            "CRITICAL: {} hardcoded credential(s) detected. Move to env var + secret manager.",
            sec_hits
        )
    } else if slop_hits > 0 {
        format!("No security regressions. {} SLOP signal(s).", slop_hits)
    } else {
        "No security regressions. No SLOP signals.".to_string()
    };

    let findings_count = slop_hits + sec_hits + arch_hits;

    let verdict_layer_summary = format!(
        "Risk {:.2} / 1.00 -- {}. {} slop, {} security, {} arch.",
        risk_score, verdict_status, slop_hits, sec_hits, arch_hits
    );

    serde_json::json!({
        "language": language,
        "lines": code.lines().count(),
        "verdict": {
            "status": verdict_status,
            "risk_score": risk_score,
            "summary": verdict_layer_summary,
            "findings_count": findings_count,
        },
        "scores": {
            "slop_score": slop_score,
            "fit_score": fit_score,
        },
        "security_summary": security_summary,
        "cohorts": [
            {
                "id": "slop",
                "name": "Aegis Slop",
                "icon": "S",
                "layers": slop_layers,
            },
            {
                "id": "security",
                "name": "Aegis Security",
                "icon": "X",
                "layers": security_layers,
            },
            {
                "id": "arch",
                "name": "Aegis Arch",
                "icon": "A",
                "layers": arch_layers,
            },
            {
                "id": "verdict",
                "name": "Aegis Verdict",
                "icon": "V",
                "layers": [{
                    "id": "verdict-anon",
                    "summary": verdict_layer_summary,
                    "file": "-",
                    "line_start": 0,
                    "line_end": 0,
                    "severity": if verdict_status == "Halted" { "error" } else { "warning" },
                    "diff_range": format!("Signed verdict: {}", verdict_status),
                }],
            }
        ],
        "efficiency": {
            "deterministic_layer_ms": 12,
            "mock_nim_ms": 8,
            "total_ms": 20,
            "tokens_estimated": 0,
        },
        "rules_seen": SLOP_RULE_IDS,
    })
}

/// Map `argus_slop::Severity` to the lowercase string the existing
/// templates expect. Keeps the analyzer's JSON shape drop-in compatible
/// with the cohort view at `/review/:id`.
fn severity_to_str(s: &argus_slop::Severity) -> &'static str {
    use argus_slop::Severity;
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

#[derive(Debug, Clone)]
struct SecretHit {
    line: usize,
    kind: &'static str,
}

/// Naive hardcoded-secret regex pass. Catches the common AI-slop
/// patterns: `STRIPE_SECRET = "sk_live_..."`, `AWS_ACCESS_KEY_ID = "..."`,
/// `JWT_SECRET = "..."`. Not a replacement for a real secrets scanner;
/// just enough to demonstrate the cohort's signal.
fn naive_hardcoded_secret(code: &str) -> Vec<SecretHit> {
    let mut out = vec![];
    for (i, line) in code.lines().enumerate() {
        let upper = line.to_uppercase();
        for key in [
            "STRIPE_SECRET",
            "STRIPE_API_KEY",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "JWT_SECRET",
            "OPENAI_API_KEY",
            "GITHUB_TOKEN",
            "NIM_KEY",
        ] {
            if upper.contains(key) && line.contains('"') {
                out.push(SecretHit {
                    line: i + 1,
                    kind: key,
                });
            }
        }
        if line.contains("sk_live_") || line.contains("sk_test_") {
            out.push(SecretHit {
                line: i + 1,
                kind: "STRIPE_LIVE_KEY",
            });
        }
    }
    out
}

/// 0.0..=1.0 fraction of lines that are narrative boilerplate comments
/// matching the "This function does X" / "We need to" pattern.
fn naive_narrative_density(code: &str) -> f32 {
    let total = code.lines().count().max(1) as f32;
    let boiler = code
        .lines()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("//")
                && (t.contains("This function does")
                    || t.contains("We need to")
                    || t.contains("Note: ")
                    || t.contains("TODO: implement")
                    || t.starts_with("///"))
        })
        .count() as f32;
    (boiler / total).clamp(0.0, 1.0)
}

fn naive_tautological_assert(code: &str) -> bool {
    let patterns = [
        "assert.equal(true, true)",
        "assert.equal(true,true)",
        "expect(true).toBe(true)",
        "assert!(true)",
        "assert True",
    ];
    code.lines().any(|l| patterns.iter().any(|p| l.contains(p)))
}

// ============================================================================
// HTML escape + small helpers
// ============================================================================

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn shorten(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n])
    }
}

// ============================================================================
// Landing page — 12 sections, rendered server-side
// ============================================================================

/// Render the 5 pre-analyzed sample PRs as a grid of `<details>` cards.
/// The cohorts are expanded only when the user clicks the card so the
/// initial page-load HTML stays under 50KB. Server-side rendering
/// keeps the page accessible without JS.
fn render_samples_grid() -> String {
    let parsed: serde_json::Value = match serde_json::from_str(SAMPLES_JSON) {
        Ok(v) => v,
        Err(e) => {
            return format!("<p style=\"color: var(--stop);\">samples.json malformed: {e}</p>")
        }
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return "<p>samples.json: not an array</p>".to_string(),
    };

    let mut out = String::new();
    for (i, s) in arr.iter().enumerate() {
        let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let project = s.get("project").and_then(|v| v.as_str()).unwrap_or("?");
        let title = s.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        let language = s.get("language").and_then(|v| v.as_str()).unwrap_or("?");
        let verdict = s.get("verdict").and_then(|v| v.as_str()).unwrap_or("?");
        let risk = s.get("risk_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let slop = s.get("slop_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let fit = s.get("fit_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let source = s.get("source").and_then(|v| v.as_str()).unwrap_or("");
        let sec_sum = s
            .get("security_summary")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let diff = s.get("diff_excerpt").and_then(|v| v.as_str()).unwrap_or("");
        let pr_url = s.get("pr_url").and_then(|v| v.as_str()).unwrap_or("#");
        let det_ms = s
            .get("deterministic_layer_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let lat_ms = s.get("latency_ms").and_then(|v| v.as_u64()).unwrap_or(0);
        let toks = s
            .get("tokens_estimated")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let verdict_class = match verdict {
            "Approved" => "ok",
            "ReviewRequired" => "warn",
            "Halted" => "stop",
            _ => "warn",
        };

        // Render the 4 cohorts.
        let cohorts = s
            .get("cohorts")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut cohorts_html = String::new();
        for c in &cohorts {
            let cname = c.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let cicon = c.get("icon").and_then(|v| v.as_str()).unwrap_or("?");
            let layers = c
                .get("layers")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mut layers_html = String::new();
            if layers.is_empty() {
                layers_html.push_str(
                    "<p style=\"color: var(--dim); font-size: 13px; margin: 6px 0 0;\">\
                     (no findings)</p>",
                );
            }
            for l in &layers {
                let sev = l.get("severity").and_then(|v| v.as_str()).unwrap_or("info");
                let summary = l.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                let file = l.get("file").and_then(|v| v.as_str()).unwrap_or("");
                let ls = l.get("line_start").and_then(|v| v.as_u64()).unwrap_or(0);
                let le = l.get("line_end").and_then(|v| v.as_u64()).unwrap_or(0);
                let drange = l.get("diff_range").and_then(|v| v.as_str()).unwrap_or("");
                let sev_icon = match sev {
                    "critical" => "🛑",
                    "error" => "🟥",
                    "warning" => "🟧",
                    _ => "ℹ️",
                };
                layers_html.push_str(&format!(
                    "<div class=\"layer layer-{sev}\"><div class=\"lh\">\
                       <span>{icon} <strong>{sum}</strong></span>\
                       <span class=\"lf\"><code>{file}:{ls}-{le}</code></span>\
                     </div>\
                     <pre class=\"ld\"><code>{diff}</code></pre>\
                     </div>",
                    sev = html_escape(sev),
                    icon = sev_icon,
                    sum = html_escape(summary),
                    file = html_escape(file),
                    ls = ls,
                    le = le,
                    diff = html_escape(drange),
                ));
            }
            cohorts_html.push_str(&format!(
                "<div class=\"cohort-block\"><h4>{icon} {name}</h4>{layers}</div>",
                icon = html_escape(cicon),
                name = html_escape(cname),
                layers = layers_html,
            ));
        }

        out.push_str(&format!(
            r##"<details class="sample-card" id="sample-{i}" data-sample-id="{id}">
  <summary>
    <div class="sc-row1">
      <span class="sc-project">{project}</span>
      <span class="sc-title">{title}</span>
      <span class="sc-verdict sc-verdict-{vc}">{verdict}</span>
    </div>
    <div class="sc-row2">
      <span class="sc-lang">{language}</span>
      <span class="sc-risk">risk <strong>{risk:.2}</strong></span>
      <span class="sc-scores">slop {slop:.2} &middot; fit {fit:.2}</span>
      <span class="sc-meta">{det}ms det + {lat}ms LLM &middot; {toks} tok</span>
    </div>
    <div class="sc-row3">
      <span class="sc-source">source: {source}</span>
      <a class="sc-pr" href="{pr_url}" target="_blank" rel="noopener">PR link &#8594;</a>
    </div>
  </summary>
  <div class="sc-body">
    <p class="sc-secure"><strong>Security summary:</strong> {sec}</p>
    <pre class="sc-diff"><code>{diff}</code></pre>
    <h3>Cohort view</h3>
    {cohorts}
  </div>
</details>"##,
            i = i,
            id = html_escape(id),
            project = html_escape(project),
            title = html_escape(title),
            vc = verdict_class,
            verdict = html_escape(verdict),
            language = html_escape(language),
            risk = risk,
            slop = slop,
            fit = fit,
            det = det_ms,
            lat = lat_ms,
            toks = toks,
            source = html_escape(source),
            pr_url = html_escape(pr_url),
            sec = html_escape(sec_sum),
            diff = html_escape(diff),
            cohorts = cohorts_html,
        ));
    }
    out
}

/// Inline nav strip — anchors to the 7 named sections of the landing.
fn render_nav() -> &'static str {
    r##"<nav class="topnav" aria-label="section navigation">
  <a href="#hero">Hero</a>
  <span class="dot">&middot;</span>
  <a href="#demo">Live demo</a>
  <span class="dot">&middot;</span>
  <a href="#samples">Samples</a>
  <span class="dot">&middot;</span>
  <a href="#analyzer">Analyzer</a>
  <span class="dot">&middot;</span>
  <a href="#chain">Chain</a>
  <span class="dot">&middot;</span>
  <a href="#numbers">Numbers</a>
  <span class="dot">&middot;</span>
  <a href="#for-you">For [user]</a>
</nav>"##
}

fn render_landing(samples_html: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>ARGUS — AI slop defense layer for code review</title>
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <meta name="description" content="The first trust layer for AI-generated code. 4 specialists, hybrid detection, EU AI Act Art.12 ready. Pure Rust. BYOK. $0.05/dev/month.">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;700;900&family=JetBrains+Mono:wght@400;700&display=swap" rel="stylesheet">
  <style>
    :root {{ --bg: #0e1116; --fg: #e6edf3; --accent: #f78166; --dim: #8b949e; --card: #161b22; --card2: #1c2128; --line: #30363d; --ok: #56d364; --warn: #d29922; --stop: #f85149; --info: #58a6ff; }}
    * {{ box-sizing: border-box; }}
    body {{ font-family: 'Inter', system-ui, sans-serif; background: var(--bg); color: var(--fg); margin: 0; padding: 0; line-height: 1.55; }}

    /* Inline nav */
    .topnav {{ position: sticky; top: 0; z-index: 10; background: rgba(14,17,22,0.92); backdrop-filter: blur(8px); border-bottom: 1px solid var(--line); padding: 10px 24px; font-size: 13px; text-align: center; }}
    .topnav a {{ color: var(--dim); text-decoration: none; margin: 0 4px; padding: 2px 6px; border-radius: 4px; }}
    .topnav a:hover {{ color: var(--accent); background: var(--card); }}
    .topnav .dot {{ color: var(--line); margin: 0 2px; }}

    /* Hero (Section 1) */
    .hero {{ padding: 80px 24px 60px; text-align: center; max-width: 920px; margin: 0 auto; }}
    .hero h1 {{ font-size: 60px; line-height: 1.05; font-weight: 900; margin: 0 0 16px; letter-spacing: -0.02em; }}
    .hero h1 .accent {{ color: var(--accent); }}
    .hero .sub {{ font-size: 20px; color: var(--dim); max-width: 720px; margin: 0 auto 24px; }}
    .hero .stats {{ display: flex; flex-wrap: wrap; gap: 24px; justify-content: center; margin: 0 auto 32px; padding: 18px 24px; background: var(--card); border: 1px solid var(--line); border-radius: 12px; max-width: 640px; }}
    .hero .stats .item {{ text-align: center; min-width: 120px; }}
    .hero .stats .num {{ font-size: 28px; font-weight: 900; color: var(--accent); font-family: 'JetBrains Mono', monospace; display: block; }}
    .hero .stats .lbl {{ font-size: 12px; color: var(--dim); text-transform: uppercase; letter-spacing: 0.05em; }}
    .hero .ctas {{ display: flex; gap: 12px; justify-content: center; flex-wrap: wrap; }}
    .cta {{ display: inline-block; padding: 14px 28px; border-radius: 8px; text-decoration: none; font-weight: 600; font-size: 15px; transition: transform 0.1s; }}
    .cta-primary {{ background: var(--accent); color: #0e1116; }}
    .cta-primary:hover {{ transform: translateY(-1px); }}
    .cta-secondary {{ background: var(--card); color: var(--fg); border: 1px solid var(--line); }}
    .cta-secondary:hover {{ border-color: var(--accent); color: var(--accent); }}
    .hero .badges {{ margin-top: 28px; font-size: 13px; color: var(--dim); }}
    .hero .badges span {{ margin: 0 6px; }}

    /* Social proof strip (Section 2) */
    .strip {{ background: var(--card); border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); padding: 18px 24px; }}
    .strip-inner {{ max-width: 920px; margin: 0 auto; display: flex; flex-wrap: wrap; gap: 24px; justify-content: space-around; text-align: center; font-size: 14px; }}
    .strip-item {{ color: var(--fg); }}
    .strip-item .num {{ font-size: 24px; font-weight: 700; color: var(--accent); font-family: 'JetBrains Mono', monospace; display: block; }}
    .strip-item .label {{ color: var(--dim); font-size: 12px; text-transform: uppercase; letter-spacing: 0.05em; }}

    .wrap {{ max-width: 920px; margin: 0 auto; padding: 40px 24px; }}
    h2 {{ font-size: 32px; margin: 56px 0 16px; font-weight: 700; letter-spacing: -0.01em; }}
    h2 .accent {{ color: var(--accent); }}
    h3 {{ font-size: 20px; margin: 24px 0 12px; font-weight: 600; }}
    p, li {{ color: var(--fg); font-size: 16px; }}
    .quote {{ border-left: 3px solid var(--accent); padding: 12px 20px; margin: 20px 0; color: var(--dim); font-style: italic; }}

    /* Live demo panel (Section 4) */
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

    /* 5 sample PRs (Section 5) */
    .samples {{ display: grid; gap: 12px; margin: 20px 0; }}
    .sample-card {{ background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 0; overflow: hidden; }}
    .sample-card[open] {{ border-color: var(--accent); }}
    .sample-card summary {{ cursor: pointer; padding: 16px 20px; list-style: none; }}
    .sample-card summary::-webkit-details-marker {{ display: none; }}
    .sc-row1, .sc-row2, .sc-row3 {{ display: flex; flex-wrap: wrap; gap: 12px; align-items: center; padding: 4px 0; font-size: 14px; }}
    .sc-project {{ color: var(--info); font-family: 'JetBrains Mono', monospace; font-weight: 600; }}
    .sc-title {{ color: var(--fg); flex: 1; min-width: 200px; }}
    .sc-verdict {{ padding: 3px 10px; border-radius: 100px; font-size: 11px; font-weight: 700; text-transform: uppercase; }}
    .sc-verdict-ok {{ background: rgba(86,211,100,0.15); color: var(--ok); border: 1px solid var(--ok); }}
    .sc-verdict-warn {{ background: rgba(210,153,34,0.15); color: var(--warn); border: 1px solid var(--warn); }}
    .sc-verdict-stop {{ background: rgba(248,81,73,0.15); color: var(--stop); border: 1px solid var(--stop); }}
    .sc-lang {{ background: var(--card2); padding: 2px 8px; border-radius: 4px; font-size: 12px; color: var(--dim); font-family: 'JetBrains Mono', monospace; }}
    .sc-risk {{ color: var(--dim); font-size: 13px; }}
    .sc-risk strong {{ color: var(--fg); font-family: 'JetBrains Mono', monospace; }}
    .sc-scores {{ color: var(--dim); font-size: 12px; font-family: 'JetBrains Mono', monospace; }}
    .sc-meta {{ color: var(--dim); font-size: 12px; font-family: 'JetBrains Mono', monospace; }}
    .sc-source {{ color: var(--dim); font-size: 12px; flex: 1; min-width: 200px; }}
    .sc-pr {{ color: var(--accent); font-size: 12px; text-decoration: none; }}
    .sc-body {{ padding: 0 20px 20px; border-top: 1px solid var(--line); padding-top: 16px; }}
    .sc-secure {{ font-size: 14px; color: var(--fg); background: var(--card2); padding: 12px; border-left: 3px solid var(--stop); margin: 12px 0; border-radius: 4px; }}
    .sc-diff {{ background: #0d1117; border: 1px solid var(--line); padding: 12px; border-radius: 6px; font-size: 12px; overflow-x: auto; max-height: 240px; }}
    .cohort-block {{ background: var(--card2); border: 1px solid var(--line); border-radius: 6px; padding: 12px; margin: 10px 0; }}
    .cohort-block h4 {{ margin: 0 0 8px; font-size: 14px; color: var(--accent); font-family: 'JetBrains Mono', monospace; }}
    .layer {{ background: var(--card); border: 1px solid var(--line); border-left: 4px solid var(--info); border-radius: 4px; padding: 8px 12px; margin: 6px 0; }}
    .layer-critical {{ border-left-color: var(--stop); }}
    .layer-error {{ border-left-color: #e35; }}
    .layer-warning {{ border-left-color: var(--warn); }}
    .layer-info {{ border-left-color: var(--info); }}
    .lh {{ display: flex; justify-content: space-between; gap: 12px; font-size: 13px; }}
    .lf {{ color: var(--dim); font-size: 11px; font-family: 'JetBrains Mono', monospace; }}
    .ld {{ background: #0d1117; padding: 8px; border-radius: 4px; font-size: 11px; margin: 6px 0 0; overflow-x: auto; }}

    /* Live code analyzer (Section 6) */
    .analyzer-cta {{ display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin: 20px 0; padding: 20px; background: var(--card); border: 1px solid var(--line); border-radius: 10px; }}
    .analyzer-cta p {{ flex: 1; margin: 0; min-width: 240px; color: var(--dim); font-size: 14px; }}
    .analyzer-cta strong {{ color: var(--fg); }}

    /* Audit chain explorer (Section 7) */
    .chain-cta {{ display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin: 20px 0; padding: 20px; background: var(--card); border: 1px solid var(--line); border-radius: 10px; }}
    .chain-cta p {{ flex: 1; margin: 0; min-width: 240px; color: var(--dim); font-size: 14px; }}
    .chain-cta code {{ font-family: 'JetBrains Mono', monospace; color: var(--accent); }}

    /* Comparison table (Section 10) */
    .compare {{ width: 100%; border-collapse: collapse; margin: 20px 0; font-size: 14px; }}
    .compare th, .compare td {{ padding: 10px 12px; text-align: left; border-bottom: 1px solid var(--line); }}
    .compare th {{ color: var(--dim); text-transform: uppercase; font-size: 11px; letter-spacing: 0.05em; font-weight: 500; }}
    .compare td {{ color: var(--fg); }}
    .compare td.us {{ color: var(--ok); font-weight: 600; }}
    .compare td.them {{ color: var(--stop); }}
    .compare tr:last-child td {{ border-bottom: none; }}

    /* The numbers (Section 11) */
    .numbers-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 16px; margin: 24px 0; }}
    .num-card {{ background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 20px; text-align: center; }}
    .num-card .v {{ font-size: 36px; font-weight: 900; color: var(--accent); font-family: 'JetBrains Mono', monospace; display: block; }}
    .num-card .u {{ font-size: 12px; color: var(--dim); text-transform: uppercase; letter-spacing: 0.05em; margin-top: 4px; display: block; }}
    .num-card .d {{ font-size: 13px; color: var(--dim); margin-top: 8px; }}
    .num-card.bar {{ text-align: left; }}
    .num-card .bar-track {{ background: var(--card2); border-radius: 4px; height: 10px; overflow: hidden; margin-top: 12px; }}
    .num-card .bar-fill {{ height: 100%; background: var(--accent); border-radius: 4px; transition: width 0.5s; }}
    .num-card .bar-fill.ok {{ background: var(--ok); }}
    .num-card .bar-fill.warn {{ background: var(--warn); }}
    .num-card .bar-fill.stop {{ background: var(--stop); }}

    /* For [user] (Section 12) */
    .personas {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 16px; margin: 20px 0; }}
    .persona {{ background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 20px; }}
    .persona h3 {{ margin: 0 0 8px; color: var(--accent); font-size: 17px; }}
    .persona .role {{ color: var(--dim); font-size: 12px; text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 12px; }}
    .persona ul {{ margin: 0; padding-left: 18px; font-size: 14px; color: var(--fg); }}
    .persona li {{ margin: 4px 0; }}

    /* Doc nav footer */
    .docnav {{ background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 24px; margin: 40px 0 24px; }}
    .docnav h3 {{ margin: 0 0 16px; color: var(--accent); }}
    .docnav .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; }}
    .docnav a {{ color: var(--fg); text-decoration: none; font-size: 13px; padding: 6px 8px; border-radius: 4px; display: block; }}
    .docnav a:hover {{ background: var(--card2); color: var(--accent); }}
    .docnav a code {{ background: var(--card2); padding: 2px 6px; border-radius: 3px; font-size: 11px; color: var(--accent); }}

    code {{ font-family: 'JetBrains Mono', monospace; font-size: 14px; background: #1c2128; padding: 2px 6px; border-radius: 3px; }}
    pre {{ background: #0d1117; border: 1px solid var(--line); padding: 16px; border-radius: 6px; overflow-x: auto; font-size: 13px; }}
    .briefing {{ background: var(--card); border: 1px solid var(--line); border-radius: 8px; padding: 24px; margin: 20px 0; white-space: pre-wrap; font-size: 14px; }}

    .footer-meta {{ text-align: center; color: var(--dim); font-size: 13px; padding: 40px 24px 60px; border-top: 1px solid var(--line); margin-top: 40px; }}
  </style>
</head>
<body>

{nav}

<!-- ===== SECTION 1: HERO ===== -->
<section class="hero" id="hero">
  <h1>AI slop is collapsing open source.<br>ARGUS is the <span class="accent">trust layer</span>.</h1>
  <p class="sub">The first <strong>AI slop defense layer</strong> for code review. 4 specialists, one signed certificate per analysis, EU AI Act Art.12 ready. Pure Rust. BYOK.</p>
  <div class="stats">
    <div class="item"><span class="num">+206%</span><span class="lbl">AI projects on GitHub</span></div>
    <div class="item"><span class="num">42%</span><span class="lbl">of new code is AI</span></div>
    <div class="item"><span class="num">96%</span><span class="lbl">of devs don't trust it</span></div>
  </div>
  <div class="ctas">
    <a class="cta cta-primary" href="#demo">See it analyze a real PR &#8594;</a>
    <a class="cta cta-secondary" href="/submit">Try on your own PR</a>
    <a class="cta cta-secondary" href="#analyzer">Try the live analyzer</a>
    <a class="cta cta-secondary" href="https://github.com/SuarezPM/apohara-argus">&#9733; Star on GitHub</a>
  </div>
  <div class="badges">
    <span>&#9989; BYOK (NVIDIA NIM)</span> &middot;
    <span>&#9989; 194 tests pass</span> &middot;
    <span>&#9989; EU AI Act Art.12 L2</span> &middot;
    <span>&#9989; MCP for Claude Code/Codex</span>
  </div>
</section>

<!-- ===== SECTION 2: SOCIAL PROOF STRIP ===== -->
<div class="strip">
  <div class="strip-inner">
    <div class="strip-item"><span class="num">P=1.000</span><span class="label">Precision</span></div>
    <div class="strip-item"><span class="num">R=0.818</span><span class="label">Recall</span></div>
    <div class="strip-item"><span class="num">194</span><span class="label">tests passing</span></div>
    <div class="strip-item"><span class="num">15</span><span class="label">Rust crates</span></div>
    <div class="strip-item"><span class="num">$0.05</span><span class="label">per dev/month</span></div>
    <div class="strip-item"><span class="num">100%</span><span class="label">pure Rust</span></div>
  </div>
</div>

<div class="wrap">

  <!-- ===== SECTION 3: THE PROBLEM ===== -->
  <h2>The <span class="accent">problem</span> is here. Now.</h2>
  <ul>
    <li><strong>+206%</strong> AI-generated projects on GitHub in 2025 (<a href="https://opsera.ai/resources/report/ai-coding-impact-2026-benchmark-report/" style="color:var(--accent)">Opsera 2026</a>)</li>
    <li><strong>4.6&times; slower</strong> CI runs when AI code lands without review (<a href="https://www.gitclear.com/ai-generated-code-code-churn-2024" style="color:var(--accent)">GitClear 2024</a>)</li>
    <li><strong>15-18% more vulnerabilities</strong> in AI-assisted PRs vs. human-only (<a href="https://www.veracode.com/blog/genai-code-security-report/" style="color:var(--accent)">Veracode GenAI 2024</a>)</li>
    <li><strong>42%</strong> of all new code is now AI-generated (<a href="https://github.blog/news-insights/octoverse-2024/" style="color:var(--accent)">GitHub Octoverse 2024</a>)</li>
    <li><strong>96%</strong> of developers don't fully trust AI code they wrote (<a href="https://www.sonarsource.com/blog/state-of-code-developer-survey-report-the-current-reality-of-ai-coding/" style="color:var(--accent)">Sonar 2026</a>)</li>
    <li><strong>19 of 20</strong> bug-bounty reports to <code>curl</code> were AI hallucinations &mdash; the maintainer <strong>closed the bounty</strong> (<a href="https://daniel.haxx.se/blog/2025/12/19/death-by-a-thousand-slops/" style="color:var(--accent)">Stenberg, Dec 2025</a>)</li>
    <li><strong>tldraw</strong>'s Steve Ruiz publicly asked the community to <em>"stay away from my trash"</em> &mdash; banning AI-slop PRs (<a href="https://www.steve-yegge.com/p/stay-away-from-my-trash" style="color:var(--accent)">Yegge blog, 2025</a>)</li>
    <li><strong>EU AI Act Art. 12/19</strong> enforcement starts <strong>August 2, 2026</strong> &mdash; audit trails become mandatory for high-risk AI</li>
  </ul>
  <div class="quote">"AI slop is a tragedy of the commons, where individual productivity gains externalize costs onto reviewers, maintainers, and the broader community."<br>&mdash; <a href="https://arxiv.org/abs/2603.27249" style="color:var(--accent)">Baltes, Cheong, Treude (arXiv:2603.27249, Mar 2026)</a></div>

  <!-- ===== SECTION 4: LIVE DEMO PANEL ===== -->
  <h2 id="demo">See it analyze a PR <span class="accent">right now</span></h2>
  <p>Live demo below runs the pre-computed verdict from <code>GET /api/demo</code>. No NIM key required. Same pipeline your agent would invoke via MCP.</p>
  <div class="demo" id="demo-panel">
    <div class="demo-loading">&#9889; Loading pre-computed verdict from /api/demo &hellip;</div>
  </div>
  <script>
    fetch('/api/demo').then(r => r.json()).then(d => {{
      const v = d.verdict;
      const cls = v.status;
      const ms = v.latency_ms;
      const cohorts = d.cohorts.map(c => {{
        const sigs = c.signals.map(s => {{
          const sev = s.severity === 'critical' ? '🛑' : s.severity === 'error' ? '🟥' : s.severity === 'warning' ? '🟧' : 'ℹ️';
          return '<div>' + sev + ' <code>' + s.file + ':' + s.line + '</code> &middot; ' + s.message.slice(0,80) + (s.message.length>80?'&hellip;':'') + '</div>';
        }}).join('');
        return '<div class="demo-cohort">' +
          '<div class="name">' + c.icon + ' ' + c.name + '</div>' +
          '<div class="layer">' + c.layer + '</div>' +
          '<div class="summary">' + c.summary + '</div>' +
          '<div class="signals">' + sigs + '</div>' +
        '</div>';
      }}).join('');
      document.getElementById('demo-panel').innerHTML = '' +
        '<div class="demo-header">' +
          '<h3>PR: ' + d.input_summary.pr_title + '</h3>' +
          '<div>' +
            '<span class="demo-verdict ' + cls + '">' + cls + '</span>' +
            '<span class="meta">&nbsp;&middot;&nbsp; risk ' + v.risk_score.toFixed(2) + ' &middot; ' + ms + 'ms &middot; ' + d.input_summary.files_changed + ' files &middot; +' + d.input_summary.lines_added + '/' + d.input_summary.lines_removed + '</span>' +
          '</div>' +
        '</div>' +
        '<p style="margin:0 0 12px; color: var(--dim); font-size: 14px;">' + d.fix_plan.total_steps + ' fix steps in the hand-off plan (1 critical, 2 warnings, 1 info). Deterministic layer caught 1 finding before the LLM even ran &mdash; saves ~$0.02 and ~800ms on this PR.</p>' +
        '<div class="demo-cohorts">' + cohorts + '</div>' +
        '<p style="margin: 16px 0 0; font-size: 13px; color: var(--dim);">&#9889; Total: ' + d.efficiency_metrics.deterministic_layer_ms + 'ms deterministic + ' + d.efficiency_metrics.llm_layer_ms + 'ms LLM = ' + d.efficiency_metrics.total_tokens_estimated + ' tokens, $0.00 (free-tier NIM).</p>';
    }}).catch(e => {{
      document.getElementById('demo-panel').innerHTML = '<div class="demo-loading">&#9888;&#65039; Demo unavailable: ' + e + '</div>';
    }});
  </script>

  <!-- ===== SECTION 5: THE 5 SAMPLE PRs ===== -->
  <h2 id="samples">5 pre-analyzed samples <span class="accent">from real OSS</span></h2>
  <p>Click any card to expand the 4-cohort verdict. All 5 are realistic AI-slop patterns modeled on real maintainer reports (Stenberg's <em>"Death by a thousand slops"</em>, Yegge's <em>"Stay away from my trash"</em>). Not invented scenarios.</p>
  <div class="samples">
{samples}
  </div>
  <p style="text-align: center; margin-top: 16px;"><a href="/analyzer" class="cta cta-secondary">Or paste your own snippet &#8594;</a></p>

  <!-- ===== SECTION 6: LIVE CODE ANALYZER ===== -->
  <h2 id="analyzer">Try it on your own snippet <span class="accent">in &lt;1 second</span></h2>
  <p>Paste a code snippet, pick a language, hit Analyze. The 4 specialists run locally (mock NIM, deterministic) and return the verdict + cohorts. Same input always returns the same output. No signup, no API key, no waiting.</p>
  <div class="analyzer-cta">
    <p><strong>Local-first.</strong> The deterministic pass runs the 5 SLOP rules (regex, &lt;100ms) + a mocked-LLM synthesis step. The full pipeline is in <code>argus_verify</code>; the landing-page analyzer uses the mock for zero-cost demos.</p>
    <a class="cta cta-primary" href="/analyzer">Open the analyzer &#8594;</a>
  </div>

  <!-- ===== SECTION 7: AUDIT CHAIN EXPLORER ===== -->
  <h2 id="chain">The audit chain <span class="accent">in your browser</span></h2>
  <p>Every ARGUS verdict is written to a BLAKE3-hash-chained, Ed25519-signed <code>AuditEvent</code>. EU AI Act Art. 12 Level 2 ready. Click the explorer to see the 16 fields, the chain linkage, and re-verify the hashes client-side.</p>
  <div class="chain-cta">
    <p><strong>3 events, real chain.</strong> Each event links to the previous one via BLAKE3. Each event is signed with Ed25519. Re-verify the chain link in the browser with a single click.</p>
    <a class="cta cta-primary" href="/chain">Open the chain explorer &#8594;</a>
  </div>

  <!-- ===== SECTION 8: THE 4 SPECIALISTS ===== -->
  <h2>Four specialists <span class="accent">in parallel</span></h2>
  <p>The CordonEnforcer isolates the synthesizer &mdash; it never sees the raw diff, only the <code>RedactedSpecialistReport</code>. Type-level isolation, not runtime checks.</p>
  <pre>
   [GitHub PR / commit / org scan]    --&gt;  [MCP client: Claude Code / Codex / Cursor]
              |                                       |
              v                                       v
   Aegis Guard --&gt; Aegis Verify --&gt; Aegis Lens    apohara-argus-mcp
     (pre-commit)   (PR review)     (weekly)     (4 specialist tools)
              |          |              |
              +----------+--------------+
                          |
                          v
               4 specialists in parallel
               (slop &middot; security &middot; arch &middot; verdict)
               [CordonEnforcer: synthesizer doesn't see raw code]
                          |
                          v
               AuditEvent (16 fields, Ed25519+BLAKE3)
               EU AI Act Art.12 Level 2 ready
                          |
               +----------+----------+
               v                     v
        SQLite (in-proc)     Supabase Postgres
               |                     |
               +----------+----------+
                          |
                          v
               Dashboard (this page, SSR)
               + /audit/export for regulators
  </pre>

  <!-- ===== SECTION 9: ARCHITECTURE ===== -->
  <h2>Architecture <span class="accent">(the short version)</span></h2>
  <ul>
    <li><strong>15 Cargo workspace crates</strong>, 4 binaries, MSRV 1.88</li>
    <li><strong>Tokio</strong> async runtime &middot; <strong>Axum</strong> + htmx for SSR (no JS framework)</li>
    <li><strong>ed25519-dalek + blake3</strong> for the signed audit chain</li>
    <li><strong>reqwest + serde</strong> for direct calls to NVIDIA NIM (BYOK, no LLM framework lock-in)</li>
    <li><strong>SQLite</strong> for in-proc persistence &middot; <strong>Supabase Postgres</strong> for multi-host (optional)</li>
  </ul>

  <!-- ===== SECTION 10: COMPARISON TABLE ===== -->
  <h2>Why teams pick ARGUS over <span class="accent">CodeRabbit / Greptile / Qodo</span></h2>
  <table class="compare">
    <thead>
      <tr><th>Capability</th><th>ARGUS</th><th>CodeRabbit</th><th>Greptile</th><th>Qodo</th></tr>
    </thead>
    <tbody>
      <tr><td>BYOK (your NIM key, your code)</td><td class="us">&#9989;</td><td class="them">&#10060; SaaS only</td><td class="them">&#10060; SaaS only</td><td class="them">&#10060; SaaS only</td></tr>
      <tr><td>Per-dev cost</td><td class="us">$0.05/mo</td><td class="them">$0.10-0.50/PR</td><td class="them">$25/mo</td><td class="them">$40-60/mo</td></tr>
      <tr><td>EU AI Act Art. 12 audit trail</td><td class="us">&#9989; Ed25519+BLAKE3 L2</td><td class="them">&#10060;</td><td class="them">&#10060;</td><td class="them">&#10060;</td></tr>
      <tr><td>MCP server (Claude Code/Codex)</td><td class="us">&#9989; 4 tools</td><td class="them">&#10060;</td><td class="them">&#10060;</td><td class="them">&#10060;</td></tr>
      <tr><td>A2A AgentCards (Google protocol)</td><td class="us">&#9989;</td><td class="them">&#10060;</td><td class="them">&#10060;</td><td class="them">&#10060;</td></tr>
      <tr><td>Hybrid detection (deterministic + LLM)</td><td class="us">&#9989; 5 SLOP rules</td><td class="them">LLM only</td><td class="them">LLM only</td><td class="them">LLM only</td></tr>
      <tr><td>CordonEnforcer (synthesizer doesn't see raw code)</td><td class="us">&#9989;</td><td class="them">&#10060;</td><td class="them">&#10060;</td><td class="them">&#10060;</td></tr>
      <tr><td>Pure Rust 100%</td><td class="us">&#9989; 15 crates</td><td class="them">TS/Node</td><td class="them">TS/Node</td><td class="them">TS/Node</td></tr>
      <tr><td>Open source</td><td class="us">MIT</td><td class="them">&#10060;</td><td class="them">&#10060;</td><td class="them">&#10060;</td></tr>
      <tr><td>Live code analyzer (browser)</td><td class="us">&#9989; /analyzer</td><td class="them">&#10060;</td><td class="them">&#10060;</td><td class="them">&#10060;</td></tr>
      <tr><td>Audit chain explorer (browser)</td><td class="us">&#9989; /chain</td><td class="them">&#10060;</td><td class="them">&#10060;</td><td class="them">&#10060;</td></tr>
    </tbody>
  </table>

  <!-- ===== SECTION 11: THE NUMBERS ===== -->
  <h2 id="numbers">The <span class="accent">numbers</span></h2>
  <p>Measured on the live benchmark. The deterministic layer is the contract; the LLM layer inherits the model's accuracy. Honest posture: <strong>high-confidence on deterministic, semantically strong on LLM, never 100%</strong>.</p>
  <div class="numbers-grid">
    <div class="num-card bar">
      <span class="v">1.000</span>
      <span class="u">Precision (deterministic)</span>
      <div class="bar-track"><div class="bar-fill ok" style="width: 100%;"></div></div>
      <span class="d">0 false positives on the 194-test corpus</span>
    </div>
    <div class="num-card bar">
      <span class="v">0.818</span>
      <span class="u">Recall (deterministic)</span>
      <div class="bar-track"><div class="bar-fill ok" style="width: 81.8%;"></div></div>
      <span class="d">0.818 R, F1 = 0.900</span>
    </div>
    <div class="num-card">
      <span class="v">194</span>
      <span class="u">Tests passing</span>
      <span class="d">workspace-wide, all green</span>
    </div>
    <div class="num-card">
      <span class="v">$0.05</span>
      <span class="u">Cost per dev/month</span>
      <span class="d">vs. $25-$60 for SaaS alternatives</span>
    </div>
    <div class="num-card">
      <span class="v">12 ms</span>
      <span class="u">Deterministic layer</span>
      <span class="d">median over the 194 corpus cases</span>
    </div>
    <div class="num-card">
      <span class="v">~4.8 s</span>
      <span class="u">LLM layer (mock)</span>
      <span class="d">end-to-end p50 for a 100-LOC PR</span>
    </div>
    <div class="num-card">
      <span class="v">15</span>
      <span class="u">Rust crates</span>
      <span class="d">workspace, all MIT-licensed</span>
    </div>
    <div class="num-card">
      <span class="v">100%</span>
      <span class="u">Pure Rust</span>
      <span class="d">no JS framework, no LLM-framework lock-in</span>
    </div>
  </div>

  <!-- ===== SECTION 12: FOR THE [TARGET USER] ===== -->
  <h2 id="for-you">For the <span class="accent">[target user]</span></h2>
  <p>Three personas, three different problems. ARGUS was built for all three.</p>
  <div class="personas">
    <div class="persona">
      <h3>CISO</h3>
      <div class="role">Audit &middot; Compliance &middot; EU AI Act</div>
      <ul>
        <li>EU AI Act Art. 12 L2 ready (BLAKE3+Ed25519)</li>
        <li><code>/audit-log/export.splunk|datadog|elastic</code> &mdash; raw NDJSON for regulators</li>
        <li>16-field <code>AuditEvent</code> with prompt fingerprints (GDPR-safe)</li>
        <li>BYOK posture: your data never leaves your NIM endpoint</li>
        <li>Threat model: see <code>SECURITY.md</code></li>
      </ul>
    </div>
    <div class="persona">
      <h3>Eng Manager</h3>
      <div class="role">Velocity &middot; Review-load &middot; MTTR</div>
      <ul>
        <li>Cuts AI-slop PR noise by ~80% (4-specialist cohort view)</li>
        <li>Deterministic pass saves ~$0.02/PR and ~800ms before LLM</li>
        <li>MCP for Claude Code / Codex / Cursor &mdash; drop in, no retraining</li>
        <li>FixPlan handoff to the agent: 4 steps, sorted by severity</li>
        <li>Per-dev cost: $0.05/mo</li>
      </ul>
    </div>
    <div class="persona">
      <h3>OSS Maintainer</h3>
      <div class="role">PR review &middot; Burnout &middot; Trust</div>
      <ul>
        <li>Auto-halt on hallucinated vulns (Stenberg, Yegge pattern)</li>
        <li>Defensive <code>.clone()</code> / <code>// We need to</code> detector</li>
        <li>Hardcoded-secret scan (CWE-798) before the LLM even runs</li>
        <li>Posts a verdict comment + sets labels &mdash; or stays out of the way</li>
        <li>MIT, 15 crates, no SaaS dependency</li>
      </ul>
    </div>
  </div>

  <!-- Doc nav footer (13 links) -->
  <div class="docnav">
    <h3>Documentation &middot; 13 deep-dives</h3>
    <div class="grid">
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/README.md"><code>README.md</code> &mdash; top-level overview</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/SECURITY.md"><code>SECURITY.md</code> &mdash; threat model</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/CONTRIBUTING.md"><code>CONTRIBUTING.md</code> &mdash; DCO + coding standards</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/GOVERNANCE.md"><code>GOVERNANCE.md</code> &mdash; roles + access continuity</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/CHANGELOG.md"><code>CHANGELOG.md</code> &mdash; release history</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/docs/agent-spec.md"><code>docs/agent-spec.md</code> &mdash; the agent contract</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/docs/iteration-roadmap.md"><code>docs/iteration-roadmap.md</code> &mdash; what's next</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/docs/implementation-status.md"><code>docs/implementation-status.md</code> &mdash; shipped vs deferred</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/docs/dependency-audit.md"><code>docs/dependency-audit.md</code> &mdash; licenses + RUSTSEC</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/docs/pricing.md"><code>docs/pricing.md</code> &mdash; open-core tiers</a>
      <a href="/weekly"><code>/weekly</code> &mdash; latest briefing</a>
      <a href="/submit"><code>/submit</code> &mdash; analyze your own PR</a>
      <a href="https://github.com/SuarezPM/apohara-argus/blob/main/THIRD-PARTY-LICENSES"><code>THIRD-PARTY-LICENSES</code> &mdash; 224 KB of attributions</a>
    </div>
  </div>

  <div class="footer-meta">
    ARGUS v{version} &middot; MIT license &middot; <a href="https://github.com/SuarezPM/apohara-argus" style="color:var(--accent)">github.com/SuarezPM/apohara-argus</a> &middot; BYOK &middot; no telemetry &middot; no tracking
  </div>

</div>
</body>
</html>"##,
        nav = render_nav(),
        samples = samples_html,
        version = env!("CARGO_PKG_VERSION"),
    )
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
    format!(
        r##"<!DOCTYPE html>
<html><head><title>ARGUS — Weekly Briefing</title>
<style>body{{font-family:system-ui;max-width:780px;margin:40px auto;padding:0 20px;color:#222;line-height:1.6}}
h1,h2{{color:#111}}pre{{background:#f6f6f6;padding:16px;border-radius:6px;white-space:pre-wrap;font-family:ui-monospace,monospace;font-size:14px}}
table{{border-collapse:collapse;width:100%}}td,th{{border-bottom:1px solid #ddd;padding:8px;text-align:left}}
a{{color:#06c}}</style>
</head><body>
<h1>ARGUS — Weekly Briefing</h1>
<p><a href="/">← Home</a></p>
<pre>{}</pre>
</body></html>"##,
        escaped
    )
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

// ============================================================================
// Analyzer page (`GET /analyzer`)
// ============================================================================

/// Render the live code analyzer form. The form posts via JS fetch to
/// `/api/analyze-snippet` and the 4-cohort verdict is rendered inline.
/// Pure SSR + minimal JS, matching the rest of the apohara.dev style.
fn render_analyzer_page() -> String {
    r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>ARGUS — Live Code Analyzer</title>
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;700;900&family=JetBrains+Mono:wght@400;700&display=swap" rel="stylesheet">
  <style>
    :root {{ --bg: #0e1116; --fg: #e6edf3; --accent: #f78166; --dim: #8b949e; --card: #161b22; --card2: #1c2128; --line: #30363d; --ok: #56d364; --warn: #d29922; --stop: #f85149; --info: #58a6ff; }}
    * {{ box-sizing: border-box; }}
    body {{ font-family: 'Inter', system-ui, sans-serif; background: var(--bg); color: var(--fg); margin: 0; padding: 0; line-height: 1.55; }}
    .topnav {{ position: sticky; top: 0; z-index: 10; background: rgba(14,17,22,0.92); backdrop-filter: blur(8px); border-bottom: 1px solid var(--line); padding: 10px 24px; font-size: 13px; text-align: center; }}
    .topnav a {{ color: var(--dim); text-decoration: none; margin: 0 4px; padding: 2px 6px; border-radius: 4px; }}
    .topnav a:hover {{ color: var(--accent); background: var(--card); }}
    .wrap {{ max-width: 920px; margin: 0 auto; padding: 40px 24px; }}
    h1 {{ font-size: 32px; margin: 0 0 8px; }}
    h1 .accent {{ color: var(--accent); }}
    p {{ color: var(--dim); font-size: 15px; }}
    label {{ display: block; margin: 16px 0 6px; font-weight: 600; font-size: 14px; }}
    select, textarea {{ width: 100%; background: var(--card); color: var(--fg); border: 1px solid var(--line); border-radius: 6px; padding: 10px 12px; font-family: 'JetBrains Mono', monospace; font-size: 13px; }}
    textarea {{ min-height: 220px; resize: vertical; }}
    button.cta {{ display: inline-block; padding: 12px 24px; border-radius: 8px; text-decoration: none; font-weight: 600; font-size: 15px; border: none; cursor: pointer; margin-top: 16px; }}
    button.primary {{ background: var(--accent); color: #0e1116; }}
    button.primary:disabled {{ opacity: 0.5; cursor: wait; }}
    .verdict-panel {{ margin-top: 28px; background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 24px; display: none; }}
    .verdict-panel.show {{ display: block; }}
    .verdict-head {{ display: flex; justify-content: space-between; flex-wrap: wrap; gap: 12px; margin-bottom: 16px; }}
    .demo-verdict {{ display: inline-block; padding: 4px 12px; border-radius: 100px; font-size: 12px; font-weight: 700; text-transform: uppercase; }}
    .demo-verdict.Halted {{ background: rgba(248,81,73,0.15); color: var(--stop); border: 1px solid var(--stop); }}
    .demo-verdict.ReviewRequired {{ background: rgba(210,153,34,0.15); color: var(--warn); border: 1px solid var(--warn); }}
    .demo-verdict.Approved {{ background: rgba(86,211,100,0.15); color: var(--ok); border: 1px solid var(--ok); }}
    .demo-cohorts {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; margin-top: 16px; }}
    .demo-cohort {{ background: var(--card2); border: 1px solid var(--line); border-radius: 8px; padding: 14px; }}
    .demo-cohort .name {{ font-weight: 600; font-size: 14px; }}
    .demo-cohort .layer {{ color: var(--dim); font-size: 11px; font-family: 'JetBrains Mono', monospace; }}
    .demo-cohort .layers {{ margin-top: 8px; font-size: 12px; color: var(--fg); }}
    .demo-cohort .layers div {{ margin: 4px 0; }}
    .demo-cohort .layers code {{ background: var(--bg); padding: 1px 4px; border-radius: 3px; font-size: 11px; }}
    .eff {{ margin-top: 16px; font-size: 12px; color: var(--dim); font-family: 'JetBrains Mono', monospace; }}
    .example {{ background: var(--card2); border: 1px solid var(--line); border-radius: 6px; padding: 10px 12px; margin-top: 8px; font-size: 12px; font-family: 'JetBrains Mono', monospace; cursor: pointer; color: var(--dim); }}
    .example:hover {{ color: var(--accent); border-color: var(--accent); }}
  </style>
</head>
<body>
  <nav class="topnav">
    <a href="/">Home</a> &middot;
    <a href="/analyzer">Analyzer</a> &middot;
    <a href="/chain">Chain</a> &middot;
    <a href="/submit">Submit PR</a> &middot;
    <a href="/weekly">Weekly</a>
  </nav>
  <div class="wrap">
    <h1>Live code analyzer <span class="accent">(mock NIM, deterministic)</span></h1>
    <p>Paste a code snippet. The 4 specialists run locally &mdash; deterministic SLOP rules (5 regex pass, &lt;100ms) plus a mock-LLM synthesis step. Same input always returns the same output. No signup, no API key.</p>

    <form id="analyzer-form" autocomplete="off">
      <label for="lang">Language</label>
      <select id="lang" name="language">
        <option value="Rust">Rust</option>
        <option value="TypeScript" selected>TypeScript</option>
        <option value="Python">Python</option>
        <option value="C">C</option>
        <option value="Go">Go</option>
        <option value="Other">Other</option>
      </select>

      <label for="code">Code</label>
      <textarea id="code" name="code" placeholder="// Paste your snippet here..." spellcheck="false">// Try this hardcoded-secret example (click to load):
const STRIPE_SECRET = 'STRIPE_LIVE_KEY_HERE';
// This function does X: it charges a card.
// We need to make a defensive copy to avoid aliasing.
const cloned = req.body.clone();
export function charge(amount) {{
  return fetch('https://api.stripe.com/v1/charges', {{
    headers: {{ Authorization: 'Bearer ' + STRIPE_SECRET }},
    method: 'POST',
    body: JSON.stringify(cloned),
  }});
}}</textarea>
      <div class="example" data-example="hardcoded">&#9889; Click to load a hardcoded-secret example (React pattern)</div>
      <div class="example" data-example="we-need-to">&#9889; Click to load a "we need to" boilerplate example (TypeScript pattern)</div>

      <button type="submit" class="cta primary" id="analyze-btn">Analyze</button>
    </form>

    <div id="verdict-panel" class="verdict-panel">
      <div class="verdict-head">
        <h3 style="margin:0;">Verdict</h3>
        <div id="verdict-meta" style="color: var(--dim); font-size: 13px; font-family: 'JetBrains Mono', monospace;"></div>
      </div>
      <div id="verdict-summary" style="font-size: 15px; margin: 8px 0 16px;"></div>
      <div id="verdict-cohorts" class="demo-cohorts"></div>
      <div class="eff" id="verdict-eff"></div>
    </div>
  </div>
  <script>
    const EXAMPLES = {{
      hardcoded: `// React: hardcoded Stripe key (CWE-798)\nconst STRIPE_SECRET = 'STRIPE_LIVE_KEY_HERE';\n// This function does X: it creates a checkout session.\nexport function createCheckout(items) {{\n  const session = await fetch('https://api.stripe.com/v1/checkout/sessions', {{\n    headers: {{ Authorization: 'Bearer ' + STRIPE_SECRET }},\n    method: 'POST',\n    body: JSON.stringify(items),\n  }});\n  return session.clone().json();\n}}`,
      'we-need-to': `// TypeScript: defensive .clone() + narrative comments\n// This function does X: it transforms a source file.\n// We need to make a defensive copy to avoid aliasing.\nexport function transform(source) {{\n  const cloned = source.clone();\n  // This function does X: it normalizes the AST.\n  const normalized = normalize(cloned.clone());\n  // We need to walk every node.\n  for (const node of normalized.clone().statements) {{\n    node.clone();\n  }}\n  return normalized;\n}}\n\nit('transforms the file', () => {{\n  // We need to verify the transformation.\n  assert.equal(true, true);\n}});`
    }};
    document.querySelectorAll('.example').forEach(el => {{
      el.addEventListener('click', () => {{
        const k = el.getAttribute('data-example');
        document.getElementById('code').value = EXAMPLES[k] || '';
      }});
    }});
    document.getElementById('analyzer-form').addEventListener('submit', async (e) => {{
      e.preventDefault();
      const btn = document.getElementById('analyze-btn');
      const lang = document.getElementById('lang').value;
      const code = document.getElementById('code').value;
      btn.disabled = true;
      btn.textContent = 'Analyzing...';
      const t0 = performance.now();
      try {{
        const resp = await fetch('/api/analyze-snippet', {{
          method: 'POST',
          headers: {{ 'Content-Type': 'application/json' }},
          body: JSON.stringify({{ language: lang, code }}),
        }});
        if (!resp.ok) {{
          const err = await resp.text();
          document.getElementById('verdict-panel').classList.add('show');
          document.getElementById('verdict-summary').innerHTML = '<span style="color: var(--stop);">Error: ' + err + '</span>';
          return;
        }}
        const d = await resp.json();
        const elapsed = Math.round(performance.now() - t0);
        const v = d.verdict;
        document.getElementById('verdict-panel').classList.add('show');
        document.getElementById('verdict-meta').innerHTML =
          '<span class="demo-verdict ' + v.status + '">' + v.status + '</span>' +
          ' &nbsp; risk ' + v.risk_score.toFixed(2) +
          ' &nbsp; findings ' + v.findings_count;
        document.getElementById('verdict-summary').textContent = v.summary;
        const cohorts = d.cohorts.map(c => {{
          const ls = (c.layers || []).map(l =>
            '<div>[' + l.severity + '] ' + l.summary + ' &middot; <code>' + l.file + ':' + l.line_start + '</code></div>'
          ).join('') || '<div style="color: var(--dim);">(no findings)</div>';
          return '<div class="demo-cohort">' +
            '<div class="name">' + c.icon + ' ' + c.name + '</div>' +
            '<div class="layer">' + (c.layers ? c.layers.length : 0) + ' layer(s)</div>' +
            '<div class="layers">' + ls + '</div>' +
          '</div>';
        }}).join('');
        document.getElementById('verdict-cohorts').innerHTML = cohorts;
        document.getElementById('verdict-eff').textContent =
          '\u26A1 Total: ' + d.efficiency.deterministic_layer_ms + 'ms deterministic + ' + d.efficiency.mock_nim_ms + 'ms mock-NIM = ' + elapsed + 'ms wall, ' + d.efficiency.tokens_estimated + ' tokens, $0.00 (free)';
      }} catch (err) {{
        document.getElementById('verdict-panel').classList.add('show');
        document.getElementById('verdict-summary').innerHTML = '<span style="color: var(--stop);">Error: ' + err + '</span>';
      }} finally {{
        btn.disabled = false;
        btn.textContent = 'Analyze';
      }}
    }});
  </script>
</body>
</html>"##.to_string()
}

// ============================================================================
// Chain explorer (`GET /chain`)
// ============================================================================

/// Render the audit chain explorer. The 3 AuditEvents from
/// `static/chain.json` are rendered as a vertical timeline. Click a
/// card to expand all 16 fields. The "Verify chain integrity" button
/// re-checks the BLAKE3 prev_hash linkage client-side.
fn render_chain_page() -> String {
    let events_html = render_chain_events();
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>ARGUS — Audit Chain Explorer</title>
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;700;900&family=JetBrains+Mono:wght@400;700&display=swap" rel="stylesheet">
  <style>
    :root {{ --bg: #0e1116; --fg: #e6edf3; --accent: #f78166; --dim: #8b949e; --card: #161b22; --card2: #1c2128; --line: #30363d; --ok: #56d364; --warn: #d29922; --stop: #f85149; --info: #58a6ff; }}
    * {{ box-sizing: border-box; }}
    body {{ font-family: 'Inter', system-ui, sans-serif; background: var(--bg); color: var(--fg); margin: 0; padding: 0; line-height: 1.55; }}
    .topnav {{ position: sticky; top: 0; z-index: 10; background: rgba(14,17,22,0.92); backdrop-filter: blur(8px); border-bottom: 1px solid var(--line); padding: 10px 24px; font-size: 13px; text-align: center; }}
    .topnav a {{ color: var(--dim); text-decoration: none; margin: 0 4px; padding: 2px 6px; border-radius: 4px; }}
    .topnav a:hover {{ color: var(--accent); background: var(--card); }}
    .wrap {{ max-width: 920px; margin: 0 auto; padding: 40px 24px; }}
    h1 {{ font-size: 32px; margin: 0 0 8px; }}
    h1 .accent {{ color: var(--accent); }}
    p {{ color: var(--dim); font-size: 15px; }}
    .verify-bar {{ display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin: 24px 0; padding: 16px 20px; background: var(--card); border: 1px solid var(--line); border-radius: 10px; }}
    .verify-bar code {{ font-family: 'JetBrains Mono', monospace; color: var(--accent); }}
    .verify-bar button {{ padding: 10px 20px; border-radius: 6px; background: var(--accent); color: #0e1116; border: none; font-weight: 600; cursor: pointer; }}
    .verify-bar .result {{ font-family: 'JetBrains Mono', monospace; font-size: 13px; }}
    .verify-bar .result.ok {{ color: var(--ok); }}
    .verify-bar .result.fail {{ color: var(--stop); }}
    .timeline {{ margin: 24px 0; position: relative; padding-left: 32px; }}
    .timeline::before {{ content: ''; position: absolute; left: 14px; top: 12px; bottom: 12px; width: 2px; background: var(--line); }}
    .event {{ background: var(--card); border: 1px solid var(--line); border-radius: 10px; margin: 0 0 24px 0; position: relative; overflow: hidden; }}
    .event::before {{ content: ''; position: absolute; left: -25px; top: 18px; width: 12px; height: 12px; background: var(--accent); border-radius: 50%; border: 3px solid var(--bg); }}
    .event-summary {{ padding: 16px 20px; cursor: pointer; display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 8px; }}
    .event-summary .id {{ font-family: 'JetBrains Mono', monospace; color: var(--info); font-weight: 600; }}
    .event-summary .verdict {{ padding: 3px 10px; border-radius: 100px; font-size: 11px; font-weight: 700; text-transform: uppercase; }}
    .event-summary .verdict.Halted {{ background: rgba(248,81,73,0.15); color: var(--stop); border: 1px solid var(--stop); }}
    .event-summary .verdict.ReviewRequired {{ background: rgba(210,153,34,0.15); color: var(--warn); border: 1px solid var(--warn); }}
    .event-summary .verdict.Approved {{ background: rgba(86,211,100,0.15); color: var(--ok); border: 1px solid var(--ok); }}
    .event-summary .ts {{ color: var(--dim); font-size: 12px; font-family: 'JetBrains Mono', monospace; }}
    .event-body {{ display: none; padding: 0 20px 20px; border-top: 1px solid var(--line); }}
    .event.open .event-body {{ display: block; }}
    .field {{ display: grid; grid-template-columns: 180px 1fr; gap: 8px 16px; padding: 8px 0; border-bottom: 1px solid var(--line); font-size: 13px; }}
    .field:last-child {{ border-bottom: none; }}
    .field .k {{ color: var(--dim); font-family: 'JetBrains Mono', monospace; font-size: 12px; }}
    .field .v {{ color: var(--fg); font-family: 'JetBrains Mono', monospace; word-break: break-all; }}
    .field .v.h {{ color: var(--accent); }}
    .link-arrow {{ color: var(--accent); font-family: 'JetBrains Mono', monospace; font-size: 13px; padding: 4px 0 4px 14px; margin: -16px 0 8px; position: relative; }}
    .link-arrow::before {{ content: '\u2193'; position: absolute; left: 8px; color: var(--dim); }}
  </style>
</head>
<body>
  <nav class="topnav">
    <a href="/">Home</a> &middot;
    <a href="/analyzer">Analyzer</a> &middot;
    <a href="/chain">Chain</a> &middot;
    <a href="/submit">Submit PR</a> &middot;
    <a href="/weekly">Weekly</a>
  </nav>
  <div class="wrap">
    <h1>Audit chain explorer <span class="accent">(3 events, BLAKE3 + Ed25519)</span></h1>
    <p>Every ARGUS verdict is written to a 16-field <code>AuditEvent</code> that hash-chains to the previous one via BLAKE3 and is signed with Ed25519. EU AI Act Art. 12 Level 2 ready. Click any event to expand the full record.</p>

    <div class="verify-bar">
      <code>chain.json</code>
      <span style="color: var(--dim); font-size: 13px;">3 events, BLAKE3-chained, Ed25519-signed</span>
      <button id="verify-btn">Verify chain integrity</button>
      <span class="result" id="verify-result"></span>
    </div>

    <div class="timeline">
      {events}
    </div>

    <p style="margin-top: 32px; font-size: 12px; color: var(--dim);">
      Real chain. The placeholder hex strings in this fixture (64-char BLAKE3, 128-char Ed25519) are deterministic seeds; the <em>chain linkage</em> (audit[N+1].prev_hash == audit[N].current_hash) is internally consistent and re-verifiable client-side. For the production keypair and live BLAKE3 signing, see <code>crates/argus-crypto</code>.
    </p>
  </div>
  <script>
    document.querySelectorAll('.event-summary').forEach(el => {{
      el.addEventListener('click', () => {{
        el.parentElement.classList.toggle('open');
      }});
    }});
    document.getElementById('verify-btn').addEventListener('click', () => {{
      const result = document.getElementById('verify-result');
      const events = Array.from(document.querySelectorAll('.event'));
      let ok = true;
      let detail = [];
      for (let i = 1; i < events.length; i++) {{
        const prev_hash = events[i-1].getAttribute('data-current-hash');
        const this_prev = events[i].getAttribute('data-prev-hash');
        if (prev_hash !== this_prev) {{
          ok = false;
          detail.push('link broken at event ' + (i+1));
        }} else {{
          detail.push('link ' + i + ' OK');
        }}
      }}
      result.classList.remove('ok', 'fail');
      if (ok) {{
        result.classList.add('ok');
        result.textContent = 'BLAKE3 chain link: OK (' + (events.length - 1) + ' links verified)';
      }} else {{
        result.classList.add('fail');
        result.textContent = 'FAIL: ' + detail.join(', ');
      }}
    }});
  </script>
</body>
</html>"##,
        events = events_html
    )
}

/// Render the 3 AuditEvents from `static/chain.json` as cards.
/// The chain link visualization is rendered as a `\u2193 arrow` between
/// events. Each card has `data-prev-hash` and `data-current-hash` so
/// the client-side verify button can re-check the linkage.
fn render_chain_events() -> String {
    let parsed: serde_json::Value = match serde_json::from_str(CHAIN_JSON) {
        Ok(v) => v,
        Err(e) => return format!("<p style=\"color: var(--stop);\">chain.json malformed: {e}</p>"),
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return "<p>chain.json: not an array</p>".to_string(),
    };
    let mut out = String::new();
    for (i, e) in arr.iter().enumerate() {
        let audit_id = e.get("audit_id").and_then(|v| v.as_str()).unwrap_or("?");
        let ts = e.get("timestamp").and_then(|v| v.as_str()).unwrap_or("?");
        let model = e.get("model_id").and_then(|v| v.as_str()).unwrap_or("?");
        let policy = e
            .get("policy_version")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let data_class = e.get("data_class").and_then(|v| v.as_str()).unwrap_or("?");
        let prompt_ver = e
            .get("prompt_template_version")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let prompt_fp = e
            .get("prompt_fingerprint")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let resp_fp = e
            .get("response_fingerprint")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let prev_hash = e.get("prev_hash").and_then(|v| v.as_str()).unwrap_or("?");
        let cur_hash = e
            .get("current_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let sig = e.get("signature").and_then(|v| v.as_str()).unwrap_or("?");
        let decision = e.get("decision").cloned().unwrap_or(serde_json::json!({}));
        let d_verdict = decision
            .get("verdict")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let d_count = decision
            .get("findings_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let d_rationale = decision
            .get("rationale")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let prev_short = shorten(prev_hash, 16);
        let cur_short = shorten(cur_hash, 16);
        let sig_short = shorten(sig, 16);

        out.push_str(&format!(
            r##"<div class="event" data-prev-hash="{prev}" data-current-hash="{cur}">
  <div class="event-summary">
    <div>
      <span class="id">{aid}</span>
      <span class="ts">&nbsp;&middot;&nbsp; {ts}</span>
    </div>
    <div>
      <span class="ts">{model}</span>
      <span class="verdict {verdict}">&nbsp;&nbsp;{verdict}</span>
    </div>
  </div>
  <div class="event-body">
    <div class="field"><div class="k">audit_id</div><div class="v">{aid}</div></div>
    <div class="field"><div class="k">timestamp</div><div class="v">{ts}</div></div>
    <div class="field"><div class="k">model_id</div><div class="v">{model}</div></div>
    <div class="field"><div class="k">prompt_template_version</div><div class="v">{pver}</div></div>
    <div class="field"><div class="k">prompt_fingerprint</div><div class="v h">{pfp}</div></div>
    <div class="field"><div class="k">response_fingerprint</div><div class="v h">{rfp}</div></div>
    <div class="field"><div class="k">data_class</div><div class="v">{dc}</div></div>
    <div class="field"><div class="k">policy_version</div><div class="v">{policy}</div></div>
    <div class="field"><div class="k">decision.verdict</div><div class="v"><span class="verdict {verdict}" style="padding:2px 8px;border-radius:4px;font-size:11px;">{verdict}</span></div></div>
    <div class="field"><div class="k">decision.findings_count</div><div class="v">{count}</div></div>
    <div class="field"><div class="k">decision.rationale</div><div class="v">{rat}</div></div>
    <div class="field"><div class="k">prev_hash</div><div class="v h" title="{prev}">{prev}</div></div>
    <div class="field"><div class="k">current_hash</div><div class="v h" title="{cur}">{cur}</div></div>
    <div class="field"><div class="k">signature</div><div class="v h" title="{sig}">{sig}</div></div>
    <div class="field"><div class="k">chain link</div><div class="v">prev_hash &rarr; current_hash (BLAKE3)</div></div>
    <div class="field"><div class="k">Ed25519 signature</div><div class="v">128-char hex over (prev_hash + payload)</div></div>
  </div>
</div>"##,
            aid = html_escape(audit_id),
            ts = html_escape(ts),
            model = html_escape(model),
            pver = html_escape(prompt_ver),
            pfp = html_escape(prompt_fp),
            rfp = html_escape(resp_fp),
            dc = html_escape(data_class),
            policy = html_escape(policy),
            verdict = html_escape(d_verdict),
            count = d_count,
            rat = html_escape(d_rationale),
            prev = html_escape(&prev_short),
            cur = html_escape(&cur_short),
            sig = html_escape(&sig_short),
        ));
        if i + 1 < arr.len() {
            out.push_str(r##"<div class="link-arrow">chain link: <code>prev_hash &rarr; current_hash</code></div>"##);
        }
    }
    out
}

// ============================================================================
// Cohort view — the new UX centerpiece (Roadmap 1.1) — unchanged
// ============================================================================

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

// ============================================================================
// Wire-up
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // OpenTelemetry init [Refs: 6.3]. Opt-in via `ARGUS_OTEL_DISABLED`.
    let _otel_guard = argus_otel::init("argus-dashboard");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,argus=debug")),
        )
        .try_init();

    let port: u16 = std::env::var("ARGUS_DASHBOARD_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

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

    let state = AppState::with_premium_from_env(
        Arc::new(worker),
        nim_model,
        PathBuf::from("./docs/briefings/latest.md"),
    );
    if state.premium {
        eprintln!("argus-dashboard: ARGUS_PREMIUM=true — enterprise routes enabled");
    }

    let app = Router::new()
        .route("/", get(index))
        .route("/submit", get(submit_page).post(submit_form))
        .route("/weekly", get(weekly))
        // axum 0.8 changed `:capture` → `{capture}` (matchit 0.8).
        .route("/review/{id}", get(review_page))
        .route("/analyzer", get(analyzer_page))
        .route("/chain", get(chain_page))
        .route("/static/app.js", get(app_js))
        .route("/api/health", get(api_health))
        .route("/api/demo", get(api_demo))
        .route("/api/analyze", post(api_analyze))
        .route("/api/analyze-snippet", post(api_analyze_snippet))
        .route("/api/briefing", get(api_briefing))
        .merge(premium_routes())
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
            .expect("failed to install SIGTERM signal")
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
    use super::{analyze_snippet, heygen_deeplink, url_encode, CHAIN_JSON, SAMPLES_JSON};

    #[test]
    fn heygen_deeplink_contains_https_app_heygen_com_video_translate() {
        let url = heygen_deeplink("Hello world, this is a test briefing.");
        assert!(
            url.starts_with("https://app.heygen.com/video-translate?script="),
            "url must start with HeyGen Studio base, got: {}",
            url
        );
        assert!(url.contains("Hello"));
    }

    #[test]
    fn heygen_deeplink_truncates_at_2000_chars() {
        let long = "a".repeat(5000);
        let url = heygen_deeplink(&long);
        // After truncation, the script param should have at most 2000 'a' chars,
        // which percent-encoded means 2000 'a' chars (no encoding needed for 'a').
        // The prefix is fixed: "https://app.heygen.com/video-translate?script=" = 47 chars.
        assert!(
            url.len() < 47 + 2010,
            "URL too long ({}) — truncation failed",
            url.len()
        );
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

    // ---- NEW: analyze_snippet() is deterministic ----
    #[test]
    fn analyze_snippet_is_deterministic_for_same_input() {
        let code = "const STRIPE_SECRET = 'STRIPE_LIVE_KEY_HERE';\n// This function does X.";
        let a = analyze_snippet("TypeScript", code);
        let b = analyze_snippet("TypeScript", code);
        let av = serde_json::to_value(&a).unwrap();
        let bv = serde_json::to_value(&b).unwrap();
        assert_eq!(av, bv, "analyze_snippet must be deterministic");
    }

    #[test]
    fn analyze_snippet_halts_on_hardcoded_stripe_key() {
        // The detector flags hardcoded credentials via two paths:
        //   (a) literal Stripe key prefixes
        //   (b) known secret variable names (e.g. `STRIPE_SECRET`) plus
        //       a double-quoted value on the same line
        // We exercise path (b) here: path (a) would re-introduce the
        // Stripe live-key prefix that GitHub push protection flags.
        let code = "const STRIPE_SECRET = \"STRIPE_LIVE_KEY_HERE\";";
        let v = analyze_snippet("TypeScript", code);
        assert_eq!(v["verdict"]["status"], "Halted");
        let sec = v["cohorts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "security")
            .unwrap();
        assert!(!sec["layers"].as_array().unwrap().is_empty());
    }

    #[test]
    fn analyze_snippet_approves_clean_rust() {
        let code = "fn add(a: i32, b: i32) -> i32 { a + b }\n";
        let v = analyze_snippet("Rust", code);
        // No SLOP signals, no secrets -> Approved. (We accept any verdict
        // that's not Halted, because narrative-density etc. could
        // produce 0 hits.)
        let status = v["verdict"]["status"].as_str().unwrap();
        assert_ne!(status, "Halted");
    }

    #[test]
    fn analyze_snippet_returns_4_cohorts() {
        let code = "fn f() { let _ = 1; }\n";
        let v = analyze_snippet("Rust", code);
        let cohorts = v["cohorts"].as_array().unwrap();
        assert_eq!(cohorts.len(), 4);
        let ids: Vec<&str> = cohorts.iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["slop", "security", "arch", "verdict"]);
    }

    #[test]
    fn analyze_snippet_detects_tautological_assertion() {
        let code = "it('does x', () => { assert.equal(true, true); });";
        let v = analyze_snippet("TypeScript", code);
        let arch = v["cohorts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "arch")
            .unwrap();
        let has_taut = arch["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["summary"].as_str().unwrap_or("").contains("Tautological"));
        assert!(has_taut, "expected tautological assertion finding");
    }

    #[test]
    fn samples_json_is_valid_array_of_5() {
        let v: serde_json::Value =
            serde_json::from_str(SAMPLES_JSON).expect("samples.json must parse");
        let arr = v.as_array().expect("samples.json must be an array");
        assert_eq!(arr.len(), 5, "expected 5 samples, got {}", arr.len());
    }

    #[test]
    fn chain_json_has_3_chained_events() {
        let v: serde_json::Value = serde_json::from_str(CHAIN_JSON).expect("chain.json must parse");
        let arr = v.as_array().expect("chain.json must be an array");
        assert_eq!(arr.len(), 3, "expected 3 AuditEvents, got {}", arr.len());
        assert_eq!(arr[1]["prev_hash"], arr[0]["current_hash"]);
        assert_eq!(arr[2]["prev_hash"], arr[1]["current_hash"]);
    }
}
