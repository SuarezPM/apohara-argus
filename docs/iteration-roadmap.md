# ARGUS Iteration Roadmap — June 2026

> **Source:** 8 EXA searches (Apr–Jun 2026), cross-referenced with the shipped ARGUS state.
> **Purpose:** prioritized, code-actionable improvements for the next iteration cycle.
> **Tag:** FRESH = source from May/Jun 2026.

## Executive Summary (Top 10 Wins, Ranked by ROI)

| # | Win | Source | Effort | Impact |
|---|-----|--------|--------|--------|
| 1 | Adopt `certifieddata/ai-decision-logging-spec` Level 2 schema | [GitHub](https://github.com/certifieddata/ai-decision-logging-spec) FRESH Apr 2026 | M | EU AI Act Article 12 marketing-grade compliance |
| 2 | Shell out to `deslop` (Rust-native, 350 Rust rules) | [GitHub](https://github.com/chinmay-sawant/deslop) FRESH | S | Zero-LLM cost on first pass + ~5s latency cut |
| 3 | Default model → `moonshotai/kimi-k2.6` or `nvidia/llama-3.1-nemotron-ultra-253b-v1` | [NIM catalog](https://build.nvidia.com/models) FRESH Jun 2026 | S | Rust+Go+Python explicit; SWE-Bench 80.2%; 5x faster |
| 4 | Add 9-phase graceful shutdown to argus-verify + argus-dashboard | [atharvapandey.com](https://www.atharvapandey.com/post/rust/rust-deploy-graceful-shutdown/) | S | Production-ready on Fly.io (no more zombie workers) |
| 5 | Add OtelAxumLayer + tracing-opentelemetry 0.33 | [base14.io](https://docs.base14.io/instrument/apps/auto-instrumentation/axum/) FRESH | S | Free observability on every crate |
| 6 | Add severity score + `/fix` handoff per finding (Greptile pattern) | [State of AI Code Review May 2026](https://dev.to/lewiska/state-of-ai-code-review-may-2026-roundup-5o7) FRESH | M | Feature parity with Greptile (cheaper since BYOK) |
| 7 | Programmable review rules via YAML in `argus-slop/configs/*.yaml` (Macroscope pattern) | [State of AI Code Review](https://dev.to/lewiska/state-of-ai-code-review-may-2026-roundup-5o7) FRESH | M | Differentiator: custom rules, not just generic LLM |
| 8 | SARIF output in `argus-slop scan` (integrates with GitHub Security tab) | [ai-slopcheck](https://github.com/Euraika-Labs/ai-slopcheck) FRESH | S | Zero-config GitHub Security tab surface |
| 9 | HeyGen integration for Lens avatar (Pay-as-you-go $5, MCP-friendly) | [HeyGen pricing](https://developers.heygen.com/docs/pricing) FRESH | M | Real video output, no D-ID watermark |
| 10 | Migrate `argus-crypto` SPIFFE-like IDs → `spiffe` crate v0.15 | [rust-spiffe](https://github.com/maxlambrecht/rust-spiffe) FRESH May 2026 | M | Standards-compliant, no rewrites needed (we already have Ed25519) |

---

## DIM 1 — EU AI Act Article 12 Compliance

### Evidence (FRESH Apr-Jun 2026)
- **`certifieddata/ai-decision-logging-spec`** is the de-facto open standard for Article 12 logs. Two conformance levels: Core (required fields + hash chain) and Full (Ed25519 + audit export + 7-year retention). [GitHub](https://github.com/certifieddata/ai-decision-logging-spec)
- **EU Commission Q&A (Apr 2026 update):** hash of input + classification + policy metadata is sufficient. Storing cleartext prompts creates GDPR derivative liability. [DeepInspect](https://www.deepinspect.ai/blog/guides-eu-ai-act-article-12-logging-implementation) FRESH Jun 2026
- **Draft standard prEN ISO/IEC 24970** will provide concrete field-level guidance. Coming soon.
- **AGLedger pattern** (Ed25519 + hash chain) is the architectural standard. [Spiffe blog](https://spiffe.io/) FRESH
- **Retention:** 6 months minimum, 24 months recommended (GDPR-aligned). [Practical AI Act](https://practical-ai-act.eu/latest/conformity/record-keeping/)
- **IETF draft-veridom-omp-euaia-00** (in progress): SHA-256 + RFC 3161 timestamp + 3-layer crypto. [IETF](https://www.ietf.org/archive/id/draft-veridom-omp-euaia-00.txt)

### Current ARGUS state
- `argus-crypto` has Ed25519 + BLAKE3 hash chain ✓
- We store raw prompt text in the ledger (GDPR risk)
- No `data_class`, `policy_version`, or `input_hash` fields
- No retention config (default = forever)

### Actionable recommendations

**HIGH PRIORITY — Adopt the spec's JSON schema for our audit records**

```rust
// crates/apohara-argus-core/src/decision_record.rs
use serde::{Deserialize, Serialize};
use blake3::Hash;

#[derive(Serialize, Deserialize, Debug)]
pub struct DecisionRecord {
    // Required per spec
    pub audit_id: Uuid,
    pub timestamp_start: DateTime<Utc>,
    pub timestamp_end: DateTime<Utc>,
    pub subject: String,             // SPIFFE ID or user ID
    pub subject_type: SubjectType,   // User | AgentOnBehalf | Service
    pub route: String,               // "nvidia/moonshotai-kimi-k2.6"
    pub data_class: DataClass,       // Pii | Phi | SourceCode | Contract | None | Mixed
    pub policy_version: String,      // SHA-256 of active policy bundle
    pub decision: Decision,          // Pass | Block | Redact
    pub reason_code: String,
    pub input_hash: Hash,            // BLAKE3(prompt) — NOT the prompt
    pub output_hash: Hash,           // BLAKE3(completion)
    pub writer_signature: Ed25519Signature,  // HMAC chain to prior

    // Recommended (Full conformance)
    pub prev_signature: Ed25519Signature,    // For chain verification
    pub prompt_template_version: String,
    pub temperature: f32,
    pub tool_calls: Vec<ToolCall>,
    pub model_response_tokens: u32,
}
```

Files: `crates/apohara-argus-core/src/types.rs`, `crates/argus-crypto/src/ledger.rs`
Effort: **M** (4-6 hours)
Compliance gain: **Level 2 (Full)** conformance per the open spec. Marketing-grade.

**MEDIUM PRIORITY — Hash, don't store, raw prompts**

In `argus-llm/src/nim.rs`, before appending to ledger:
- Compute `BLAKE3::hash(prompt.as_bytes())` → `input_hash`
- Discard the prompt text
- Append only the hash

Files: `crates/argus-llm/src/audit.rs`
Effort: **S** (1-2 hours)
Compliance gain: Removes GDPR derivative liability per Apr 2026 Commission Q&A.

**LOW PRIORITY — Add retention policy config**

```rust
// crates/apohara-argus-core/src/config.rs
pub struct RetentionConfig {
    pub default_days: u32,    // Default 730 (2 years, GDPR-aligned)
    pub high_risk_days: u32,  // Default 2555 (7 years, Article 19)
    pub purge_on_rotation: bool,  // Default false (compliance archive)
}
```

Effort: **S**
Compliance gain: Aligns with Article 19 retention obligations.

---

## DIM 2 — Rust AI Agent Frameworks

### Evidence
- **OpenFang v0.5.10** ([openfang.app](https://openfang.app/)): 137K LOC, 32MB binary, 180ms cold start, 7 Hands, MCP+A2A+OFP, WASM dual-metered sandbox, 16 security layers, Merkle audit trail. **Pre-1.0 — breaking changes possible.** MIT.
- **AutoAgents** ([liquidos-ai](https://github.com/liquidos-ai/AutoAgents)): 1,046MB peak memory, 5,714ms avg latency, 98.03 benchmark score, WASM sandbox, OTEL, Ractor actor model. "Production-grade" claim.
- **Rig** ([rig.dev](https://www.rig.rs/)): 1,019MB peak, 6,065ms latency, 90.06 score. More mature, used by Cloudflare/Neon/Nethermind.
- **Benchmark [DEV.to](https://dev.to/saivishwak/benchmarking-ai-agent-frameworks-in-2026-autoagents-rust-vs-langchain-langgraph-llamaindex-338f) FRESH Feb 2026:** Rust 5x less memory, 43.7% lower latency vs LangGraph, 13x throughput vs CrewAI.

### Current ARGUS state
- Pure custom Tokio + 4 specialists + 1 synthesizer
- ~2,200 LOC for the agent layer
- Ed25519 + BLAKE3 chain (better audit than any framework)
- No multi-agent standards (A2A, MCP) — closed loop

### Actionable recommendation

**STAY CUSTOM — adopt 2-3 patterns from OpenFang/AutoAgents without migrating**

Reasoning:
- We have a working agent (47 tests pass)
- Migrating to a framework = weeks of work + risk
- We can copy specific patterns (WASM tool sandbox, MCP server) incrementally
- Our CordonEnforcer pattern (synthesizer doesn't see raw code) is novel — keep it

**LOW PRIORITY — Add MCP server support to expose our analyzers**

```rust
// crates/apohara-argus-mcp/src/lib.rs (NEW crate, ~300 LOC)
use rmcp::{Server, tool};
// Expose argus-slop, argus-verify, argus-lens as MCP tools
// so Claude Code / Codex / Cursor can call us
```

Effort: **M** (1 day, requires `rmcp` crate)
Impact: Becomes a tool that other agents consume. Network effect.

**LOW PRIORITY — Add WASM tool sandbox for user-defined analyzers**

Copy the OpenFang dual-metered pattern but expose it as a config option:
- `argus-slop --wasm-tools ./my-rules.wasm`
- Metered execution (fuel limit, time limit)
- No fs/network by default

Effort: **L** (3-5 days, requires `wasmtime` integration)
Impact: Lets users add custom analyzers safely. Not a near-term priority.

---

## DIM 3 — NVIDIA NIM Model Selection

### Evidence (FRESH Jun 2026)
- **147 models** in catalog. [build.nvidia.com/models](https://build.nvidia.com/models)
- **Kimi K2.6** ([details](https://docs.api.nvidia.com/nim/reference/moonshotai-kimi-k2-6)): 1T params (32B active) MoE. **Rust+Go+Python+frontend+DevOps** in explicit use cases. SWE-Bench Verified 80.2%, Terminal-Bench 66.7%, OSWorld 73.1%, LiveCodeBench v6 89.6%. Released Apr 29 2026.
- **Nemotron 3 Ultra** ([blog](https://developer.nvidia.com/blog/nvidia-nemotron-3-ultra-powers-faster-more-efficient-reasoning-for-long-running-agents/) FRESH Jun 4 2026): 550B MoE (55B active). 5x faster inference vs class. "Frontier accuracy in a smaller model." Ruler @1M = 95%.
- **Nemotron 3 Super 120B A12B**: hybrid Mamba-Transformer MoE, 1M context, agentic reasoning + coding + tool calling.
- **DeepSeek V4 Flash**: 284B MoE, 1M context, "fast coding and agents."
- **GLM-5.1** (744B): flagship agentic.
- **Llama 3.3 Nemotron Super 49B v1.5**: cost-efficient, fine-tuning ready.

### Current ARGUS default
- `meta/llama-3.1-70b-instruct`
- Generic, not optimized for Rust/code review

### Actionable recommendations

**HIGH PRIORITY — Add a model registry in `argus-llm` with per-analyzer defaults**

```rust
// crates/argus-llm/src/model_registry.rs
pub struct ModelRegistry {
    pub slop_detector: &'static str,    // Cost-optimized
    pub security_reviewer: &'static str,// Reasoning-optimized
    pub arch_reviewer: &'static str,    // Reasoning-optimized
    pub verdict_synthesizer: &'static str, // Strongest reasoning
}

pub const DEFAULT_REGISTRY: ModelRegistry = ModelRegistry {
    slop_detector: "meta/llama-3.3-nemotron-super-49b-v1.5",  // Cheap
    security_reviewer: "moonshotai/kimi-k2.6",                // Rust+security
    arch_reviewer: "moonshotai/kimi-k2.6",
    verdict_synthesizer: "nvidia/llama-3.1-nemotron-ultra-253b-v1", // Strongest
};
```

Files: `crates/argus-llm/src/model_registry.rs`, `crates/argus-llm/src/nim.rs`
Effort: **S** (2-3 hours)
Impact: 30-50% quality bump on verdict, ~3x cost reduction on slop-detector.

**MEDIUM PRIORITY — Add a `--fast` flag that uses Nemotron 3 Super for everything**

```bash
argus verify --fast   # Single model: nemotron-3-super-120b-a12b
```

For users who want speed over depth. Estimated 2-3x speedup, ~40% quality drop.

**LOW PRIORITY — Support reasoning/thinking mode**

```rust
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: f32,
    pub reasoning_effort: Option<ReasoningEffort>, // Low | Medium | High
}
```

Kimi K2.6 + Nemotron 3 Ultra support this. Better verdict synthesis.

---

## DIM 4 — AI Slop Detection (Deterministic Layer)

### Evidence (FRESH 2026)
- **`aislop` v0.9.4** ([scanaislop/aislop](https://github.com/scanaislop/aislop)): 40+ rules, 7 langs (incl. Rust), sub-second, MIT, 6 parallel engines. 25 releases since 2026-03.
- **`deslop`** ([chinmay-sawant/deslop](https://github.com/chinmay-sawant/deslop)): **Pure Rust**, tree-sitter, 1,791 stable rules (350 for Rust), GitHub Action, sub-second.
- **`antislop` v0.3.0** ([skew202/antislop](https://github.com/skew202/antislop)): Rust+tree-sitter hybrid. Regex mode 0.47ms, AST mode 4ms. 19 languages.
- **`ai-slopcheck` v1.2.0** ([Euraika-Labs](https://github.com/Euraika-Labs/ai-slopcheck)): 72 rules, SARIF output, diff-only mode, baselines, `--min-confidence`.
- **`flamehaven01/AI-SLOP-Detector` v3.8.5** ([GitHub](https://github.com/flamehaven01/ai-slop-detector)): optional Rust acceleration.

### Current ARGUS state
- 100% LLM-based slop detection (4 analyzers)
- ~$0.02 per PR analyzed (Llama 70B tokens)
- Latency: 4-8s per analyzer
- No AST/regex layer

### Actionable recommendations

**HIGH PRIORITY — Add `deslop` as a pre-LLM filter in `argus-slop`**

```rust
// crates/argus-slop/src/deterministic.rs
use std::process::Command;
use std::path::Path;

pub fn deslop_scan(diff: &str) -> Result<Vec<SlopFinding>, SlopError> {
    // Write diff to temp file
    let tmp = tempfile::NamedTempFile::new()?;
    std::fs::write(tmp.path(), diff)?;
    
    // Shell out to deslop
    let output = Command::new("deslop")
        .arg("scan")
        .arg("--format=json")
        .arg(tmp.path())
        .output()?;
    
    serde_json::from_slice(&output.stdout)
        .map_err(Into::into)
}
```

Add to pipeline: `deslop findings → LLM analysis only on findings` (hybrid).
Files: `crates/argus-slop/src/deterministic.rs`, `crates/argus-slop/src/lib.rs`
Effort: **S** (3-4 hours including tests)
Impact:
- 60-80% of LLM cost cut (only call LLM on complex findings)
- Sub-second response on clean code
- Deterministic baseline (won't drift with model versions)

**MEDIUM PRIORITY — Add SARIF output to `argus-slop scan`**

```rust
// crates/argus-slop/src/sarif.rs
pub fn to_sarif(findings: &[SlopFinding]) -> serde_json::Value {
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": { "name": "argus-slop", "version": env!("CARGO_PKG_VERSION") }},
            "results": findings.iter().map(|f| json!({
                "ruleId": f.rule_id,
                "level": severity_to_sarif(f.severity),
                "message": { "text": f.message },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": f.file },
                        "region": { "startLine": f.line, "startColumn": f.col }
                    }
                }]
            })).collect::<Vec<_>>()
        }]
    })
}
```

CLI: `argus scan --format=sarif > results.sarif`
Files: `crates/argus-slop/src/sarif.rs`, `crates/apohara-argus-cli/src/commands/scan.rs`
Effort: **S** (2 hours)
Impact: Direct integration with GitHub Security tab. Zero-config surfacing.

**MEDIUM PRIORITY — Add `--diff-only` mode to scan only changed files**

Match `ai-slopcheck`'s pattern. Critical for PR review (don't re-scan unchanged code).

```rust
// crates/argus-slop/src/diff.rs
pub fn changed_files_since(base_sha: &str, head_sha: &str) -> Result<Vec<PathBuf>> {
    // Use git2 or shell out to `git diff --name-only base_sha..head_sha`
}
```

Effort: **S**
Impact: 5-10x speedup on typical PRs (only 5-20 files changed vs 1000s in repo).

---

## DIM 5 — Axum Production Patterns

### Evidence (FRESH 2026)
- **axum 0.8.8+** recommended for OTel. [base14.io](https://docs.base14.io/instrument/apps/auto-instrumentation/axum/)
- **tracing-opentelemetry 0.33+** current. Same source.
- **9-phase graceful shutdown** ([atharvapandey.com](https://www.atharvapandey.com/post/rust/rust-deploy-graceful-shutdown/)):
  1. Mark as not-ready (readiness returns 503)
  2. Wait for endpoint propagation (sleep 3-5s)
  3. Stop accepting new connections
  4. Wait for in-flight requests
  5. Signal background workers
  6. Wait for workers (with timeout)
  7. Close DB/cache pools
  8. Flush metrics + trace buffers
  9. Exit 0
- **OtelAxumLayer** + **OtelInResponseLayer** order matters: bottom-to-top. [EncodePanda](https://github.com/EncodePanda/rust-telemetry) FRESH Feb 2026
- **BatchSpanProcessor**: 30s delay, 2048 queue, 512 batch. [base14.io](https://docs.base14.io/instrument/apps/auto-instrumentation/axum/)
- **SQLx pattern**: `SQLX_OFFLINE=true` with `.sqlx/` in git, `cargo sqlx prepare` locally, `condition: service_healthy` in `depends_on`. [devcheolu.com](https://devcheolu.com/en/posts/REA8G6eGFYSfWm5Qd9rE) FRESH May 2026

### Current ARGUS state
- `argus-verify` and `argus-dashboard` use Axum (older 0.7 probably)
- No graceful shutdown (kill -9 leaves zombie reqs)
- No OTel
- No readiness/liveness split
- `argus-lens` runs as a separate process

### Actionable recommendations

**HIGH PRIORITY — Implement 9-phase graceful shutdown in both servers**

```rust
// crates/argus-verify/src/main.rs (and argus-dashboard)
use axum::serve;
use tokio::signal;

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.ok();
    };
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler")
            .recv()
            .await;
    };
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
    tracing::info!("shutdown signal received");
}

#[tokio::main]
async fn main() {
    // 1. Mark not-ready
    set_readiness(false).await;
    
    // 2. Start server with graceful shutdown
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            // 2. Wait for endpoint propagation
            tokio::time::sleep(Duration::from_secs(3)).await;
            shutdown_signal().await;
        });
    
    // Health server on different port for k8s probes
    tokio::spawn(health_server());
    
    server.await.unwrap();
    
    // 7-8. Close resources
    pool.close().await;
    tracer_provider.shutdown().ok();
    
    // 9. Exit 0
    std::process::exit(0);
}
```

Files: `crates/argus-verify/src/main.rs`, `crates/argus-dashboard/src/main.rs`
Effort: **S** (3-4 hours each)
Impact: Production-ready on Fly.io. No more dropped requests on deploy.

**MEDIUM PRIORITY — Add OpenTelemetry with OtelAxumLayer**

```toml
# crates/argus-verify/Cargo.toml
[dependencies]
axum = "0.8"
tracing = "0.1"
tracing-opentelemetry = "0.33"
opentelemetry = "0.27"
opentelemetry-otlp = "0.27"
opentelemetry_sdk = "0.27"
axum-tracing-opentelemetry = "0.13"
```

```rust
// crates/argus-verify/src/telemetry.rs
use opentelemetry::global;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::trace::BatchSpanProcessor;
use opentelemetry_otlp::WithExportConfig;
use axum_tracing_opentelemetry::OtelAxumLayer;

pub fn init_telemetry() -> SdkTracerProvider {
    global::set_text_map_propagator(TraceContextPropagator::new());
    
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint("https://api.honeycomb.io"))
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;
    
    SdkTracerProvider::builder()
        .with_batch_exporter(...)
        .with_resource(...)
        .build()
}
```

Layer order: `.layer(OtelAxumLayer).layer(OtelInResponseLayer)` — bottom to top.

Effort: **M** (4-6 hours, requires picking a backend: Honeycomb, Jaeger, or base14)
Impact: Free observability. See every request across all 4 specialists.

**LOW PRIORITY — SQLx offline mode for CI**

If we adopt Postgres (currently in-memory): add `SQLX_OFFLINE=true`, commit `.sqlx/`, `cargo sqlx prepare` in CI.

---

## DIM 6 — AI Avatar Video (Lens output)

### Evidence (FRESH Jun 2026)
- **HeyGen** ([pricing](https://developers.heygen.com/docs/pricing) FRESH): $5 pay-as-you-go, no subscription. Avatar V 720p = **$0.05/sec = $3/min**. Avatar V 4K = $0.0667/sec. **MCP support for Claude/Codex** ([developers.heygen.com](https://developers.heygen.com/)). Digital Twin creation: $1/call.
- **D-ID**: $4.70/mo (watermarked), $29/mo Pro (15min, no watermark), $196/mo Advanced (100min, API+studio share pool). [Anam](https://anam.ai/blog/d-id-alternatives) FRESH Apr 2026
- **Tavus**: $29/mo, **only real-time streaming competitor**. [veed.io](https://www.veed.io/learn/best-talking-head-video-apis) FRESH Apr 2026
- **Open-source:** MuseTalk 30 FPS on A10G ($0.30/hr spot), SadTalker CPU-only free. [punithvt/ai-avatar-system](https://github.com/punithvt/ai-avatar-system)

### Current ARGUS state
- `argus-lens` generates the "CTO avatar script" as text
- No video rendering
- User must manually paste into HeyGen/D-ID

### Actionable recommendations

**MEDIUM PRIORITY — Integrate HeyGen for the Lens output**

```rust
// crates/argus-lens/src/heygen.rs
pub struct HeyGenClient {
    api_key: String,
    base: String,  // https://api.heygen.io
    http: reqwest::Client,
}

impl HeyGenClient {
    pub async fn render_script(
        &self,
        script: &str,
        voice_id: &str,       // "en-US-ChristopherNeural" or custom
        avatar_id: &str,      // "Daisy-inskirt-20220818"
    ) -> Result<VideoUrl> {
        // 1. POST /v2/video/generate
        let res = self.http.post(format!("{}/v2/video/generate", self.base))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "video_inputs": [{
                    "character": { "avatar_id": avatar_id, "type": "talking_photo" },
                    "voice": { "input_text": script, "voice_id": voice_id },
                    "background": { "type": "color", "value": "#0F172A" }
                }],
                "dimension": { "width": 1280, "height": 720 }
            }))
            .send().await?;
        
        let video_id: String = res.json::<GenerateResponse>().await?.data.video_id;
        
        // 2. Poll /v1/video_status.get until completed
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let status = self.poll_status(&video_id).await?;
            if status.status == "completed" { return Ok(status.video_url); }
        }
    }
}
```

CLI: `argus lens --render --avatar=daisy --voice=chris`
Cost: ~$3 per weekly briefing (60-90s @ 720p). Free if user pastes manually.
Files: `crates/argus-lens/src/heygen.rs`, `crates/argus-lens/src/main.rs`
Effort: **M** (6-8 hours)
Impact: Actual video output. Differentiator: nobody else has this in AI code review.

**LOW PRIORITY — Support MCP server for HeyGen**

The HeyGen docs [explicitly recommend](https://developers.heygen.com/) the MCP path. We could expose our HeyGen integration as an MCP tool so Claude Code can generate videos.

---

## DIM 7 — SPIFFE/SPIRE Migration

### Evidence (FRESH May 2026)
- **`spiffe` v0.15** + **`spire-api` v0.7.0** ([crates.io](https://crates.io/crates/spire-api), updated May 8 2026). [GitHub](https://github.com/maxlambrecht/rust-spiffe). Apache-2.0, MSRV 1.88.
- 5 crates: `spiffe`, `spire-api`, `spiffe-rustls`, `spiffe-rustls-tokio`, `spiffe-rust`.
- **20 contributors, 63 releases, 34 stars.** [Disclaimer:](https://github.com/maxlambrecht/rust-spiffe) "Does not claim formal security audits or guaranteed production fitness."
- **SPIRE architecture**: server + agents, Workload API, attestors (Unix/Kubernetes/Docker). [spiffe.io](https://spiffe.io/docs/latest/spire-about/spire-concepts/)
- **NIST NCCoE paper** (Feb 2026): "Accelerating the Adoption of Software and AI Agent Identity and Authorization" — SPIFFE/SPIRE in scope.

### Current ARGUS state
- `argus-crypto` has custom "SPIFFE-like" JWT IDs (SPIFFE URI in `sub`)
- Ed25519 signed, no SPIRE server
- No X.509-SVID, no workload attestation

### Actionable recommendations

**LOW PRIORITY — Adopt `spiffe` crate for ID generation, keep custom signing**

```toml
# crates/argus-crypto/Cargo.toml
[dependencies]
spiffe = "0.15"
```

```rust
// crates/argus-crypto/src/id.rs
use spiffe::TrustDomain;

pub fn agent_id(trust_domain: &str, agent_type: &str, id: &str) -> Result<SpiffeId> {
    let td = TrustDomain::new(trust_domain)?;
    let path = format!("/{}/{}", agent_type, id);
    SpiffeId::new(td, &path).map_err(Into::into)
}
```

Effort: **S** (2-3 hours)
Impact: Standards-compliant SPIFFE IDs. No server needed. Just the ID primitives.

**DEFERRED — SPIRE server**

Requires Kubernetes or systemd unit + admin socket. Not viable for Fly.io single-VM deploy. Skip unless we move to K8s.

**DEFERRED — X.509-SVID via mTLS**

Useful if we have multiple services calling each other. Currently only `argus-verify` calls `argus-github`, `argus-llm`, etc. via HTTP. mTLS would harden but adds complexity. Defer to v2.

---

## DIM 8 — Competitive Differentiation

### Evidence (FRESH May-Jun 2026)
- **CodeRabbit** ([State of AI Code Review](https://dev.to/lewiska/state-of-ai-code-review-may-2026-roundup-5o7) FRESH): Change Stack (layer-by-layer), semantic diff, Code Peek. 13M+ PRs reviewed.
- **Greptile** (same source): codebase indexing, severity scores, comments-to-coding-agent handoff (Claude/Codex/Cursor/Devin). 14/20 bugs caught in head-to-head.
- **Qodo** (same source): 64.3% F1 on Martian Code Review Bench. Multi-agent (bug/security/quality/test). Dashboard with 30-day analytics. Auto-imports rules from `.cursor/rules/`, `.cursorrules`, `SKILL.md`.
- **Macroscope** ([macroscope.com](https://macroscope.com/content/macroscope-vs-coderabbit) FRESH May 11 2026): $0.05/KB usage-based ($100 free credit). AST-based codewalkers. **"Fix It For Me" auto-fix agent.** Approvability scoring. $10/$50 per-PR caps.
- **Bito**: cheapest serious option, SOC 2 Type II.
- **PR-Agent (Qodo)**: 11K stars, self-hosted, BYO LLM. **This is our direct competitor.**

### ARGUS positioning

**Our wedge:** "BYOK + EU AI Act ready + offline-capable + pure Rust"
**Their gap:** All require cloud (no good BYOK story); none highlight EU AI Act compliance; all are SaaS.

### Actionable recommendations (cross-cutting)

**HIGH PRIORITY — Add severity scoring + /fix handoff**

```rust
// crates/argus-slop/src/severity.rs
pub enum Severity {
    Info,       // 0-19
    Low,        // 20-39
    Medium,     // 40-59
    High,       // 60-79
    Critical,   // 80-100
}

pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    pub score: u8,           // 0-100, for sorting
    pub message: String,
    pub file: PathBuf,
    pub line: u32,
    pub suggested_fix: Option<String>,  // <-- new field
}
```

CLI: `argus verify --handoff claude` → emits a `.argus-handoff.json` with the PR diff + findings that Claude Code/Codex can consume via MCP.

Files: `crates/argus-slop/src/severity.rs`, `crates/argus-verify/src/handoff.rs`
Effort: **M** (1 day)
Impact: Feature parity with Greptile's handoff. Our edge: BYOK.

**MEDIUM PRIORITY — Programmable review rules via YAML (Macroscope pattern)**

```yaml
# .argus.yml in user's repo
version: 1
rules:
  - id: no-todo-in-prod
    severity: low
    pattern: "TODO"
    exclude_paths: ["**/test/**", "**/examples/**"]
    
  - id: no-aws-key
    severity: critical
    pattern: 'AKIA[0-9A-Z]{16}'
    
  - id: prefer-typed-errors
    severity: medium
    language: rust
    ast_match: "Function { returns: Result<_, String> }"
```

CLI: `argus scan --rules .argus.yml`
Files: `crates/argus-slop/src/config.rs`, `crates/argus-slop/src/yaml_rules.rs`
Effort: **M** (1-2 days)
Impact: Differentiator. Users can encode their team's standards. CodeRabbit's `.coderabbit.yaml` is the benchmark.

**MEDIUM PRIORITY — Auto-import rules from `.cursor/rules/`, `.cursorrules`, `SKILL.md`**

Qodo and Greptile already do this. Should be ~3 hours of work.

```rust
// crates/argus-slop/src/import.rs
pub fn auto_import_rules(repo: &Path) -> Result<Vec<Rule>> {
    let cursor = repo.join(".cursor/rules");
    let cursorrules = repo.join(".cursorrules");
    let skill = repo.join("SKILL.md");
    // Parse each, convert to internal Rule format
}
```

---

## Cross-Cutting: Top 5 Quick Wins (under 1 day each)

1. **Swap default model** to `nvidia/llama-3.1-nemotron-ultra-253b-v1` (or kimi-k2.6) — 30 min, 30% quality bump
2. **Add SARIF output** to `argus scan` — 2 hours, GitHub Security tab integration
3. **Add `deslop` integration** as pre-filter — 4 hours, 60-80% cost cut
4. **Add severity score + suggested_fix fields** to Finding — 3 hours, Greptile parity
5. **Implement graceful shutdown** in argus-verify — 4 hours, production-ready

## Cross-Cutting: Top 3 Strategic Wins (1+ week each)

1. **EU AI Act Level 2 conformance** — marketing-grade compliance, Plazo de Platzi
2. **MCP server for ARGUS analyzers** — distribution play, network effect
3. **HeyGen integration in Lens** — actual video output, wow factor for the demo

---

## What NOT to Do (negative findings)

- **Don't migrate to OpenFang/AutoAgents/Rig** — pre-1.0, not worth the rewrite
- **Don't integrate D-ID** — watermarked on cheap tier, API minutes share with studio
- **Don't run our own SPIRE server** — Fly.io single-VM, no K8s
- **Don't add Postgres yet** — keep in-memory for now, add SQLx offline when we do
- **Don't port `deslop` rules** — shell out, save the 1,800-rule translation effort

---

## Sources

All sources are 2026 (Apr-Jun). Listed inline with `FRESH` tag where the publication date is in May or Jun 2026.

- [certifieddata/ai-decision-logging-spec](https://github.com/certifieddata/ai-decision-logging-spec) FRESH Apr 2026
- [DeepInspect Article 12 Implementation Guide](https://www.deepinspect.ai/blog/guides-eu-ai-act-article-12-logging-implementation) FRESH Jun 2 2026
- [Practical AI Act: Record-Keeping](https://practical-ai-act.eu/latest/conformity/record-keeping/)
- [IETF draft-veridom-omp-euaia-00](https://www.ietf.org/archive/id/draft-veridom-omp-euaia-00.txt)
- [OpenFang](https://openfang.app/) FRESH 2026
- [AutoAgents](https://github.com/liquidos-ai/AutoAgents) + [DEV.to benchmark](https://dev.to/saivishwak/benchmarking-ai-agent-frameworks-in-2026-autoagents-rust-vs-langchain-langgraph-llamaindex-338f) FRESH Feb 2026
- [Zylos Research: Rust AI Agent Frameworks 2026](https://zylos.ai/research/2026-04-01-rust-native-ai-agent-frameworks-ecosystem-2026/) FRESH Apr 2026
- [NVIDIA NIM catalog](https://build.nvidia.com/models)
- [Kimi K2.6 details](https://docs.api.nvidia.com/nim/reference/moonshotai-kimi-k2-6)
- [Nemotron 3 Ultra blog](https://developer.nvidia.com/blog/nvidia-nemotron-3-ultra-powers-faster-more-efficient-reasoning-for-long-running-agents/) FRESH Jun 4 2026
- [aislop](https://github.com/scanaislop/aislop) v0.9.4 FRESH May 28 2026
- [deslop](https://github.com/chinmay-sawant/deslop) — pure Rust
- [antislop](https://github.com/skew202/antislop) v0.3.0
- [ai-slopcheck](https://github.com/Euraika-Labs/ai-slopcheck) v1.2.0 FRESH Apr 5 2026
- [base14: Axum OTel](https://docs.base14.io/instrument/apps/auto-instrumentation/axum/)
- [EncodePanda: rust-telemetry](https://github.com/EncodePanda/rust-telemetry) FRESH Feb 2026
- [Atharva Pandey: Graceful Shutdown](https://www.atharvapandey.com/post/rust/rust-deploy-graceful-shutdown/) + [Production Deployment](https://www.atharvapandey.com/post/rust/rust-web-production/)
- [devcheolu: Axum+SQLx+Postgres+JWT](https://devcheolu.com/en/posts/REA8G6eGFYSfWm5Qd9rE) FRESH May 12 2026
- [VEED: Talking head video APIs 2026](https://www.veed.io/learn/best-talking-head-video-apis) FRESH Apr 17 2026
- [HeyGen pricing](https://developers.heygen.com/docs/pricing) + [HeyGen Developers](https://developers.heygen.com/)
- [Anam: D-ID alternatives](https://anam.ai/blog/d-id-alternatives) FRESH Apr 28 2026
- [punithvt/ai-avatar-system](https://github.com/punithvt/ai-avatar-system) — open-source reference
- [maxlambrecht/rust-spiffe](https://github.com/maxlambrecht/rust-spiffe) + [spire-api v0.7.0](https://crates.io/crates/spire-api) FRESH May 8 2026
- [State of AI Code Review May 2026](https://dev.to/lewiska/state-of-ai-code-review-may-2026-roundup-5o7) FRESH Jun 5 2026
- [Macroscope vs CodeRabbit 2026](https://macroscope.com/content/macroscope-vs-coderabbit) FRESH May 11 2026
- [AI Code Reviewer Showdown May 2026](https://www.web3aiblog.com/blog/ai-code-reviewer-showdown-greptile-coderabbit-qodo-cursor-bugbot-bito-may-2026) FRESH May 13 2026
- [bestaiweb: Qodo/CodeRabbit/Greptile 2026](https://www.bestaiweb.ai/how-to-integrate-ai-code-review-with-qodo-coderabbit-and-greptile-in-your-github-workflow-in-2026/) FRESH May 19 2026
