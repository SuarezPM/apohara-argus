//! argus-lens CLI: run a weekly digest manually or via cron.
//!
//! Usage:
//!   argus-lens --org acme --repos acme/api acme/web
//!
//! For a real demo, supply a few mock PRs (we don't have the full ARGUS
//! ledger wired up yet). Use --mock-prs to seed fake data.

use argus_lens::{LensRunner, PRBriefSummary};
use clap::Parser;
use std::process::ExitCode;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "argus-lens", about = "ARGUS Aegis Lens — weekly org digest", version)]
struct Cli {
    #[arg(long)]
    org: String,

    /// Comma-separated list of repos to scan. If empty, uses --mock-prs.
    #[arg(long, value_delimiter = ',')]
    repos: Vec<String>,

    /// Comma-separated list of mock PR refs (e.g., "acme/api#1,acme/web#2") for demo.
    #[arg(long, value_delimiter = ',')]
    mock_prs: Vec<String>,

    /// NIM API key. Falls back to ARGUS_NIM_KEY env var.
    #[arg(long, env = "ARGUS_NIM_KEY")]
    nim_key: String,

    /// Output file for the Markdown briefing.
    #[arg(long, default_value = "./docs/briefings/latest.md")]
    output: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let cli = Cli::parse();

    if cli.nim_key.is_empty() {
        eprintln!("Error: --nim-key or ARGUS_NIM_KEY env var is required (BYOK).");
        return ExitCode::from(2);
    }

    // Build PR summaries — either from mock data or (TODO) from the ledger.
    let prs: Vec<PRBriefSummary> = if !cli.mock_prs.is_empty() {
        cli.mock_prs.iter().enumerate().map(|(i, pr_ref)| {
            PRBriefSummary {
                pr_ref: pr_ref.clone(),
                author: format!("dev{}", i + 1),
                risk_score: 0.2 + (i as f32 * 0.15) % 0.8,
                top_finding: if i == 0 { "hardcoded secret in config.py".into() } else { "minor AI slop signals".into() },
                critical_findings: if i == 0 { 1 } else { 0 },
            }
        }).collect()
    } else {
        eprintln!("No repos or mock-prs provided. Use --mock-prs to seed demo data.");
        return ExitCode::from(2);
    };

    let runner = LensRunner::new();
    match runner.run(&cli.org, &prs, &cli.nim_key).await {
        Ok(out) => {
            // Write markdown
            if let Some(parent) = std::path::Path::new(&cli.output).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&cli.output, &out.markdown) {
                eprintln!("Error writing output: {}", e);
                return ExitCode::from(2);
            }
            println!("✓ Briefing written to {}", cli.output);
            println!("\n--- Preview ---\n{}", out.markdown);
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("Lens run failed: {}", e);
            ExitCode::from(2)
        }
    }
}
