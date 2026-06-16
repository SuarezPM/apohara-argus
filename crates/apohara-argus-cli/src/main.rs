//! argus — the unified ARGUS CLI
//!
//! Subcommands:
//!   argus guard --diff <file>      → Aegis Guard (pre-commit check)
//!   argus verify --pr-url <url>   → Aegis Verify (PR review, posts to GH)
//!   argus lens  --org <name>      → Aegis Lens (weekly digest)
//!   argus prompts                 → list the 4 prompts
//!   argus health                  → quick NIM connectivity check
//!
//! NIM key: --nim-key or ARGUS_NIM_KEY env var (BYOK).

use clap::{Parser, Subcommand};
use std::process::ExitCode;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "argus", about = "ARGUS — AI slop defense layer", version)]
struct Cli {
    /// NIM API key. Falls back to ARGUS_NIM_KEY env var.
    #[arg(long, env = "ARGUS_NIM_KEY", global = true, default_value = "")]
    nim_key: String,

    /// Override the LLM model.
    #[arg(
        long,
        env = "ARGUS_NIM_MODEL",
        global = true,
        default_value = "meta/llama-3.1-70b-instruct"
    )]
    nim_model: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Pre-commit AI slop check on a diff.
    Guard {
        #[arg(long)]
        diff: Option<std::path::PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Review a PR via URL and (optionally) post the verdict.
    Verify {
        #[arg(long)]
        pr_url: String,
        #[arg(long, default_value_t = false)]
        post_comment: bool,
        #[arg(long, default_value_t = false)]
        set_labels: bool,
        #[arg(long)]
        json: bool,
    },
    /// Weekly org-wide digest.
    Lens {
        #[arg(long)]
        org: String,
        #[arg(long, value_delimiter = ',')]
        mock_prs: Vec<String>,
        #[arg(long, default_value = "./docs/briefings/latest.md")]
        output: String,
    },
    /// List the 4 Argus Prompt Library prompts.
    Prompts,
    /// Quick NIM connectivity check.
    Health,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    // OpenTelemetry init [Refs: 6.3]. Opt-in via `ARGUS_OTEL_DISABLED`.
    // The `try_init` is a no-op when OTel is disabled.
    let _otel_guard = argus_otel::init("apohara-argus-cli");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init();
    let cli = Cli::parse();

    if cli.nim_key.is_empty() {
        eprintln!("Error: --nim-key or ARGUS_NIM_KEY env var is required (BYOK).");
        eprintln!("Get a free key at https://build.nvidia.com/");
        return ExitCode::from(2);
    }

    // Dispatch to the right command handler. Each handler takes
    // only the fields it needs (not `&Cli`) so the match can
    // move `cli.cmd` into the variant without partially moving
    // the whole struct.
    let nim_key = cli.nim_key.clone();
    let nim_model = cli.nim_model.clone();
    match cli.cmd {
        Cmd::Guard { diff, json } => run_guard(&nim_key, &nim_model, diff.as_ref(), json).await,
        Cmd::Verify {
            pr_url,
            post_comment,
            set_labels,
            json,
        } => run_verify(&nim_key, &nim_model, pr_url, post_comment, set_labels, json).await,
        Cmd::Lens {
            org,
            mock_prs,
            output,
        } => run_lens(&nim_key, &nim_model, &org, &mock_prs, &output).await,
        Cmd::Prompts => run_prompts(),
        Cmd::Health => run_health(&nim_key, &nim_model).await,
    }
}

/// `argus guard` — pre-commit hook. Reads a diff, runs the
/// deterministic + LLM layers, prints the verdict.
async fn run_guard(
    nim_key: &str,
    nim_model: &str,
    diff: Option<&std::path::PathBuf>,
    json: bool,
) -> ExitCode {
    let runner = argus_guard::GuardRunner::new(nim_key).with_model(nim_model);
    let d = match argus_guard::GuardRunner::read_diff(diff) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(2);
        }
    };
    let out = match runner.run(&d).await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Pipeline error: {}", e);
            return ExitCode::from(2);
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        print!("{}", out.render_terminal());
    }
    ExitCode::from(out.decision.exit_code() as u8)
}

