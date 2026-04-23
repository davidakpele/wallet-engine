// src/application/dto.rs
//
// Data Transfer Objects for the application layer.
// These cross the boundary between interfaces and use-cases.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::entities::{Transaction, TransactionStatus, TransactionType, Wallet};

// ─── Commands (Inbound) ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositCommand {
    pub idempotency_key: String,
    pub wallet_id:       Uuid,
    pub amount:          String,
    pub currency:        String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalCommand {
    pub idempotency_key: String,
    pub wallet_id:       Uuid,
    pub amount:          String,
    pub currency:        String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferCommand {
    pub idempotency_key: String,
    pub from_wallet_id:  Uuid,
    pub to_wallet_id:    Uuid,
    pub amount:          String,
    pub currency:        String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWalletCommand {
    pub user_id:  Uuid,
    pub currency: String,
}

// ─── Results (Outbound) ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletDto {
    pub id:         Uuid,
    pub user_id:    Uuid,
    pub balance:    String,
    pub currency:   String,
    pub version:    i64,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&Wallet> for WalletDto {
    fn from(w: &Wallet) -> Self {
        Self {
            id:         w.id().inner(),
            user_id:    w.user_id(),
            balance:    w.balance().to_string_amount(),
            currency:   w.balance().currency().code().to_string(),
            version:    w.version(),
            created_at: w.created_at().to_rfc3339(),
            updated_at: w.updated_at().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionDto {
    pub id:               Uuid,
    pub wallet_id:        Uuid,
    pub to_wallet_id:     Option<Uuid>,
    pub amount:           String,
    pub currency:         String,
    pub transaction_type: TransactionType,
    pub status:           TransactionStatus,
    pub idempotency_key:  String,
    pub failure_reason:   Option<String>,
    pub created_at:       String,
    pub updated_at:       String,
}

impl From<&Transaction> for TransactionDto {
    fn from(t: &Transaction) -> Self {
        Self {
            id:               t.id().inner(),
            wallet_id:        t.wallet_id().inner(),
            to_wallet_id:     t.to_wallet_id().map(|id| id.inner()),
            amount:           t.amount().to_string_amount(),
            currency:         t.amount().currency().code().to_string(),
            transaction_type: t.transaction_type(),
            status:           t.status(),
            idempotency_key:  t.idempotency_key().value().to_string(),
            failure_reason:   t.failure_reason().map(str::to_string),
            created_at:       t.created_at().to_rfc3339(),
            updated_at:       t.updated_at().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositResult {
    pub transaction: TransactionDto,
    pub wallet:      WalletDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalResult {
    pub transaction: TransactionDto,
    pub wallet:      WalletDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResult {
    pub transaction:  TransactionDto,
    pub from_wallet:  WalletDto,
    pub to_wallet:    WalletDto,
}