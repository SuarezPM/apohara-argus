//! End-to-end diff -> verdict latency.
//!
//! Source: `data/prs.jsonl`. Times the FULL pipeline the user pays
//! for on every PR review:
//!
//!   start = now()
//   run_deterministic_rules(diff)   // SLOP-001..005, < 100ms
//!   mock_nim_client.complete(...)   // mock LLM call
//!   verdict = synthesize(...)
//!   elapsed = now() - start
//!
//! 10 random PRs are sampled, each is timed over N iterations, and
//! we report min / p50 / p99 / max per PR. Release-mode timing
//! only (the `cargo bench` profile). Numbers will move with CPU
//! and load; the SHAPE (deterministic layer in low microseconds,
//! mock LLM dominated by the 256-token response shape) is stable.

use std::hint::black_box;
use std::time::{Duration, Instant};

use argus_benchmarks::{load_dataset, MockNimClient};
use argus_llm::{CompletionRequest, LlmClient, Message};
use argus_slop::run_deterministic_rules;

const SYSTEM_PROMPT: &str =
    "You are a Rust code-review assistant. Classify the diff as SLOP or CLEAN.";

/// Iterations timed per PR. 200 is enough for a stable p99 over a
/// workload that runs in low microseconds and keeps the bench fast.
const ITERS: usize = 200;

/// Number of PRs to sample. 10 is the spec; we use the first 10
/// (deterministic) since random sampling on a 37-entry dataset
/// would just add run-to-run noise without changing the headline.
const SAMPLE_SIZE: usize = 10;

fn user_msg(diff: &str) -> String {
    format!(
        "Analyze the following PR diff for AI-generated code signals. \
         Return ONLY valid JSON.\n\n```diff\n{}\n```",
        diff
    )
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank]
}

async fn run_one(client: &MockNimClient, diff: &str) {
    let _signals = run_deterministic_rules(black_box(diff));
    let req = CompletionRequest::new(
        "mock-nim-v1",
        vec![
            Message::system(SYSTEM_PROMPT),
            Message::user(user_msg(diff)),
        ],
    )
    .with_temperature(0.0)
    .with_max_tokens(256);
    let resp = client
        .complete(black_box(req), "mock-key")
        .await
        .expect("mock");
    let parsed: serde_json::Value = serde_json::from_str(&resp.content).expect("json");
    let _verdict = parsed.get("verdict").and_then(|v| v.as_str());
    black_box(resp);
}

fn report(id: &str, title: &str, durs: &mut [Duration]) {
    durs.sort_unstable();
    let min = durs[0];
    let p50 = percentile(durs, 0.50);
    let p99 = percentile(durs, 0.99);
    let max = durs[durs.len() - 1];
    let title_short: String = if title.len() > 36 {
        format!("{}…", &title[..35])
    } else {
        title.to_string()
    };
    println!(
        "  {id:<8} {title_short:<38} min={min:>10.3?}  p50={p50:>10.3?}  p99={p99:>10.3?}  max={max:>10.3?}",
    );
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    // `cargo bench` runs the binary with cwd = the package dir
    // (crates/argus-benchmarks), not the workspace root. Anchor the data
    // path to CARGO_MANIFEST_DIR so it works regardless of cwd.
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let data_dir = manifest_dir.join("data");
    let (prs, _labels) = load_dataset(data_dir.join("prs.jsonl"), data_dir.join("labels.json"))
        .expect("load dataset");
    let client = MockNimClient::new(&prs);

    let sample: Vec<_> = prs.iter().take(SAMPLE_SIZE).collect();
    println!(
        "End-to-end diff -> verdict latency ({ITERS} iters/PR, {} PRs sampled)",
        sample.len()
    );
    let header = format!(
        "  {:<8} {:<38} min/..      p50/..      p99/..      max/..",
        "id", "title"
    );
    println!("{header}");

    let mut all_p50 = Vec::with_capacity(sample.len());
    let mut all_p99 = Vec::with_capacity(sample.len());

    for pr in &sample {
        // Warm up: pay the lazy-init cost (BLAKE3, regex caches) once.
        for _ in 0..32 {
            run_one(&client, &pr.diff).await;
        }
        let mut durs = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let start = Instant::now();
            run_one(&client, &pr.diff).await;
            durs.push(start.elapsed());
        }
        let p50 = percentile_sorted(&durs, 0.50);
        let p99 = percentile_sorted(&durs, 0.99);
        all_p50.push(p50);
        all_p99.push(p99);
        report(&pr.id, &pr.title, &mut durs);
    }

    // Aggregate.
    all_p50.sort_unstable();
    all_p99.sort_unstable();
    println!();
    println!("=== Aggregate over the 10 sampled PRs (median of medians) ===");
    println!("  p50:  {}", fmt(all_p50[all_p50.len() / 2]));
    println!("  p99:  {}", fmt(all_p99[all_p99.len() / 2]));
}

fn percentile_sorted(durs: &[Duration], p: f64) -> Duration {
    percentile(durs, p)
}

fn fmt(d: Duration) -> String {
    format!("{:.3?}", d)
}
