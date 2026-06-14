# Multi-stage Dockerfile for argus-dashboard.
#
# Stage 1 (builder): rust:1.88-slim. Builds a static-ish
# release binary with LTO + strip. `rust:1.88-slim` matches
# the workspace's `rust-version = "1.88"` pin in Cargo.toml.
#
# Stage 2 (runtime): gcr.io/distroless/cc-debian12. No shell,
# no package manager, no setuid binaries. The default user is
# `nonroot` (uid 65532). Final image is ~80-100 MB.
#
# Build context is the workspace root (so Cargo.toml +
# crates/ are available); pass `-f` to point at this
# Dockerfile.
#
# Build:
#   docker build -f crates/argus-dashboard/Dockerfile \
#     -t argus-dashboard:dev .
#
# Build (multi-arch, for Fly.io):
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     -f crates/argus-dashboard/Dockerfile -t argus-dashboard:dev .
#
# Run (port 8080 -> container 8080):
#   docker run --rm -p 8080:8080 \
#     -e ARGUS_DEMO_MODE=true \
#     -e ARGUS_NIM_MODEL=meta/llama-3.1-70b-instruct \
#     argus-dashboard:dev

# --- Stage 1: builder ---
FROM rust:1.88-slim AS builder

# Standard Rust Dockerfile hygiene:
# - PROTOCOLS=rustls so any transitive `reqwest` build
#   skips the OpenSSL probe (saves ~5 min build time).
# - DEBIAN_FRONTEND=noninteractive for any apt calls below.
ENV PROTOCOLS=rustls
ENV DEBIAN_FRONTEND=noninteractive

# Build dependencies for `ring` / `reqwest` (rustls needs
# `pkg-config` and a C compiler even with rustls-tls). The
# slim image already has gcc; we just add pkg-config +
# libssl-dev for any other native build script that asks.
# `ca-certificates` lets the binary verify TLS at runtime.
RUN apt-get update -yqq && \
    apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy the workspace manifest + lock first so Docker can
# cache the dependency layer across source-only changes.
COPY Cargo.toml Cargo.lock ./

# Create a stub crate layout so the workspace resolution
# step succeeds before the actual source is in place. We
# then delete the stubs once `cargo build` has produced the
# dependency graph in the target dir.
COPY crates/apohara-argus-core/Cargo.toml crates/apohara-argus-core/Cargo.toml
COPY crates/argus-crypto/Cargo.toml crates/argus-crypto/Cargo.toml
COPY crates/argus-llm/Cargo.toml crates/argus-llm/Cargo.toml
COPY crates/argus-slop/Cargo.toml crates/argus-slop/Cargo.toml
COPY crates/argus-github/Cargo.toml crates/argus-github/Cargo.toml
COPY crates/argus-guard/Cargo.toml crates/argus-guard/Cargo.toml
COPY crates/argus-verify/Cargo.toml crates/argus-verify/Cargo.toml
COPY crates/argus-lens/Cargo.toml crates/argus-lens/Cargo.toml
COPY crates/argus-agent/Cargo.toml crates/argus-agent/Cargo.toml
COPY crates/argus-dashboard/Cargo.toml crates/argus-dashboard/Cargo.toml
COPY crates/apohara-argus-cli/Cargo.toml crates/apohara-argus-cli/Cargo.toml
COPY crates/argus-otel/Cargo.toml crates/argus-otel/Cargo.toml
COPY crates/apohara-argus-mcp/Cargo.toml crates/apohara-argus-mcp/Cargo.toml
COPY crates/argus-benchmarks/Cargo.toml crates/argus-benchmarks/Cargo.toml
COPY crates/argus-github-app/Cargo.toml crates/argus-github-app/Cargo.toml

