//! Quick smoke test: connects to NVIDIA NIM with the BYOK key, sends a
//! trivial completion, prints the result.
//!
//! Usage:
//!   ARGUS_NIM_KEY=nvapi-xxx cargo run -p argus-llm --example nim_smoke
//!
//! If it works, you'll see a real LLM response. If it fails, you'll see
//! the error and a hint.

use argus_llm::{LlmClient, NimClient};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env if present (optional)
    let _ = dotenv_load();

    let key =
        env::var("ARGUS_NIM_KEY").expect("Set ARGUS_NIM_KEY env var (your NVIDIA NIM BYOK key)");
    if key.is_empty() {
        eprintln!("ARGUS_NIM_KEY is empty. Get a free key at https://build.nvidia.com/");
        std::process::exit(1);
    }

    let model =
        env::var("ARGUS_NIM_MODEL").unwrap_or_else(|_| "meta/llama-3.1-70b-instruct".to_string());

    println!("→ Connecting to NVIDIA NIM with model: {}", model);
    println!("→ API key: {}...{}", &key[..8], &key[key.len() - 4..]);

    let client = NimClient::new().with_model(model.clone());

    let sys = "You are a helpful assistant. Always answer in one short sentence.";
    let user = "Reply with exactly: 'ARGUS_NIM_OK' and nothing else.";

    let start = std::time::Instant::now();
    let resp = client
        .complete_one_shot(&model, sys, user, &key, 0.0, 64)
        .await?;
    let elapsed = start.elapsed();

    println!("\n✓ Response received in {:?}", elapsed);
    println!("  Model: {}", resp.model);
    println!(
        "  Tokens: prompt={} completion={} total={}",
        resp.usage.prompt_tokens, resp.usage.completion_tokens, resp.usage.total_tokens
    );
    println!("\n  Content: {}", resp.content.trim());

    if resp.content.contains("ARGUS_NIM_OK") {
        println!("\n✓ Smoke test passed. ARGUS can talk to NVIDIA NIM.");
    } else {
        println!("\n⚠ Unexpected response content (but the call worked).");
    }
    Ok(())
}

/// Minimal .env loader (we don't depend on dotenvy in argus-llm).
fn dotenv_load() -> std::io::Result<()> {
    use std::fs;
    let path = std::path::Path::new(".env");
    if !path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            let v = v.trim().trim_matches('"').trim_matches('\'');
            if env::var(k).is_err() {
                env::set_var(k, v);
            }
        }
    }
    Ok(())
}
