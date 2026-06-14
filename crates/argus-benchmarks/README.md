# argus-benchmarks

The 15th crate in the ARGUS workspace. Internal-only (`publish = false`).
Holds:

- `data/prs.jsonl` — the labeled PR dataset (≥30 entries, mix of
  hand-crafted slop + common LLM-suggested patterns from open-source
  Rust projects).
- `data/labels.json` — the ground-truth map for the dataset.
- `src/mock_nim.rs` — a deterministic mock LLM client that
  implements `argus_llm::LlmClient`. Returns ground-truth labels for
  the labeled dataset, so the precision/recall bench can run without
  an API key and without network.
- `benches/precision_recall.rs` — P/R/F1 over the dataset.
- `benches/latency.rs` — end-to-end diff -> verdict p50/p99.
- `benches/cost.rs` — tokens + estimated USD per verdict.

All 3 benches are `harness = false` (run as plain binaries under both
`cargo test` and `cargo bench`). See `../../docs/BENCHMARK.md` for the
published numbers and methodology.