/// `argus verify` — PR review. Builds the worker (with or without
/// a GitHub client depending on `GITHUB_TOKEN`), runs the analysis,
/// prints the result.
async fn run_verify(
    nim_key: &str,
    nim_model: &str,
    pr_url: String,
    post_comment: bool,
    set_labels: bool,
    json: bool,
) -> ExitCode {
    let worker = build_verify_worker(nim_key, nim_model);
    let req = argus_verify::AnalyzeRequest {
        pr_url,
        repo_context: None,
        post_comment,
        set_labels,
    };
    let resp = match worker.analyze(req).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(2);
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&resp).unwrap());
    } else {
        print_verify_human(&resp);
    }
    verify_exit_code(&resp.verdict.status)
}

/// Build the VerifyWorker, optionally attaching a GitHub client
/// if `GITHUB_TOKEN` is set. The GitHub client is only needed
/// when `post_comment` or `set_labels` is true, but the worker
/// ignores it if those flags are off — so we attach it always
/// when the token is present (cheaper than checking the flags).
fn build_verify_worker(nim_key: &str, nim_model: &str) -> argus_verify::VerifyWorker {
    let gh_token = std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty());
    match gh_token {
        Some(tok) => argus_verify::VerifyWorker::new(nim_key)
            .with_model(nim_model)
            .with_github(argus_github::GitHubClient::new(tok)),
        None => argus_verify::VerifyWorker::new(nim_key).with_model(nim_model),
    }
}

/// Human-readable rendering of the verify response. Mirrors
/// the layout the operator sees in the dashboard.
fn print_verify_human(resp: &argus_verify::AnalyzeResponse) {
    eprintln!("\n=== ARGUS Aegis Verify ===");
    eprintln!("PR: {}", resp.pr_ref);
    eprintln!("Status: {:?}", resp.verdict.status);
    eprintln!("Risk: {:.2}", resp.verdict.risk_score.as_f32());
    eprintln!("Summary: {}", resp.verdict.summary);
    eprintln!(
        "Slop: {} | Fit: {} | Sec: {}",
        resp.slop_score
            .map(|s| format!("{:.2}", s))
            .unwrap_or("n/a".into()),
        resp.fit_score
            .map(|s| format!("{:.2}", s))
            .unwrap_or("n/a".into()),
        resp.security_summary.as_deref().unwrap_or("n/a")
    );
    eprintln!(
        "Comment posted: {} | Labels set: {}",
        resp.comment_posted, resp.labels_set
    );
    eprintln!("\nFindings:");
    for f in &resp.verdict.key_findings {
        eprintln!("  - {}", f);
    }
    eprintln!("\nAction items:");
    for a in &resp.verdict.action_items {
        eprintln!("  - {}", a);
    }
    eprintln!("\nLedger hash: {}", resp.review.ledger_signature);
}

/// Map verdict status to process exit code. Approved and
/// ReviewRequired both return 0 (the review is informational;
/// CI decides whether to block). Halted is 1.
fn verify_exit_code(status: &apohara_argus_core::VerdictStatus) -> ExitCode {
    let code = match status {
        apohara_argus_core::VerdictStatus::Approved => 0,
        apohara_argus_core::VerdictStatus::ReviewRequired => 0,
        apohara_argus_core::VerdictStatus::Halted => 1,
    };
    ExitCode::from(code as u8)
}

/// `argus lens` — weekly digest. Seeds demo PRs from `--mock-prs`,
/// runs the lens, writes the markdown to `--output`.
async fn run_lens(
    nim_key: &str,
    nim_model: &str,
    org: &str,
    mock_prs: &[String],
    output: &str,
) -> ExitCode {
    use argus_lens::{LensRunner, PRBriefSummary};

    if mock_prs.is_empty() {
        eprintln!("Use --mock-prs to seed demo data");
        return ExitCode::from(2);
    }
    let prs: Vec<PRBriefSummary> = mock_prs
        .iter()
        .enumerate()
        .map(|(i, pr_ref)| PRBriefSummary {
            pr_ref: pr_ref.clone(),
            author: format!("dev{}", i + 1),
            // Vary the risk score so the briefing has interesting
            // shape (alternates between 0.2 and 1.0).
            risk_score: 0.2 + (i as f32 * 0.15) % 0.8,
            top_finding: if i == 0 {
                "hardcoded secret in config.py".into()
            } else {
                "minor AI slop signals".into()
            },
            critical_findings: if i == 0 { 1 } else { 0 },
        })
        .collect();

    let runner = LensRunner::new().with_model(nim_model);
    match runner.run(org, &prs, nim_key).await {
        Ok(out) => {
            if let Some(parent) = std::path::Path::new(output).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(output, &out.markdown) {
                eprintln!("Error writing output: {}", e);
                return ExitCode::from(2);
            }
            println!("✓ Briefing written to {}", output);
            println!("\n{}", out.markdown);
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("Lens run failed: {}", e);
            ExitCode::from(2)
        }
    }
}

