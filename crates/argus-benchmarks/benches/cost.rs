//! Token cost per verdict.
//!
//! Source: `data/prs.jsonl`. For each of the first 10 PRs, we
//! count the tokens in the (system, user) prompt and the mock's
//! completion, then report:
//!   - prompt_tokens
//!   - completion_tokens
//!   - total_tokens
//!   - estimated USD at $0.001 per 1K tokens (placeholder rate;
//!     the spec says: "use a $0.001/1K token rate as the
//!     placeholder")
//!
//! The mock's `estimate_tokens_sync` skips the async runtime and
//! returns a `Usage` directly; cost is a pure accounting
//! measurement, so the tight loop runs synchronously.

use std::time::{Duration, Instant};

use argus_benchmarks::{load_dataset, MockNimClient};
use argus_llm::Usage;

const SYSTEM_PROMPT: &str =
    "You are a Rust code-review assistant. Classify the diff as SLOP or CLEAN.";

/// Placeholder rate. Real NIM pricing is per-model and per-role;
/// the spec says "use $0.001/1K tokens as the placeholder." We
/// surface that in the BENCHMARK.md "Limitations" section.
const USD_PER_1K_TOKENS: f64 = 0.001;

const SAMPLE_SIZE: usize = 10;

fn user_msg(diff: &str) -> String {
    format!(
        "Analyze the following PR diff for AI-generated code signals. \
         Return ONLY valid JSON.\n\n```diff\n{}\n```",
        diff
    )
}

fn usd(usage: &Usage) -> f64 {
    (usage.total_tokens as f64) * USD_PER_1K_TOKENS / 1000.0
}

fn fmt_n(n: u32) -> String {
    format!("{n:>5}")
}

fn fmt_usd(d: f64) -> String {
    format!("${:.6}", d)
}

fn main() {
    let workspace_root = std::env::current_dir().expect("cwd");
    let data_dir = workspace_root.join("crates/argus-benchmarks/data");
    let (prs, _labels) = load_dataset(data_dir.join("prs.jsonl"), data_dir.join("labels.json"))
        .expect("load dataset");
    let client = MockNimClient::new(&prs);

    let sample: Vec<_> = prs.iter().take(SAMPLE_SIZE).collect();
    println!(
        "Token cost per verdict ({} PRs, ${:.3}/1K tokens placeholder)",
        sample.len(),
        USD_PER_1K_TOKENS
    );
    println!(
        "  {id:<8} {title:<38} {prompt:>6} {compl:>6} {total:>6} {usd:>12}",
        id = "id",
        title = "title",
        prompt = "prompt",
        compl = "compl",
        total = "total",
        usd = "USD",
    );

    let mut total_prompt = 0u32;
    let mut total_compl = 0u32;
    let mut total_total = 0u32;
    let mut total_usd = 0.0f64;

    // Time the whole accounting loop to assert the bench itself
    // runs in well under a second (sanity check on the synchronous
    // tight loop).
    let bench_start = Instant::now();
    for pr in &sample {
        let usage = client.estimate_tokens_sync(SYSTEM_PROMPT, &user_msg(&pr.diff));
        let title_short: String = if pr.title.len() > 36 {
            format!("{}…", &pr.title[..35])
        } else {
            pr.title.clone()
        };
        let usd = usd(&usage);
        println!(
            "  {:<8} {:<38} {:>6} {:>6} {:>6} {:>12}",
            pr.id,
            title_short,
            fmt_n(usage.prompt_tokens),
            fmt_n(usage.completion_tokens),
            fmt_n(usage.total_tokens),
            fmt_usd(usd),
        );
        total_prompt += usage.prompt_tokens;
        total_compl += usage.completion_tokens;
        total_total += usage.total_tokens;
        total_usd += usd;
    }
    let bench_elapsed = bench_start.elapsed();

    println!();
    println!("=== Aggregate over the 10 sampled PRs ===");
    println!("  prompt_tokens     : {total_prompt}");
    println!("  completion_tokens : {total_compl}");
    println!("  total_tokens      : {total_total}");
    println!("  estimated USD     : ${:.6}", total_usd);
    println!(
        "  bench loop wall   : {:?} (sanity check; should be sub-second)",
        bench_elapsed
    );

    // Optional: warn if the bench itself took too long, which would
    // indicate a hot-path regression. We give a generous 5s budget.
    if bench_elapsed > Duration::from_secs(5) {
        eprintln!("WARN: cost bench took {:?}; expected < 5s", bench_elapsed);
    }
}
