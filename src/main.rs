// src/main.rs

use std::sync::Arc;
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing::info;

mod application;
mod config;
mod domain;
mod infrastructure;
mod interfaces;
mod observability;

use application::service::TransactionService;
use config::AppConfig;
use infrastructure::{
    postgres::{PgTransactionRepository, PgWalletRepository},
    rabbitmq::RabbitMQPublisher,
};
use interfaces::grpc::{
    handler::{proto::wallet_service_server::WalletServiceServer, WalletGrpcHandler},
    middleware::build_rate_limiter,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Config & Observability ────────────────────────────────────────────────
    let cfg = AppConfig::from_env().expect("Failed to load configuration");
    observability::init_tracing();
    observability::init_metrics(&cfg.metrics.host, cfg.metrics.port);

    info!("Starting wallet-engine v{}", env!("CARGO_PKG_VERSION"));

    // ── Database ──────────────────────────────────────────────────────────────
    let pool = Arc::new(
        PgPoolOptions::new()
            .max_connections(cfg.database.max_connections)
            .min_connections(cfg.database.min_connections)
            .connect(&cfg.database.url)
            .await?,
    );

    // Run pending migrations
    sqlx::migrate!("./migrations").run(pool.as_ref()).await?;
    info!("Database migrations applied");

    // ── Infrastructure ────────────────────────────────────────────────────────
    let wallet_repo      = Arc::new(PgWalletRepository::new(Arc::clone(&pool)));
    let transaction_repo = Arc::new(PgTransactionRepository::new(Arc::clone(&pool)));
    let event_publisher  = Arc::new(RabbitMQPublisher::new(&cfg.rabbitmq.url).await?);

    // ── Application ───────────────────────────────────────────────────────────
    let service = Arc::new(TransactionService::new(
        wallet_repo,
        transaction_repo,
        event_publisher,
        Arc::clone(&pool),
    ));

    // ── gRPC Server ───────────────────────────────────────────────────────────
    let _rate_limiter = build_rate_limiter(cfg.rate_limit.requests_per_second);
    let grpc_handler  = WalletGrpcHandler::new(Arc::clone(&service));

    let addr = format!("{}:{}", cfg.server.host, cfg.server.port).parse()?;

    info!(%addr, "gRPC server listening");

    Server::builder()
        .trace_fn(|_| tracing::info_span!("grpc_request"))
        .add_service(WalletServiceServer::new(grpc_handler))
        .serve(addr)
        .await?;

    Ok(())
}