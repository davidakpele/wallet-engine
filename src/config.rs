// src/config.rs
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server:   ServerConfig,
    pub database: DatabaseConfig,
    pub rabbitmq: RabbitMQConfig,
    pub metrics:  MetricsConfig,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url:             String,
    pub max_connections: u32,
    pub min_connections: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RabbitMQConfig {
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetricsConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, config::ConfigError> {
        dotenvy::dotenv().ok();

        config::Config::builder()
            .set_default("server.host", "0.0.0.0")?
            .set_default("server.port", 50051)?
            .set_default("database.max_connections", 20)?
            .set_default("database.min_connections", 2)?
            .set_default("metrics.host", "0.0.0.0")?
            .set_default("metrics.port", 9090)?
            .set_default("rate_limit.requests_per_second", 10000)?
            .add_source(
                config::Environment::default()
                    .separator("__")
                    .prefix("WALLET"),
            )
            .build()?
            .try_deserialize()
    }
}