//! argus-benchmarks — labeled dataset + mock NIM client + 3 benches.
//!
//! This crate is **CI-internal only** (`publish = false`). It exists to
//! give ARGUS honest precision/recall, latency, and cost numbers
//! without requiring an API key. The headline numbers are published
//! in `../../docs/BENCHMARK.md`.
//!
//! See the module docs:
//! - [`mock_nim`] — deterministic mock `LlmClient` for the dataset.
//! - [`dataset`] — JSONL loader for `data/prs.jsonl`.

pub mod dataset;
pub mod mock_nim;

pub use dataset::{load_dataset, load_labels, Label, LabeledPR};
pub use mock_nim::MockNimClient;
