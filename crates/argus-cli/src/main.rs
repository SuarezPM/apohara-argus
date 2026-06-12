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
    #[arg(long, env = "ARGUS_NIM_MODEL", global = true, default_value = "meta/llama-3.1-70b-instruct")]
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
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let cli = Cli::parse();

    if cli.nim_key.is_empty() {
        eprintln!("Error: --nim-key or ARGUS_NIM_KEY env var is required (BYOK).");
        eprintln!("Get a free key at https://build.nvidia.com/");
        return ExitCode::from(2);
    }

    match cli.cmd {
        Cmd::Guard { diff, json } => {
            let runner = argus_guard::GuardRunner::new(&cli.nim_key).with_model(&cli.nim_model);
            let d = match argus_guard::GuardRunner::read_diff(diff.as_ref()) {
                Ok(d) => d,
                Err(e) => { eprintln!("Error: {}", e); return ExitCode::from(2); }
            };
            let out = match runner.run(&d).await {
                Ok(o) => o,
                Err(e) => { eprintln!("Pipeline error: {}", e); return ExitCode::from(2); }
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            } else {
                print!("{}", out.render_terminal());
            }
            ExitCode::from(out.decision.exit_code() as u8)
        }
        Cmd::Verify { pr_url, post_comment, set_labels, json } => {
            let gh_token = std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty());
            let worker = if let Some(tok) = gh_token {
                argus_verify::VerifyWorker::new(&cli.nim_key)
                    .with_model(&cli.nim_model)
                    .with_github(argus_github::GitHubClient::new(tok))
            } else {
                argus_verify::VerifyWorker::new(&cli.nim_key).with_model(&cli.nim_model)
            };
            let req = argus_verify::AnalyzeRequest {
                pr_url,
                repo_context: None,
                post_comment,
                set_labels,
            };
            let resp = match worker.analyze(req).await {
                Ok(r) => r,
                Err(e) => { eprintln!("Error: {}", e); return ExitCode::from(2); }
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&resp).unwrap());
            } else {
                eprintln!("\n=== ARGUS Aegis Verify ===");
                eprintln!("PR: {}", resp.pr_ref);
                eprintln!("Status: {:?}", resp.verdict.status);
                eprintln!("Risk: {:.2}", resp.verdict.risk_score.as_f32());
                eprintln!("Summary: {}", resp.verdict.summary);
                eprintln!("Slop: {} | Fit: {} | Sec: {}",
                    resp.slop_score.map(|s| format!("{:.2}", s)).unwrap_or("n/a".into()),
                    resp.fit_score.map(|s| format!("{:.2}", s)).unwrap_or("n/a".into()),
                    resp.security_summary.as_deref().unwrap_or("n/a"));
                eprintln!("Comment posted: {} | Labels set: {}", resp.comment_posted, resp.labels_set);
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
            let exit = match resp.verdict.status {
                argus_core::VerdictStatus::Approved => 0,
                argus_core::VerdictStatus::ReviewRequired => 0,
                argus_core::VerdictStatus::Halted => 1,
            };
            ExitCode::from(exit as u8)
        }
        Cmd::Lens { org, mock_prs, output } => {
            use argus_lens::{LensRunner, PRBriefSummary};
            let prs: Vec<PRBriefSummary> = if !mock_prs.is_empty() {
                mock_prs.iter().enumerate().map(|(i, pr_ref)| {
                    PRBriefSummary {
                        pr_ref: pr_ref.clone(),
                        author: format!("dev{}", i + 1),
                        risk_score: 0.2 + (i as f32 * 0.15) % 0.8,
                        top_finding: if i == 0 { "hardcoded secret in config.py".into() } else { "minor AI slop signals".into() },
                        critical_findings: if i == 0 { 1 } else { 0 },
                    }
                }).collect()
            } else {
                eprintln!("Use --mock-prs to seed demo data");
                return ExitCode::from(2);
            };
            let runner = LensRunner::new().with_model(&cli.nim_model);
            match runner.run(&org, &prs, &cli.nim_key).await {
                Ok(out) => {
                    if let Some(parent) = std::path::Path::new(&output).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&output, &out.markdown) {
                        eprintln!("Error writing output: {}", e);
                        return ExitCode::from(2);
                    }
                    println!("✓ Briefing written to {}", output);
                    println!("\n{}", out.markdown);
                    ExitCode::from(0)
                }
                Err(e) => { eprintln!("Lens run failed: {}", e); ExitCode::from(2) }
            }
        }
        Cmd::Prompts => {
            let lib = argus_core::PromptLibrary::load_embedded().expect("load");
            eprintln!("ARGUS Prompt Library — 4 interconnected prompts:\n");
            for name in lib.list() {
                if let Some(p) = lib.get(name) {
                    eprintln!("▸ {} ({})", p.metadata.name, p.metadata.model);
                    eprintln!("  {}", p.metadata.description);
                    eprintln!("  temp={} max_tokens={}\n", p.metadata.temperature, p.metadata.max_tokens);
                }
            }
            ExitCode::from(0)
        }
        Cmd::Health => {
            use argus_llm::LlmClient;
            // Article 19 retention surfacing (Roadmap 2.4): cheap, instant
            // feedback on the configured audit retention window. Print
            // before the NIM round-trip so the line is visible even if the
            // network call hangs or fails.
            let config = argus_core::config::Config::from_env()
                .expect("config: from_env only fails on dotenv I/O, never on defaults");
            eprintln!("{}", format_retention_line(config.retention_days));
            let client = argus_llm::NimClient::new();
            eprintln!("→ Testing NIM connectivity...");
            let resp = client.complete_one_shot(
                &cli.nim_model,
                "You are a health-check echo. Reply with exactly 'ARGUS_OK'.",
                "ping",
                &cli.nim_key,
                0.0, 16,
            ).await;
            match resp {
                Ok(r) => {
                    if r.content.contains("ARGUS_OK") {
                        eprintln!("✓ NIM healthy ({} tokens)", r.usage.total_tokens);
                        ExitCode::from(0)
                    } else {
                        eprintln!("⚠ NIM responded but content unexpected: {}", r.content.trim());
                        ExitCode::from(1)
                    }
                }
                Err(e) => { eprintln!("✗ NIM failed: {}", e); ExitCode::from(2) }
            }
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
