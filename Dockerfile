# ─── Stage 1: Builder ────────────────────────────────────────────────────────
# rust:1.88 required by transitive deps: home, time, icu_* crates.
# protoc is NOT needed at the system level — protoc-bin-vendored supplies it.
FROM rust:1.88-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# ── Layer 1: manifests + proto (changes rarely) ───────────────────────────────
COPY Cargo.toml Cargo.lock build.rs ./
COPY proto/ ./proto/

# Dummy binary so `cargo fetch` / dep compilation can proceed without real src
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs

# Pre-fetch and compile all dependencies.
# The 2>/dev/null suppresses the "couldn't compile" noise from the stub main.
RUN cargo build --release 2>/dev/null; true

# ── Layer 2: real source (changes often) ──────────────────────────────────────
COPY src/ ./src/
COPY migrations/ ./migrations/

# Invalidate the cached stub so Cargo recompiles the real binary
RUN touch src/main.rs && cargo build --release

# ─── Stage 2: Runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Run as non-root
RUN useradd -ms /bin/bash wallet
USER wallet

WORKDIR /app

COPY --from=builder /app/target/release/wallet-engine .
COPY --from=builder /app/migrations/ ./migrations/

# gRPC | Prometheus metrics
EXPOSE 50051 9090

ENV RUST_LOG=wallet_engine=info

ENTRYPOINT ["./wallet-engine"]