/// `argus prompts` — print the 4 specialist prompt metadata
/// (name, model, description, temperature, max_tokens).
fn run_prompts() -> ExitCode {
    let lib = apohara_argus_core::PromptLibrary::load_embedded().expect("load");
    eprintln!("ARGUS Prompt Library — 4 interconnected prompts:\n");
    for name in lib.list() {
        if let Some(p) = lib.get(name) {
            eprintln!("▸ {} ({})", p.metadata.name, p.metadata.model);
            eprintln!("  {}", p.metadata.description);
            eprintln!(
                "  temp={} max_tokens={}\n",
                p.metadata.temperature, p.metadata.max_tokens
            );
        }
    }
    ExitCode::from(0)
}

/// `argus health` — retention line + NIM connectivity check.
/// Article 19 of the EU AI Act requires logging "throughout the
/// lifecycle" of high-risk AI systems. We surface the configured
/// retention window first (before the network round-trip) so the
/// line is visible even if the NIM call hangs or fails.
async fn run_health(nim_key: &str, nim_model: &str) -> ExitCode {
    use argus_llm::LlmClient;

    let config = apohara_argus_core::config::Config::from_env()
        .expect("config: from_env only fails on dotenv I/O, never on defaults");
    eprintln!("{}", format_retention_line(config.retention_days));

    let client = argus_llm::NimClient::new();
    eprintln!("→ Testing NIM connectivity...");
    match client
        .complete_one_shot(
            nim_model,
            "You are a health-check echo. Reply with exactly 'ARGUS_OK'.",
            "ping",
            nim_key,
            0.0,
            16,
        )
        .await
    {
        Ok(r) if r.content.contains("ARGUS_OK") => {
            eprintln!("✓ NIM healthy ({} tokens)", r.usage.total_tokens);
            ExitCode::from(0)
        }
        Ok(r) => {
            eprintln!(
                "⚠ NIM responded but content unexpected: {}",
                r.content.trim()
            );
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("✗ NIM failed: {}", e);
            ExitCode::from(2)
        }
    }
}

/// Render the retention-policy line for `argus health`.
///
/// Article 19 of the EU AI Act requires logging "throughout the lifecycle"
/// of high-risk AI systems. Internally ARGUS treats 180 days as the minimum
/// acceptable retention window; anything below that gets the warning glyph.
fn format_retention_line(days: u32) -> String {
    if days >= 180 {
        format!("✓ Retention {}d (≥ 180d Article 19 minimum)", days)
    } else {
        format!("⚠ Retention {}d < Article 19 minimum (180d)", days)
    }
}

#[cfg(test)]
mod tests {
    use super::format_retention_line;

    #[test]
    fn happy_default_above_minimum() {
        assert_eq!(
            format_retention_line(365),
            "✓ Retention 365d (≥ 180d Article 19 minimum)"
        );
    }

    #[test]
    fn edge_exactly_at_minimum_is_ok() {
        assert_eq!(
            format_retention_line(180),
            "✓ Retention 180d (≥ 180d Article 19 minimum)"
        );
    }

    #[test]
    fn edge_just_below_minimum_warns() {
        assert_eq!(
            format_retention_line(179),
            "⚠ Retention 179d < Article 19 minimum (180d)"
        );
    }

    #[test]
    fn edge_short_retention_warns() {
        assert_eq!(
            format_retention_line(30),
            "⚠ Retention 30d < Article 19 minimum (180d)"
        );
    }

    #[test]
    fn regression_zero_days_warns() {
        assert_eq!(
            format_retention_line(0),
            "⚠ Retention 0d < Article 19 minimum (180d)"
        );
    }
}
