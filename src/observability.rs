// src/observability.rs
use metrics_exporter_prometheus::PrometheusBuilder;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialise structured logging with `tracing`.
/// In production, set RUST_LOG=wallet_engine=info
pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("wallet_engine=debug,tower_http=debug")
        }))
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}

/// Install the Prometheus metrics recorder and start the scrape endpoint.
pub fn init_metrics(host: &str, port: u16) {
    let addr: std::net::SocketAddr = format!("{}:{}", host, port).parse().unwrap();
    PrometheusBuilder::new()
        .with_http_listener(addr)
        .install()
        .expect("Failed to install Prometheus metrics recorder");

    tracing::info!(%addr, "Prometheus metrics available at /metrics");
}