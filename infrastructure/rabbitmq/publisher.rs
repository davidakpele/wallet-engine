// src/infrastructure/rabbitmq/publisher.rs
use async_trait::async_trait;
use backoff::{future::retry, ExponentialBackoff};
use lapin::{
    options::{BasicPublishOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::{
    application::{errors::ApplicationError, ports::EventPublisher},
    domain::events::DomainEvent,
};

const EXCHANGE_NAME:  &str = "wallet.events";
const DLX_NAME:       &str = "wallet.events.dlx";
const DLQ_NAME:       &str = "wallet.events.dlq";

pub struct RabbitMQPublisher {
    channel: Arc<Mutex<Option<Channel>>>,
    amqp_url: String,
}

impl RabbitMQPublisher {
    pub async fn new(amqp_url: impl Into<String>) -> Result<Self, ApplicationError> {
        let amqp_url = amqp_url.into();
        let publisher = Self {
            channel:  Arc::new(Mutex::new(None)),
            amqp_url: amqp_url.clone(),
        };
        publisher.connect().await?;
        Ok(publisher)
    }

    async fn connect(&self) -> Result<(), ApplicationError> {
        let conn = Connection::connect(&self.amqp_url, ConnectionProperties::default())
            .await
            .map_err(|e| ApplicationError::infrastructure(format!("RabbitMQ connect: {e}")))?;

        let channel = conn
            .create_channel()
            .await
            .map_err(|e| ApplicationError::infrastructure(format!("Create channel: {e}")))?;

        // Declare Dead-Letter Exchange
        channel
            .exchange_declare(
                DLX_NAME,
                ExchangeKind::Fanout,
                ExchangeDeclareOptions { durable: true, ..Default::default() },
                FieldTable::default(),
            )
            .await
            .map_err(|e| ApplicationError::infrastructure(format!("DLX declare: {e}")))?;

        // Declare Dead-Letter Queue
        channel
            .queue_declare(
                DLQ_NAME,
                QueueDeclareOptions { durable: true, ..Default::default() },
                FieldTable::default(),
            )
            .await
            .map_err(|e| ApplicationError::infrastructure(format!("DLQ declare: {e}")))?;

        channel
            .queue_bind(DLQ_NAME, DLX_NAME, "", QueueBindOptions::default(), FieldTable::default())
            .await
            .map_err(|e| ApplicationError::infrastructure(format!("DLQ bind: {e}")))?;

        // Declare main topic exchange with DLX fallback
        let mut args = FieldTable::default();
        args.insert(
            "x-dead-letter-exchange".into(),
            lapin::types::AMQPValue::LongString(DLX_NAME.into()),
        );

        channel
            .exchange_declare(
                EXCHANGE_NAME,
                ExchangeKind::Topic,
                ExchangeDeclareOptions { durable: true, ..Default::default() },
                args,
            )
            .await
            .map_err(|e| ApplicationError::infrastructure(format!("Exchange declare: {e}")))?;

        *self.channel.lock().await = Some(channel);
        info!("RabbitMQ channel ready");
        Ok(())
    }

    async fn get_channel(&self) -> Result<tokio::sync::MutexGuard<'_, Option<Channel>>, ApplicationError> {
        let guard = self.channel.lock().await;
        if guard.is_none() {
            drop(guard);
            self.connect().await?;
            Ok(self.channel.lock().await)
        } else {
            Ok(guard)
        }
    }
}

#[async_trait]
impl EventPublisher for RabbitMQPublisher {
    async fn publish(&self, event: DomainEvent) -> Result<(), ApplicationError> {
        let routing_key = event.routing_key();
        let payload = serde_json::to_vec(&event)
            .map_err(|e| ApplicationError::infrastructure(format!("Serialize event: {e}")))?;

        let backoff = ExponentialBackoff {
            max_elapsed_time: Some(Duration::from_secs(5)),
            ..Default::default()
        };

        retry(backoff, || async {
            let guard = self.get_channel().await.map_err(backoff::Error::Permanent)?;
            let channel = guard.as_ref().ok_or_else(|| {
                backoff::Error::Transient {
                    err: ApplicationError::infrastructure("No channel available"),
                    retry_after: None,
                }
            })?;

            let props = BasicProperties::default()
                .with_content_type("application/json".into())
                .with_delivery_mode(2) // Persistent
                .with_message_id(event.event_id().to_string().into())
                .with_timestamp(event.occurred_at().timestamp() as u64);

            channel
                .basic_publish(
                    EXCHANGE_NAME,
                    routing_key,
                    BasicPublishOptions::default(),
                    &payload,
                    props,
                )
                .await
                .map_err(|e| {
                    warn!(error = %e, "Publish failed, will retry");
                    backoff::Error::Transient {
                        err: ApplicationError::EventPublish(e.to_string()),
                        retry_after: None,
                    }
                })?;

            Ok(())
        })
        .await
        .map_err(|e| {
            error!(error = %e, routing_key, "Failed to publish event after retries");
            ApplicationError::EventPublish(e.to_string())
        })?;

        Ok(())
    }
}