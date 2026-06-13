# ARGUS Dockerfile — multi-stage build for a minimal Rust image
# Stage 1: build the binaries
FROM rust:1.75-slim AS builder

WORKDIR /build

# Install build deps
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Cache deps
COPY Cargo.toml Cargo.lock* ./
COPY crates/apohara-argus-core/Cargo.toml crates/apohara-argus-core/Cargo.toml
COPY crates/argus-crypto/Cargo.toml crates/argus-crypto/Cargo.toml
COPY crates/argus-llm/Cargo.toml crates/argus-llm/Cargo.toml
COPY crates/argus-slop/Cargo.toml crates/argus-slop/Cargo.toml
COPY crates/argus-github/Cargo.toml crates/argus-github/Cargo.toml
COPY crates/argus-guard/Cargo.toml crates/argus-guard/Cargo.toml
COPY crates/argus-verify/Cargo.toml crates/argus-verify/Cargo.toml
COPY crates/argus-lens/Cargo.toml crates/argus-lens/Cargo.toml
COPY crates/argus-agent/Cargo.toml crates/argus-agent/Cargo.toml
COPY crates/argus-api/Cargo.toml crates/argus-api/Cargo.toml
COPY crates/argus-dashboard/Cargo.toml crates/argus-dashboard/Cargo.toml
COPY crates/apohara-argus-cli/Cargo.toml crates/apohara-argus-cli/Cargo.toml

# Create dummy sources so cargo can resolve
RUN mkdir -p crates/{apohara-argus-core,argus-crypto,argus-llm,argus-slop,argus-github,argus-guard,argus-verify,argus-lens,argus-agent,argus-api,argus-dashboard,apohara-argus-cli}/src && \
    echo 'pub fn _dummy() {}' > crates/apohara-argus-core/src/lib.rs && \
    for c in crypto llm slop github guard verify lens agent api dashboard; do \
        echo 'pub fn _dummy() {}' > crates/argus-$c/src/lib.rs; done && \
    echo 'pub fn _dummy() {}' > crates/apohara-argus-cli/src/lib.rs && \
    for c in guard verify lens dashboard; do \
        echo 'fn main() {}' > crates/argus-$c/src/main.rs; done && \
    echo 'fn main() {}' > crates/apohara-argus-cli/src/main.rs
RUN cargo build --release --bin argus 2>&1 | tail -5

# Now copy the real sources
COPY crates/ crates/
RUN touch crates/{apohara-argus-core,argus-crypto,argus-llm,argus-slop,argus-github,argus-guard,argus-verify,argus-lens,argus-agent,argus-api,argus-dashboard,apohara-argus-cli}/src/lib.rs && \
    for c in guard verify lens dashboard; do touch crates/argus-$c/src/main.rs; done && \
    touch crates/apohara-argus-cli/src/main.rs
RUN cargo build --release --bin argus --bin argus-verify --bin argus-dashboard --bin argus-lens --bin argus-guard

# Stage 2: minimal runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/target/release/argus /usr/local/bin/argus
COPY --from=builder /build/target/release/argus-verify /usr/local/bin/argus-verify
COPY --from=builder /build/target/release/argus-dashboard /usr/local/bin/argus-dashboard
COPY --from=builder /build/target/release/argus-lens /usr/local/bin/argus-lens
COPY --from=builder /build/target/release/argus-guard /usr/local/bin/argus-guard
COPY --from=builder /build/docs ./docs

# Default: run the unified CLI. Override with `argus-dashboard`, `argus-verify`, etc.
ENV ARGUS_API_PORT=8080
ENV ARGUS_DASHBOARD_PORT=3000
ENV RUST_LOG=info,argus=debug

EXPOSE 3000 8080

ENTRYPOINT ["/usr/local/bin/argus"]
CMD ["--help"]
