# ─── Stage 1: Builder ────────────────────────────────────────────────────────
FROM rust:1.82-slim-bookworm AS builder

# Install system deps for sqlx, tonic-build (protoc), openssl
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./
COPY proto/ ./proto/

# Create a dummy main so `cargo build` can compile deps
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs

RUN cargo build --release 2>/dev/null; true

# Now copy real source and build
COPY src/ ./src/
COPY migrations/ ./migrations/

# Touch main.rs to force recompile
RUN touch src/main.rs

RUN cargo build --release

# ─── Stage 2: Runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for security
RUN useradd -ms /bin/bash wallet
USER wallet

WORKDIR /app

# Copy binary and migrations
COPY --from=builder /app/target/release/wallet-engine .
COPY --from=builder /app/migrations/ ./migrations/

# gRPC port | Prometheus metrics port
EXPOSE 50051 9090

ENV RUST_LOG=wallet_engine=info

ENTRYPOINT ["./wallet-engine"]