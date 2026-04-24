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
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = AppConfig::from_env().expect("Failed to load configuration");
    observability::init_tracing();
    observability::init_metrics(&cfg.metrics.host, cfg.metrics.port);

    info!("Starting wallet-engine v{}", env!("CARGO_PKG_VERSION"));

    let pool = Arc::new(
        PgPoolOptions::new()
            .max_connections(cfg.database.max_connections)
            .min_connections(cfg.database.min_connections)
            .connect(&cfg.database.url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to database: {e}"))?,
    );

    sqlx::migrate!("./migrations")
        .run(pool.as_ref())
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;
    info!("Database migrations applied");

    let wallet_repo      = Arc::new(PgWalletRepository::new(Arc::clone(&pool)));
    let transaction_repo = Arc::new(PgTransactionRepository::new(Arc::clone(&pool)));
    let event_publisher  = Arc::new(
        RabbitMQPublisher::new(&cfg.rabbitmq.url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to RabbitMQ: {e}"))?,
    );

    let service = Arc::new(TransactionService::new(
        wallet_repo,
        transaction_repo,
        event_publisher,
        Arc::clone(&pool),
    ));

    let grpc_handler = WalletGrpcHandler::new(Arc::clone(&service));

    let addr = format!("{}:{}", cfg.server.host, cfg.server.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid server address: {e}"))?;

    info!(%addr, "gRPC server listening");

    Server::builder()
        .trace_fn(|_| tracing::info_span!("grpc_request"))
        .add_service(WalletServiceServer::new(grpc_handler))
        .serve_with_shutdown(addr, shutdown_signal())
        .await?;

    info!("Server shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    // SIGTERM is Unix-only. On Windows (local dev) we only handle Ctrl-C.
    #[cfg(unix)]
    {
        use tokio::signal;
        let sigterm = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to register SIGTERM handler")
                .recv()
                .await;
        };
        tokio::select! {
            _ = sigterm             => info!("Received SIGTERM, shutting down"),
            _ = signal::ctrl_c()   => info!("Received Ctrl-C, shutting down"),
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to register Ctrl-C handler");
        info!("Received Ctrl-C, shutting down");
    }
}