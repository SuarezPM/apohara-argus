# External Dependency Audit — ARGUS Supremum Implementation

**Date**: 2026-06-12
**System rustc**: 1.96.0 (ac68faa20 2026-05-25) / cargo 1.96.0
**Workspace `rust-version` declared**: 1.75 (in root `Cargo.toml`)
**Scope**: read-only spike to pin versions for the 6 Wave 1 crates and document fallback plans. No code changes; no `Cargo.toml` edits in this commit.

> **MSRV consequence**: three of the six crates (`spiffe`, `a2a-rust`, `sqlx`) require a newer
> toolchain than the workspace's declared 1.75. Wave 1 must bump `workspace.package.rust-version`
> to **1.94** (the strictest, set by `sqlx` 0.9.0). System rustc 1.96.0 already satisfies it.
> See "Workspace MSRV bump" below.

## Crate audit table

| Crate | Latest stable (crates.io) | MSRV (from `rust-version` in the crate's `Cargo.toml`) | Brief 1-line API surface (what we'd actually use) | Fallback if unavailable / broken / MSRV-incompatible |
|---|---|---|---|---|
| `llm-retry` | **0.1.0** (released 2026-05-16; 14 downloads — brand-new single-author crate) | **1.75** ✅ matches workspace | `retry(&RetryConfig, \|err\| should_retry, op)` + `retry_async(...)` with optional `tokio` feature; `Jitter::Full` enum variant; `predicates::{is_anthropic_retryable, is_openai_retryable, is_bedrock_retryable, is_google_retryable}` for provider-specific error-code classification. | Roll our own full-jitter loop in ~80 LOC (rand + retry counter + provider enum) inside `argus-llm`; or use mature `backoff` + `tokio-retry` crates which lack provider-specific predicate tables. |
| `a2a-rust` | **0.1.0** (released 2026-03-14; 24 downloads — brand-new single-author crate) | **1.85** ⚠️ bump required (also `edition = "2024"`; optional `axum = "0.8"` clashes with workspace's `axum = "0.7"`) | `A2AClient::new(A2AClientConfig)` for outbound calls, `AgentCardDiscovery` for `.well-known/agent-card.json` fetch, `TaskStore` trait + `InMemoryTaskStore` impl, `A2AError` enum, full `types` module with A2A v1.0 protocol types. `server` feature wires an axum router; `client` feature adds `reqwest`-based transport. | If a2a-rust is too immature (24 downloads, 1 release): implement the 6 A2A v1.0 endpoints directly over `reqwest` + manual JSON-RPC 2.0 envelopes (~300 LOC, no crate dep). To dodge the `axum 0.7` vs `axum 0.8` skew, consume with `default-features = false, features = ["client"]` (no axum pulled in). |
| `spiffe` | **0.16.0** (released 2026-06-08; 775,896 downloads — mature, used in production) | **1.88** ⚠️ bump required (pinned via `time >=0.3.47` for RUSTSEC-2026-0009) | `SpiffeId` / `TrustDomain` newtypes; `JwtSource::fetch_jwt_svid(&["audience"]).await` and `WorkloadApiClient::connect_env().await` for SVID minting; `BundleSource::bundle_for_trust_domain(&td)` for JWKS retrieval. Features: enable `workload-api-jwt` for JWT only, or `workload-api` for both X.509 + JWT. | If MSRV or API changes break us: parse `spiffe://trust-domain/path` ourselves (~50 LOC, regex + `url::Url`); sign JWTs with `jsonwebtoken` 10.x directly; fetch JWKS via `reqwest`. We lose the Workload-API streaming watcher but keep the protocol surface. |
| `syn` | **2.0.117** (released 2026-02-20; 1.79 B downloads — dtolnay, de-facto standard) | **1.71** ✅ below workspace floor | `syn::parse_file(&contents)?` then `syn::visit::Visit::visit_file(&mut visitor, &file)` to walk `ItemFn` / `ItemStruct` / `ExprMethodCall` for slop-pattern detection. Use `features = ["full", "visit", "extra-traits"]` (defaults already include `derive`/`parsing`/`printing`/`clone-impls`/`proc-macro`). | No realistic fallback for deterministic AST analysis — `ra_ap_syntax` (rust-analyzer) is heavier and not stable. If `syn` ever blocks us, fall back to line/brace-scanning heuristics (loses precision but unblocks ship). |
| `quote` | **1.0.45** (released 2026-03-03; 1.23 B downloads — dtolnay, de-facto standard) | **1.71** ✅ below workspace floor | `quote!(#name, #ty, ...)` macro + `ToTokens` trait impls (paired with `syn`) for any code-generation we might add to the slop-detector's "rewrite to fix" path. | If `quote` is unavailable we cannot write proc macros — but ARGUS is not a proc-macro crate, so a missing `quote` is harmless. We can drop it from the dep list. |
| `sqlx` (features: `runtime-tokio` + `tls-rustls` + `sqlite` + `migrate`) | **0.9.0** (released 2026-05-21; 107.9 M downloads — mature). NB: brief said "0.8.x"; 0.9.0 is the current latest stable. In 0.9 the combined `runtime-tokio-rustls` feature was split — enable `runtime-tokio` + `tls-rustls` (or `tls-rustls-ring-native-roots`) separately. | **1.94.0** ⚠️ bump required (highest MSRV in this audit) | `SqlitePool::connect(&url).await`; embed migrations with `static MIGRATOR: Migrator = sqlx::migrate!("./migrations");` and run `MIGRATOR.run(&pool).await.unwrap();`. Use `sqlx::query!("SELECT ...").fetch_all(&pool)` for compile-time-checked queries (requires `DATABASE_URL` at build time) or `sqlx::query(...)` for runtime queries. | If 0.9's MSRV is a blocker, pin to `sqlx = "0.8"` (MSRV ~1.78 per their `CHANGELOG`). If sqlx is unusable entirely: `rusqlite` (sync, mature, MSRV 1.70) wrapped in `tokio::task::spawn_blocking`; loses compile-time query checking but keeps the migration story. |

## Feature-flag correction (sqlx 0.9)

The brief specified `features = ["sqlite", "runtime-tokio-rustls", "migrate"]`. In `sqlx` 0.9 that combined feature no longer exists — it is now `runtime-tokio` + `tls-rustls` (or a `-ring-*` variant) + `sqlite` + `migrate`. Wave 1 should write:

```toml
sqlx = { version = "0.9", default-features = false, features = [
    "runtime-tokio",
    "tls-rustls-ring-native-roots",  # or "tls-rustls" for the bare webpki-roots default
    "sqlite",
    "migrate",
    "macros",                        # needed for query!/query_as! macros
] }
```

## Workspace MSRV bump

| Crate | MSRV | Action |
|---|---|---|
| `syn` 2.0.117 | 1.71 | none |
| `quote` 1.0.45 | 1.71 | none |
| `llm-retry` 0.1.0 | 1.75 | none (matches current workspace) |
| `a2a-rust` 0.1.0 | 1.85 | bump `rust-version` |
| `spiffe` 0.16.0 | 1.88 | bump `rust-version` |
| `sqlx` 0.9.0 | 1.94 | bump `rust-version` (highest) |

**Required**: raise `workspace.package.rust-version` from `"1.75"` → `"1.94"` in root `Cargo.toml`. System rustc 1.96.0 already satisfies it (verified by `rustc --version`).

## Cross-cutting risks

1. **Immature single-author crates** (`llm-retry` 0.1.0 / 14 downloads, `a2a-rust` 0.1.0 / 24 downloads). If either disappears or ships a breaking 0.2.0, fallbacks above are ready and small.
2. **`a2a-rust` `axum 0.8` vs workspace `axum 0.7`**. Two axum majors cannot coexist. Mitigation: use `default-features = false, features = ["client"]` (no axum pulled in; `server` feature is optional). If we need the in-crate A2A server, bump workspace `axum` to 0.8 in Wave 1.
3. **Spiffe ecosystem split** (`spiffe-rustls` 0.7.0, `spiffe-rustls-tokio` 0.4.0) — useful siblings if we need mTLS in `argus-guard` for Items 8.1/8.2, but not required for the JWT-SVID-only path.

## Sources queried

- crates.io REST API: `https://crates.io/api/v1/crates/{name}` (versions, downloads, repository, description)
- docs.rs: `https://docs.rs/{name}/latest/{name}/` (re-exports, modules, feature lists)
- Upstream `Cargo.toml` on GitHub for crates that don't publish `rust-version` via crates.io (`syn`, `quote`, `sqlx`, `spiffe`, `a2a-rust`, `llm-retry`)
- `cargo search` cross-check (all six versions match)