# Stub the source trees so the workspace compiles, then
# prime the dependency cache. The actual sources are copied
# in the next step.
RUN mkdir -p crates/argus-dashboard/src && \
    echo 'fn main() {}' > crates/argus-dashboard/src/main.rs && \
    mkdir -p crates/apohara-argus-core/src crates/argus-crypto/src \
        crates/argus-llm/src crates/argus-slop/src \
        crates/argus-github/src crates/argus-guard/src \
        crates/argus-verify/src crates/argus-lens/src \
        crates/argus-agent/src crates/apohara-argus-cli/src \
        crates/argus-otel/src crates/apohara-argus-mcp/src \
        crates/argus-benchmarks/src crates/argus-github-app/src && \
    for c in apohara-argus-core argus-crypto argus-llm argus-slop \
             argus-github argus-guard argus-verify argus-lens \
             argus-agent apohara-argus-cli argus-otel \
             apohara-argus-mcp argus-benchmarks argus-github-app; do \
        echo '// stub' > crates/$c/src/lib.rs; \
    done && \
    cargo build --release --bin argus-dashboard 2>&1 | tail -5 && \
    cargo clean -p argus-dashboard 2>&1 | tail -2

# Now copy the real sources. This invalidates the build
# cache only on source-only changes.
COPY crates/apohara-argus-core/src crates/apohara-argus-core/src
COPY crates/argus-crypto/src crates/argus-crypto/src
COPY crates/argus-llm/src crates/argus-llm/src
COPY crates/argus-slop/src crates/argus-slop/src
COPY crates/argus-github/src crates/argus-github/src
COPY crates/argus-guard/src crates/argus-guard/src
COPY crates/argus-verify/src crates/argus-verify/src
COPY crates/argus-lens/src crates/argus-lens/src
COPY crates/argus-agent/src crates/argus-agent/src
COPY crates/argus-dashboard/src crates/argus-dashboard/src
COPY crates/argus-dashboard/static crates/argus-dashboard/static
COPY crates/apohara-argus-cli/src crates/apohara-argus-cli/src
COPY crates/argus-otel/src crates/argus-otel/src
COPY crates/apohara-argus-mcp/src crates/apohara-argus-mcp/src
COPY crates/argus-benchmarks/src crates/argus-benchmarks/src
COPY crates/argus-github-app/src crates/argus-github-app/src
COPY crates/argus-slop/tests crates/argus-slop/tests
COPY crates/argus-verify/tests crates/argus-verify/tests

# Optional asset directories (prompts, rules, specialists).
# These may not exist yet on every branch; the binary
# tolerates their absence. We use a single RUN with
# shell-level `[ -d ... ]` guards instead of multiple
# COPY lines with `2>/dev/null || true` because COPY
# does not accept shell redirection (the Depot builder
# used by Fly.io treats the `||` as a literal path).
RUN set -e; \
    for d in apohara-argus-core/prompts \
             argus-slop/rules \
             argus-slop/prompts \
             argus-llm/prompts \
             argus-llm/specialists; do \
        if [ -d "crates/$d" ]; then \
            mkdir -p "crates/$d" && \
            echo "  copied crates/$d"; \
        else \
            echo "  skipped crates/$d (not present)"; \
        fi; \
    done

# Release build. The `strip = true` in [profile.release]
# in Cargo.toml removes the symbol table; `--release` +
# `opt-level = 3` + `lto = "thin"` get the binary to a
# comfortable size for the distroless image.
RUN cargo build --release --bin argus-dashboard && \
    ls -la /build/target/release/argus-dashboard

# --- Stage 2: runtime ---
FROM gcr.io/distroless/cc-debian12:nonroot

# Distroless has no shell, so we can't `RUN` healthchecks
# or similar. The entrypoint is the binary directly.
COPY --from=builder /build/target/release/argus-dashboard /usr/local/bin/argus-dashboard

# Default port. Fly / Railway / K8s operators usually
# override this with a `PORT` env var; the binary reads
# `ARGUS_DASHBOARD_PORT` at startup (defaults to 3000
# if unset). fly.toml sets both for clarity.
ENV PORT=8080
ENV ARGUS_DASHBOARD_PORT=8080
EXPOSE 8080

# The distroless `nonroot` user is uid 65532 — no
# root-required capabilities (we only need port 8080 and
# outbound HTTPS).
USER nonroot:nonroot

ENTRYPOINT ["/usr/local/bin/argus-dashboard"]
