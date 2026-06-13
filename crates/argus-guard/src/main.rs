//! argus-guard CLI
//!
//! Usage:
//!   argus-guard --diff pr.diff                    # analyze a file
//!   git diff | argus-guard                         # analyze stdin
//!   argus-guard --diff pr.diff --json             # JSON output
//!   argus-guard --diff pr.diff --model meta/llama-3.1-405b-instruct

use argus_guard::{Decision, GuardOutput, GuardRunner};
use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "argus-guard",
    about = "ARGUS pre-commit AI slop check",
    version
)]
struct Cli {
    /// Path to a diff file. If omitted, reads from stdin.
    #[arg(long)]
    diff: Option<PathBuf>,

    /// Output JSON instead of a pretty report.
    #[arg(long)]
    json: bool,

    /// Override the LLM model (default: meta/llama-3.1-70b-instruct).
    #[arg(long)]
    model: Option<String>,

    /// NIM API key. Falls back to ARGUS_NIM_KEY env var.
    #[arg(long, env = "ARGUS_NIM_KEY")]
    nim_key: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    if cli.nim_key.is_empty() {
        eprintln!("Error: --nim-key or ARGUS_NIM_KEY env var is required (BYOK).");
        eprintln!("Get a free key at https://build.nvidia.com/");
        return ExitCode::from(2);
    }

    let diff = match GuardRunner::read_diff(cli.diff.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error reading diff: {}", e);
            return ExitCode::from(2);
        }
    };
    if diff.trim().is_empty() {
        eprintln!("Empty diff — nothing to analyze.");
        return ExitCode::from(0);
    }

    let mut runner = GuardRunner::new(&cli.nim_key);
    if let Some(m) = cli.model {
        runner = runner.with_model(m);
    }

    let output: GuardOutput = match runner.run(&diff).await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Pipeline error: {}", e);
            return ExitCode::from(2);
        }
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
    } else {
        print!("{}", output.render_terminal());
    }
    ExitCode::from(output.decision.exit_code() as u8)
}
