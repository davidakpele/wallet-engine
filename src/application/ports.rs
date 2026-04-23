// src/application/ports.rs
use async_trait::async_trait;

use crate::domain::events::DomainEvent;
use crate::application::errors::ApplicationError;

/// Port for publishing domain events.
/// Implemented by `infrastructure::rabbitmq::RabbitMQPublisher`.
#[async_trait]
pub trait EventPublisher: Send + Sync + 'static {
    async fn publish(&self, event: DomainEvent) -> Result<(), ApplicationError>;
}