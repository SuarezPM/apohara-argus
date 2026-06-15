# ARGUS Dockerfile — multi-stage build for a multi-binary runtime image.
#
# Stage 1 (builder): rust:1.88-slim. Matches the workspace's
# `rust-version = "1.88"` pin in Cargo.toml. Builds every
# user-facing binary into target/release/.
#
# Stage 2 (runtime): debian:bookworm-slim. The deploy image
# is meant to be a *single image with all binaries in $PATH*,
# so an operator can `docker run ... argus-verify` or
# `docker run ... argus-dashboard` or `docker run ... argus`
# without rebuilding. The default `ENTRYPOINT` is the unified
# `argus` CLI.
#
# Build context is the workspace root (so Cargo.toml +
# crates/ are available).
#
# Build:
#   docker build -f deploy/Dockerfile -t apohara-argus:deploy .
#
# Run (default = unified CLI):
#   docker run --rm apohara-argus:deploy --help
#   docker run --rm apohara-argus:deploy argus-dashboard --port 3000
#   docker run --rm apohara-argus:deploy argus-verify    --port 8080
#   docker run --rm apohara-argus:deploy argus-mcp
#   docker run --rm apohara-argus:deploy argus-lens --days 7
#   docker run --rm apohara-argus:deploy argus-guard pre-commit
#
# 7 user-facing binaries are installed:
#   - argus            (from apohara-argus-cli)
#   - argus-verify     (from argus-verify)
#   - argus-dashboard  (from argus-dashboard)
#   - argus-lens       (from argus-lens)
#   - argus-guard      (from argus-guard)
#   - argus-mcp        (from apohara-argus-mcp)
#   - argus-github-app (from argus-github-app; also has its
#                       own single-binary distroless Dockerfile
#                       in crates/argus-github-app/Dockerfile
#                       for production deploys)

# --- Stage 1: builder ---
FROM rust:1.88-slim@sha256:38bc5a86d998772d4aec2348656ed21438d20fcdce2795b56ca434cf21430d89 AS builder

WORKDIR /build

# Standard Rust Dockerfile hygiene: PROTOCOLS=rustls so any
# transitive `reqwest` build skips the OpenSSL probe.
ENV PROTOCOLS=rustls
ENV DEBIAN_FRONTEND=noninteractive

# Build deps for `ring` / `reqwest` (pkg-config + a C
# compiler). The slim image already has gcc; we add
# pkg-config + libssl-dev for native build scripts and
# ca-certificates so the binaries verify TLS at runtime.
RUN apt-get update -yqq && \
    apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Cache deps: copy the workspace manifest + every member
# crate's manifest BEFORE copying the sources, so Docker can
# cache the dependency layer across source-only changes.
COPY Cargo.toml Cargo.lock* ./
COPY crates/apohara-argus-core/Cargo.toml  crates/apohara-argus-core/Cargo.toml
COPY crates/apohara-argus-cli/Cargo.toml   crates/apohara-argus-cli/Cargo.toml
COPY crates/apohara-argus-mcp/Cargo.toml  crates/apohara-argus-mcp/Cargo.toml
COPY crates/argus-crypto/Cargo.toml        crates/argus-crypto/Cargo.toml
COPY crates/argus-llm/Cargo.toml           crates/argus-llm/Cargo.toml
COPY crates/argus-slop/Cargo.toml          crates/argus-slop/Cargo.toml
COPY crates/argus-github/Cargo.toml        crates/argus-github/Cargo.toml
COPY crates/argus-github-app/Cargo.toml    crates/argus-github-app/Cargo.toml
COPY crates/argus-guard/Cargo.toml         crates/argus-guard/Cargo.toml
COPY crates/argus-verify/Cargo.toml        crates/argus-verify/Cargo.toml
COPY crates/argus-lens/Cargo.toml          crates/argus-lens/Cargo.toml
COPY crates/argus-agent/Cargo.toml         crates/argus-agent/Cargo.toml
COPY crates/argus-dashboard/Cargo.toml     crates/argus-dashboard/Cargo.toml
COPY crates/argus-otel/Cargo.toml          crates/argus-otel/Cargo.toml
COPY crates/argus-benchmarks/Cargo.toml    crates/argus-benchmarks/Cargo.toml

