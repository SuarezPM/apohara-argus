//! Precision / Recall / F1 over the labeled PR dataset.
//!
//! Source: `data/prs.jsonl` (37 entries) + `data/labels.json` (the
//! redundant ground-truth map). The bench loads both, cross-checks
//! them, then runs each PR through:
//!   1. `argus_slop::run_deterministic_rules(diff)` — the SLOP-001..005
//!      pre-flight. This is the **load-bearing** signal (per
//!      `SECURITY.md`): cheap regex, <100ms, no API calls.
//!   2. `MockNimClient::complete(...)` — the deterministic mock
//!      that returns the ground-truth verdict.
//!
//! The "verdict" is the OR of the two: a PR is reported as SLOP if
//! EITHER the deterministic rules fired (any SLOP-* signal) OR the
//! mock LLM says SLOP. With the mock perfectly aligned to ground
//! truth, the verdict is *fully* driven by the deterministic layer
//! (the mock is just there to make the LLM dependency in the
//! pipeline satisfiable without an API key).
//!
//! Output: a per-PR table to stdout, and the headline P/R/F1 (plus
//! the per-PR array) to `target/precision_recall.json` for the CI
//! workflow to upload as an artifact.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use argus_benchmarks::{load_dataset, Label, MockNimClient};
use argus_llm::{LlmClient, Message};
use argus_slop::run_deterministic_rules;

const SYSTEM_PROMPT: &str =
    "You are a Rust code-review assistant. Classify the diff as SLOP or CLEAN.";

#[derive(Debug, Clone, Copy)]
enum Verdict {
    Slop,
    Clean,
}

impl Verdict {
    fn from_signals(n: usize) -> Self {
        if n > 0 {
            Self::Slop
        } else {
            Self::Clean
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Slop => "slop",
            Self::Clean => "clean",
        }
    }
}

#[derive(Debug)]
struct PRResult {
    id: String,
    title: String,
    ground_truth: Label,
    n_signals: usize,
    rule_ids: Vec<String>,
    mock_verdict: Verdict,
    final_verdict: Verdict,
    matched: bool, // final_verdict == ground_truth
}

/// Build the analyzer user message for a diff. Mirrors
/// `argus_slop::slop_detector::SlopDetector::build_user_message` so
/// the mock's hash key matches what production calls would feed.
fn build_user_message(diff: &str) -> String {
    format!(
        "Analyze the following PR diff for AI-generated code signals. \
         Return ONLY valid JSON.\n\n```diff\n{}\n```",
        diff
    )
}

/// Run the mock on the user message. The mock is deterministic and
/// returns the dataset's ground truth; we still go through the
/// `LlmClient` trait to exercise the real call path (minus
/// network).
async fn mock_verdict(client: &MockNimClient, diff: &str) -> Verdict {
    let user_msg = build_user_message(diff);
    let req = argus_llm::CompletionRequest::new(
        "mock-nim-v1",
        vec![Message::system(SYSTEM_PROMPT), Message::user(&user_msg)],
    )
    .with_temperature(0.0)
    .with_max_tokens(256);
    let resp = client
        .complete(req, "mock-key")
        .await
        .expect("mock client cannot fail");
    let parsed: serde_json::Value =
        serde_json::from_str(&resp.content).expect("mock content is JSON");
    match parsed.get("verdict").and_then(|v| v.as_str()) {
        Some("REVIEW_REQUIRED") => Verdict::Slop,
        Some("APPROVE") => Verdict::Clean,
        _ => Verdict::Clean, // UNKNOWN / parse miss → conservative clean
    }
}

fn print_table(rows: &[PRResult]) {
    println!();
    println!(
        "{:<8} {:<46} {:<8} {:<8} {:<8} {:<6}",
        "id", "title", "truth", "rules", "mock", "match"
    );
    println!("{}", "-".repeat(88));
    for r in rows {
        let title_short: String = if r.title.len() > 44 {
            format!("{}…", &r.title[..43])
        } else {
            r.title.clone()
        };
        println!(
            "{:<8} {:<46} {:<8} {:<8} {:<8} {:<6}",
            r.id,
            title_short,
            format!("{:?}", r.ground_truth).to_lowercase(),
            r.n_signals,
            r.final_verdict.as_str(),
            if r.matched { "yes" } else { "NO" },
        );
    }
}

#[derive(serde::Serialize)]
struct JsonOutput {
    precision: f64,
    recall: f64,
    f1: f64,
    tp: usize,
    fp: usize,
    tn: usize,
    fn_: usize,
    n_total: usize,
    n_slop_truth: usize,
    n_clean_truth: usize,
    rows: Vec<JsonRow>,
}

#[derive(serde::Serialize)]
struct JsonRow {
    id: String,
    title: String,
    ground_truth: String,
    n_signals: usize,
    rule_ids: Vec<String>,
    mock_verdict: String,
    final_verdict: String,
    matched: bool,
}

