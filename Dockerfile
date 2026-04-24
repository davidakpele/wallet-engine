FROM rust:latest

WORKDIR /app

COPY . .

# Install sqlx-cli with only postgres feature
RUN cargo install sqlx-cli --no-default-features --features postgres

# Build the application
RUN cargo build --release

# gRPC | Prometheus metrics
EXPOSE 50051 9090

CMD sqlx migrate run && ./target/release/wallet-engine