# Create dummy sources so cargo can resolve the workspace
# without the real sources yet. Every lib crate gets a stub
# lib.rs; every bin crate also gets a stub main.rs. We then
# prime the dep cache by building ONE binary (cheapest).
# 15 crates total; 7 of them have a binary entry point.
RUN mkdir -p \
        crates/apohara-argus-core/src \
        crates/apohara-argus-cli/src \
        crates/apohara-argus-mcp/src \
        crates/argus-crypto/src \
        crates/argus-llm/src \
        crates/argus-slop/src \
        crates/argus-github/src \
        crates/argus-github-app/src \
        crates/argus-guard/src \
        crates/argus-verify/src \
        crates/argus-lens/src \
        crates/argus-agent/src \
        crates/argus-dashboard/src \
        crates/argus-otel/src \
        crates/argus-benchmarks/src && \
    for c in apohara-argus-core argus-crypto argus-llm argus-slop \
             argus-github argus-github-app argus-guard argus-verify \
             argus-lens argus-agent argus-dashboard argus-otel \
             apohara-argus-mcp argus-benchmarks; do \
        echo 'pub fn _dummy() {}' > crates/$c/src/lib.rs; \
    done && \
    echo 'pub fn _dummy() {}' > crates/apohara-argus-cli/src/lib.rs && \
    for c in argus-guard argus-verify argus-lens argus-dashboard argus-github-app; do \
        echo 'fn main() {}' > crates/$c/src/main.rs; \
    done && \
    echo 'fn main() {}' > crates/apohara-argus-cli/src/main.rs && \
    echo 'fn main() {}' > crates/apohara-argus-mcp/src/main.rs
RUN cargo build --release --bin argus 2>&1 | tail -5

# Now copy the real sources. This invalidates the build
# cache only on source changes.
COPY crates/ crates/

# Touch every stub file so cargo sees the real sources as
# newer and rebuilds.
RUN touch \
        crates/apohara-argus-core/src/lib.rs \
        crates/apohara-argus-cli/src/lib.rs \
        crates/apohara-argus-mcp/src/lib.rs \
        crates/argus-crypto/src/lib.rs \
        crates/argus-llm/src/lib.rs \
        crates/argus-slop/src/lib.rs \
        crates/argus-github/src/lib.rs \
        crates/argus-github-app/src/lib.rs \
        crates/argus-guard/src/lib.rs \
        crates/argus-verify/src/lib.rs \
        crates/argus-lens/src/lib.rs \
        crates/argus-agent/src/lib.rs \
        crates/argus-dashboard/src/lib.rs \
        crates/argus-otel/src/lib.rs \
        crates/argus-benchmarks/src/lib.rs && \
    touch \
        crates/argus-guard/src/main.rs \
        crates/argus-verify/src/main.rs \
        crates/argus-lens/src/main.rs \
        crates/argus-dashboard/src/main.rs \
        crates/argus-github-app/src/main.rs \
        crates/apohara-argus-cli/src/main.rs \
        crates/apohara-argus-mcp/src/main.rs

# Build all 7 user-facing binaries in one cargo invocation
# (shared incremental build cache).
RUN cargo build --release \
        --bin argus \
        --bin argus-verify \
        --bin argus-dashboard \
        --bin argus-lens \
        --bin argus-guard \
        --bin argus-mcp \
        --bin argus-github-app

# --- Stage 2: minimal runtime image ---
FROM debian:bookworm-slim@sha256:96e378d7e6531ac9a15ad505478fcc2e69f371b10f5cdf87857c4b8188404716

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy all 7 user-facing binaries into $PATH.
COPY --from=builder /build/target/release/argus             /usr/local/bin/argus
COPY --from=builder /build/target/release/argus-verify      /usr/local/bin/argus-verify
COPY --from=builder /build/target/release/argus-dashboard   /usr/local/bin/argus-dashboard
COPY --from=builder /build/target/release/argus-lens        /usr/local/bin/argus-lens
COPY --from=builder /build/target/release/argus-guard       /usr/local/bin/argus-guard
COPY --from=builder /build/target/release/argus-mcp         /usr/local/bin/argus-mcp
COPY --from=builder /build/target/release/argus-github-app  /usr/local/bin/argus-github-app
COPY --from=builder /build/docs ./docs

# Default: run the unified CLI. Override with
# `docker run ... argus-dashboard`, `docker run ... argus-verify`, etc.
ENV ARGUS_API_PORT=8080
ENV ARGUS_DASHBOARD_PORT=3000
ENV RUST_LOG=info,argus=debug

EXPOSE 3000 8080

ENTRYPOINT ["/usr/local/bin/argus"]
CMD ["--help"]
