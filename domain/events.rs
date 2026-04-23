// src/domain/events.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::entities::{TransactionStatus, TransactionType};
use crate::domain::value_objects::{TransactionId, WalletId};

/// All domain events emitted after state changes.
/// Published to RabbitMQ for downstream consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DomainEvent {
    TransactionCreated(TransactionCreatedEvent),
    TransactionCompleted(TransactionCompletedEvent),
    TransactionFailed(TransactionFailedEvent),
    TransactionRolledBack(TransactionRolledBackEvent),
    WalletCreated(WalletCreatedEvent),
}

impl DomainEvent {
    pub fn event_id(&self) -> Uuid {
        match self {
            Self::TransactionCreated(e)   => e.event_id,
            Self::TransactionCompleted(e) => e.event_id,
            Self::TransactionFailed(e)    => e.event_id,
            Self::TransactionRolledBack(e) => e.event_id,
            Self::WalletCreated(e)        => e.event_id,
        }
    }

    pub fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            Self::TransactionCreated(e)   => e.occurred_at,
            Self::TransactionCompleted(e) => e.occurred_at,
            Self::TransactionFailed(e)    => e.occurred_at,
            Self::TransactionRolledBack(e) => e.occurred_at,
            Self::WalletCreated(e)        => e.occurred_at,
        }
    }

    pub fn routing_key(&self) -> &'static str {
        match self {
            Self::TransactionCreated(_)    => "transaction.created",
            Self::TransactionCompleted(_)  => "transaction.completed",
            Self::TransactionFailed(_)     => "transaction.failed",
            Self::TransactionRolledBack(_) => "transaction.rolledback",
            Self::WalletCreated(_)         => "wallet.created",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionCreatedEvent {
    pub event_id:         Uuid,
    pub transaction_id:   TransactionId,
    pub wallet_id:        WalletId,
    pub to_wallet_id:     Option<WalletId>,
    pub amount:           String,
    pub currency:         String,
    pub transaction_type: TransactionType,
    pub idempotency_key:  String,
    pub occurred_at:      DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionCompletedEvent {
    pub event_id:         Uuid,
    pub transaction_id:   TransactionId,
    pub wallet_id:        WalletId,
    pub amount:           String,
    pub currency:         String,
    pub transaction_type: TransactionType,
    pub new_balance:      String,
    pub occurred_at:      DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionFailedEvent {
    pub event_id:       Uuid,
    pub transaction_id: TransactionId,
    pub wallet_id:      WalletId,
    pub reason:         String,
    pub occurred_at:    DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRolledBackEvent {
    pub event_id:       Uuid,
    pub transaction_id: TransactionId,
    pub wallet_id:      WalletId,
    pub occurred_at:    DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletCreatedEvent {
    pub event_id:    Uuid,
    pub wallet_id:   WalletId,
    pub user_id:     Uuid,
    pub currency:    String,
    pub occurred_at: DateTime<Utc>,
}