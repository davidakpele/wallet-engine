# wallet-engine

> High-performance financial transaction engine — Rust · gRPC · PostgreSQL · RabbitMQ

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     interfaces/                             │
│   gRPC Handler  (thin adapter: proto ↔ application DTOs)    │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│                    application/                             │
│   TransactionService  (use-cases, orchestration)            │
│   No infrastructure imports — only trait objects            │
└──────┬────────────────────────────────────────┬────────────┘
       │ WalletRepository (port)                 │ EventPublisher (port)
       │ TransactionRepository (port)            │
┌──────▼──────────────────────┐  ┌──────────────▼────────────┐
│   infrastructure/postgres/  │  │  infrastructure/rabbitmq/ │
│   PgWalletRepository        │  │  RabbitMQPublisher        │
│   PgTransactionRepository   │  │  DLX / DLQ setup          │
└─────────────────────────────┘  └───────────────────────────┘
       │
┌──────▼──────────────────────────────────────────────────────┐
│                      domain/                                │
│  Wallet · Transaction · Money · Currency · TransactionId    │
│  DomainError · DomainEvent · Repository traits              │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

| Concern | Decision | Rationale |
|---|---|---|
| Arithmetic | `rust_decimal::Decimal` | Zero floating-point error |
| Concurrency | Pessimistic locking (`SELECT FOR UPDATE`) | Prevents double-spend in high-TPS bursts |
| Optimistic lock | `version` column on `wallets` | Secondary guard for out-of-band updates |
| Idempotency | Unique constraint on `idempotency_key` + pre-check | At-most-once processing |
| Atomicity | `sqlx::Transaction` spanning wallet + transaction writes | ACID guarantee |
| Deadlock avoidance | Lock wallets in UUID-sorted order during transfers | Consistent lock ordering |
| Events | RabbitMQ topic exchange + DLX/DLQ | Durable, retryable event delivery |
| Rate limiting | `governor` token bucket (global + per-wallet) | Prevents abuse |

---

## Project Layout

```
wallet-engine/
├── proto/
│   └── wallet.proto              # gRPC service definition
├── src/
│   ├── main.rs                   # Composition root
│   ├── config.rs                 # Environment-driven config
│   ├── observability.rs          # Tracing + Prometheus
│   ├── domain/
│   │   ├── entities.rs           # Wallet, Transaction aggregates
│   │   ├── value_objects.rs      # Money, Currency, TransactionId, …
│   │   ├── errors.rs             # DomainError (thiserror)
│   │   ├── events.rs             # DomainEvent variants
│   │   ├── repositories.rs       # Port traits
│   │   └── tests.rs              # Domain unit tests
│   ├── application/
│   │   ├── service.rs            # TransactionService use-cases
│   │   ├── dto.rs                # Command / Result DTOs
│   │   ├── ports.rs              # EventPublisher port
│   │   └── errors.rs             # ApplicationError
│   ├── infrastructure/
│   │   ├── postgres/
│   │   │   ├── wallet_repository.rs
│   │   │   └── transaction_repository.rs
│   │   └── rabbitmq/
│   │       └── publisher.rs
│   └── interfaces/
│       └── grpc/
│           ├── handler.rs        # tonic WalletService impl
│           └── middleware.rs     # Rate limiting, metrics
├── migrations/
│   └── 20240101000001_initial_schema.sql
├── tests/
│   └── integration_test.rs
├── Cargo.toml
├── build.rs
├── Dockerfile
└── docker-compose.yml
```

---

## Getting Started

### Prerequisites
- Rust 1.82+
- Docker & Docker Compose
- `protoc` (Protocol Buffer compiler)

### Run locally with Docker

```bash
# Clone and enter project
cd wallet-engine

# Copy env template
cp .env.example .env

# Start all services
docker compose up --build

# gRPC server → localhost:50051
# RabbitMQ UI → localhost:15672  (guest/guest)
# Prometheus  → localhost:9091
```

### Run unit tests

```bash
cargo test
```

### Run integration tests (requires Docker for testcontainers)

```bash
cargo test --test integration_test
```

---

## Sample Transaction Flow

### 1. Create Wallets

```bash
grpcurl -plaintext -d '{
  "user_id": "550e8400-e29b-41d4-a716-446655440000",
  "currency": "USD"
}' localhost:50051 wallet.v1.WalletService/CreateWallet
```

### 2. Deposit

```bash
grpcurl -plaintext -d '{
  "idempotency_key": "dep-001-alice",
  "wallet_id": "<WALLET_ID>",
  "amount": { "amount": "1000.00", "currency": "USD" }
}' localhost:50051 wallet.v1.WalletService/Deposit
```

### 3. Transfer

```bash
grpcurl -plaintext -d '{
  "idempotency_key": "txfr-001",
  "from_wallet_id": "<ALICE_WALLET_ID>",
  "to_wallet_id":   "<BOB_WALLET_ID>",
  "amount": { "amount": "250.00", "currency": "USD" }
}' localhost:50051 wallet.v1.WalletService/Transfer
```

---

## Configuration Reference

All config is driven by environment variables prefixed `WALLET__`:

| Variable | Default | Description |
|---|---|---|
| `WALLET__SERVER__PORT` | `50051` | gRPC listen port |
| `WALLET__DATABASE__URL` | — | PostgreSQL connection string |
| `WALLET__DATABASE__MAX_CONNECTIONS` | `20` | Connection pool max |
| `WALLET__RABBITMQ__URL` | — | AMQP connection string |
| `WALLET__METRICS__PORT` | `9090` | Prometheus scrape port |
| `WALLET__RATE_LIMIT__REQUESTS_PER_SECOND` | `10000` | Global RPS cap |

---

## Performance Targets

| Metric | Target | Mechanism |
|---|---|---|
| Throughput | 10,000+ TPS | Connection pooling, async I/O, zero-copy |
| Latency (p99) | < 10ms | Pessimistic lock only within DB tx; no distributed lock |
| Memory safety | Guaranteed | Rust ownership model |
| Data races | Zero | No `unsafe`, no shared mutable state |

---

## Integration with Spring Boot Ecosystem

This service exposes a standard gRPC interface and publishes events to RabbitMQ.
Spring Boot consumers can:

1. **Call gRPC** using `net.devh:grpc-spring-boot-starter`
2. **Consume events** via `spring-amqp` / `spring-rabbit`
3. **Observe metrics** via Prometheus + Grafana (standard scrape)

No Spring-specific coupling exists in this service — it is a pure gRPC microservice.