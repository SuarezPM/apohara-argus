# Multi-stage Dockerfile for argus-github-app.
#
# Stage 1 (builder): rust:1.88-slim. Builds a static-ish
# release binary with LTO + strip. `rust:1.88-slim` matches
# the workspace's `rust-version = "1.88"` pin in Cargo.toml.
#
# Stage 2 (runtime): gcr.io/distroless/cc-debian12. No shell,
# no package manager, no setuid binaries. The default user is
# `nonroot` (uid 65532). Final image is ~80-90 MB.
#
# Build context is the workspace root (so Cargo.toml +
# crates/ are available); pass `-f` to point at this
# Dockerfile.
#
# Build:
#   docker build -f crates/argus-github-app/Dockerfile \
#     -t apohara-argus:dev .
#
# Run (port 8080 -> container 8080):
#   docker run --rm -p 8080:8080 \
#     -e ARGUS_APP_WEBHOOK_SECRET=... \
#     -e ARGUS_APP_INSTALL_TOKEN=... \
#     apohara-argus:dev

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
COPY crates/argus-github-app/Cargo.toml crates/argus-github-app/Cargo.toml
COPY crates/argus-core/Cargo.toml crates/argus-core/Cargo.toml
COPY crates/argus-crypto/Cargo.toml crates/argus-crypto/Cargo.toml
COPY crates/argus-llm/Cargo.toml crates/argus-llm/Cargo.toml
COPY crates/argus-slop/Cargo.toml crates/argus-slop/Cargo.toml
COPY crates/argus-github/Cargo.toml crates/argus-github/Cargo.toml
COPY crates/argus-guard/Cargo.toml crates/argus-guard/Cargo.toml
COPY crates/argus-verify/Cargo.toml crates/argus-verify/Cargo.toml
COPY crates/argus-lens/Cargo.toml crates/argus-lens/Cargo.toml
COPY crates/argus-agent/Cargo.toml crates/argus-agent/Cargo.toml
COPY crates/argus-dashboard/Cargo.toml crates/argus-dashboard/Cargo.toml
COPY crates/argus-cli/Cargo.toml crates/argus-cli/Cargo.toml
COPY crates/argus-otel/Cargo.toml crates/argus-otel/Cargo.toml
COPY crates/argus-mcp/Cargo.toml crates/argus-mcp/Cargo.toml
COPY crates/argus-benchmarks/Cargo.toml crates/argus-benchmarks/Cargo.toml

# Stub the source trees so the workspace compiles, then
# prime the dependency cache. The actual sources are copied
# in the next step.
RUN mkdir -p crates/argus-github-app/src && \
    echo 'fn main() {}' > crates/argus-github-app/src/main.rs && \
    mkdir -p crates/argus-core/src crates/argus-crypto/src \
        crates/argus-llm/src crates/argus-slop/src \
        crates/argus-github/src crates/argus-guard/src \
        crates/argus-verify/src crates/argus-lens/src \
        crates/argus-agent/src crates/argus-dashboard/src \
        crates/argus-cli/src crates/argus-otel/src \
        crates/argus-mcp/src crates/argus-benchmarks/src && \
    for c in argus-core argus-crypto argus-llm argus-slop \
             argus-github argus-guard argus-verify argus-lens \
             argus-agent argus-dashboard argus-cli argus-otel \
             argus-mcp argus-benchmarks; do \
        echo '// stub' > crates/$c/src/lib.rs; \
    done && \
    cargo build --release --bin argus-github-app 2>&1 | tail -5 && \
    cargo clean -p argus-github-app 2>&1 | tail -2

# Now copy the real sources. This invalidates the build
# cache only on source changes.
COPY crates/argus-core/src crates/argus-core/src
COPY crates/argus-crypto/src crates/argus-crypto/src
COPY crates/argus-llm/src crates/argus-llm/src
COPY crates/argus-slop/src crates/argus-slop/src
COPY crates/argus-github/src crates/argus-github/src
COPY crates/argus-guard/src crates/argus-guard/src
COPY crates/argus-verify/src crates/argus-verify/src
COPY crates/argus-lens/src crates/argus-lens/src
COPY crates/argus-agent/src crates/argus-agent/src
COPY crates/argus-dashboard/src crates/argus-dashboard/src
COPY crates/argus-cli/src crates/argus-cli/src
COPY crates/argus-otel/src crates/argus-otel/src
COPY crates/argus-mcp/src crates/argus-mcp/src
COPY crates/argus-benchmarks/src crates/argus-benchmarks/src
COPY crates/argus-github-app/src crates/argus-github-app/src
COPY crates/argus-slop/tests crates/argus-slop/tests
COPY crates/argus-verify/tests crates/argus-verify/tests
COPY crates/argus-slop/rules crates/argus-slop/rules 2>/dev/null || true
COPY crates/argus-slop/prompts crates/argus-slop/prompts 2>/dev/null || true
COPY crates/argus-llm/prompts crates/argus-llm/prompts 2>/dev/null || true
COPY crates/argus-llm/specialists crates/argus-llm/specialists 2>/dev/null || true

# Release build. The `strip = true` in [profile.release]
# in Cargo.toml removes the symbol table; `--release` +
# `opt-level = 3` + `lto = "thin"` get the binary to a
# comfortable size for the distroless image.
RUN cargo build --release --bin argus-github-app && \
    ls -la /build/target/release/argus-github-app

# --- Stage 2: runtime ---
FROM gcr.io/distroless/cc-debian12:nonroot

# Distroless has no shell, so we can't `RUN` healthchecks
# or similar. The entrypoint is the binary directly.
COPY --from=builder /build/target/release/argus-github-app /usr/local/bin/argus-github-app

# Default port. Fly / Railway / K8s operators usually
# override this with a `PORT` env var; the binary reads
# `PORT` at startup.
ENV PORT=8080
EXPOSE 8080

# The distroless `nonroot` user is uid 65532 — no
# root-required capabilities (we only need port 8080 and
# outbound HTTPS).
USER nonroot:nonroot

ENTRYPOINT ["/usr/local/bin/argus-github-app"]