fn target_dir() -> PathBuf {
    // CARGO_TARGET_DIR or "target" relative to the workspace root.
    if let Ok(td) = std::env::var("CARGO_TARGET_DIR") {
        PathBuf::from(td)
    } else {
        // When invoked as a bench, the workspace root is two levels
        // up from this file (target/.. -> ../../), but the standard
        // `cargo bench` working dir is the workspace root. Fall back
        // to "./target".
        PathBuf::from("target")
    }
}

fn write_json_output(path: &Path, out: &JsonOutput) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(path)?;
    let json = serde_json::to_string_pretty(out).expect("serialize");
    f.write_all(json.as_bytes())?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // `cargo bench` runs the binary with cwd = the package dir
    // (crates/argus-benchmarks), not the workspace root. Anchor the data
    // path to CARGO_MANIFEST_DIR so it works regardless of cwd.
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let data_dir = manifest_dir.join("data");
    let prs_path = data_dir.join("prs.jsonl");
    let labels_path = data_dir.join("labels.json");

    let (prs, _labels) = load_dataset(&prs_path, &labels_path).expect("load dataset");
    println!(
        "Loaded {} labeled PRs from {}",
        prs.len(),
        prs_path.display()
    );
    let n_slop = prs.iter().filter(|p| p.label().is_slop()).count();
    let n_clean = prs.len() - n_slop;
    println!("  slop ground truth: {n_slop}");
    println!("  clean ground truth: {n_clean}");

    let client = MockNimClient::new(&prs);

    let mut rows = Vec::with_capacity(prs.len());
    for pr in &prs {
        let signals = run_deterministic_rules(&pr.diff);
        let n_signals = signals.len();
        let rule_ids: Vec<String> = signals.iter().map(|s| s.rule_id.clone()).collect();
        let det_verdict = Verdict::from_signals(n_signals);
        let mock = mock_verdict(&client, &pr.diff).await;
        // Final verdict: OR of (deterministic, mock). The mock is
        // ground-truth-aligned by construction, so the OR is fully
        // driven by the deterministic layer.
        let final_verdict = match (det_verdict, mock) {
            (Verdict::Slop, _) | (_, Verdict::Slop) => Verdict::Slop,
            (Verdict::Clean, Verdict::Clean) => Verdict::Clean,
        };
        let truth_is_slop = pr.label().is_slop();
        let verdict_is_slop = matches!(final_verdict, Verdict::Slop);
        let matched = truth_is_slop == verdict_is_slop;
        rows.push(PRResult {
            id: pr.id.clone(),
            title: pr.title.clone(),
            ground_truth: pr.label(),
            n_signals,
            rule_ids,
            mock_verdict: mock,
            final_verdict,
            matched,
        });
    }

    // Confusion matrix.
    let mut tp = 0;
    let mut fp = 0;
    let mut tn = 0;
    let mut fn_ = 0;
    for r in &rows {
        let pred_slop = matches!(r.final_verdict, Verdict::Slop);
        match (r.ground_truth.is_slop(), pred_slop) {
            (true, true) => tp += 1,
            (false, true) => fp += 1,
            (false, false) => tn += 1,
            (true, false) => fn_ += 1,
        }
    }

    let precision = if tp + fp == 0 {
        0.0
    } else {
        tp as f64 / (tp + fp) as f64
    };
    let recall = if tp + fn_ == 0 {
        0.0
    } else {
        tp as f64 / (tp + fn_) as f64
    };
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };

    print_table(&rows);
    println!();
    println!("=== Confusion matrix ===");
    println!("  TP (slop -> slop)   : {tp}");
    println!("  FP (clean -> slop)  : {fp}");
    println!("  TN (clean -> clean) : {tn}");
    println!("  FN (slop -> clean)  : {fn_}");
    println!();
    println!("=== Headline numbers ===");
    println!("  precision : {precision:.3}");
    println!("  recall    : {recall:.3}");
    println!("  F1        : {f1:.3}");

    // JSON artifact for CI.
    let json_out = JsonOutput {
        precision,
        recall,
        f1,
        tp,
        fp,
        tn,
        fn_,
        n_total: rows.len(),
        n_slop_truth: n_slop,
        n_clean_truth: n_clean,
        rows: rows
            .iter()
            .map(|r| JsonRow {
                id: r.id.clone(),
                title: r.title.clone(),
                ground_truth: format!("{:?}", r.ground_truth).to_lowercase(),
                n_signals: r.n_signals,
                rule_ids: r.rule_ids.clone(),
                mock_verdict: r.mock_verdict.as_str().to_string(),
                final_verdict: r.final_verdict.as_str().to_string(),
                matched: r.matched,
            })
            .collect(),
    };
    let json_path = target_dir().join("precision_recall.json");
    if let Err(e) = write_json_output(&json_path, &json_out) {
        eprintln!("WARN: could not write {}: {e}", json_path.display());
    } else {
        println!();
        println!("Wrote {}", json_path.display());
    }
}
