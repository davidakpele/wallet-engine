// src/domain/entities.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{
    errors::DomainError,
    value_objects::{Currency, IdempotencyKey, Money, TransactionId, WalletId},
};

// ─── Enums ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "transaction_type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionType {
    Deposit,
    Withdrawal,
    Transfer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "transaction_status", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionStatus {
    Pending,
    Completed,
    Failed,
    RolledBack,
}

// ─── Wallet ───────────────────────────────────────────────────────────────────

/// Core wallet aggregate root.
/// All balance mutations are performed through domain methods,
/// never by direct field assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    id:         WalletId,
    user_id:    Uuid,
    balance:    Money,
    version:    i64,   // Optimistic concurrency version
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Wallet {
    /// Factory constructor — only way to create a new wallet.
    pub fn create(user_id: Uuid, currency: Currency) -> Self {
        let now = Utc::now();
        Self {
            id:         WalletId::new(),
            user_id,
            balance:    Money::zero(currency),
            version:    0,
            created_at: now,
            updated_at: now,
        }
    }

    /// Reconstitute from persistence (repository use only).
    pub fn reconstitute(
        id:         WalletId,
        user_id:    Uuid,
        balance:    Money,
        version:    i64,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self { id, user_id, balance, version, created_at, updated_at }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    pub fn id(&self)         -> WalletId          { self.id }
    pub fn user_id(&self)    -> Uuid              { self.user_id }
    pub fn balance(&self)    -> &Money            { &self.balance }
    pub fn version(&self)    -> i64               { self.version }
    pub fn created_at(&self) -> DateTime<Utc>     { self.created_at }
    pub fn updated_at(&self) -> DateTime<Utc>     { self.updated_at }

    // ── Domain Commands ───────────────────────────────────────────────────────

    /// Credit the wallet (deposit side of any transaction).
    pub fn credit(&mut self, amount: &Money) -> Result<(), DomainError> {
        self.balance = self.balance.checked_add(amount).map_err(|e| match e {
            DomainError::CurrencyMismatch { expected, got } =>
                DomainError::CurrencyMismatch { expected, got },
            other => other,
        })?;
        self.bump_version();
        Ok(())
    }

    /// Debit the wallet (withdrawal side of any transaction).
    pub fn debit(&mut self, amount: &Money) -> Result<(), DomainError> {
        self.balance = self.balance.checked_sub(amount).map_err(|e| match e {
            DomainError::InsufficientFunds { available, requested, .. } =>
                DomainError::InsufficientFunds {
                    wallet_id: self.id.inner(),
                    available,
                    requested,
                },
            other => other,
        })?;
        self.bump_version();
        Ok(())
    }

    fn bump_version(&mut self) {
        self.version += 1;
        self.updated_at = Utc::now();
    }
}

// ─── Transaction ──────────────────────────────────────────────────────────────

/// Transaction aggregate — immutable once Completed or Failed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    id:               TransactionId,
    wallet_id:        WalletId,
    to_wallet_id:     Option<WalletId>,   // Only for Transfer type
    amount:           Money,
    transaction_type: TransactionType,
    status:           TransactionStatus,
    idempotency_key:  IdempotencyKey,
    failure_reason:   Option<String>,
    created_at:       DateTime<Utc>,
    updated_at:       DateTime<Utc>,
}

impl Transaction {
    /// Create a new pending transaction.
    pub fn create(
        wallet_id:        WalletId,
        to_wallet_id:     Option<WalletId>,
        amount:           Money,
        transaction_type: TransactionType,
        idempotency_key:  IdempotencyKey,
    ) -> Result<Self, DomainError> {
        if !amount.is_positive() {
            return Err(DomainError::NonPositiveAmount);
        }
        if transaction_type == TransactionType::Transfer {
            if let Some(to) = to_wallet_id {
                if to == wallet_id {
                    return Err(DomainError::SelfTransfer);
                }
            }
        }
        let now = Utc::now();
        Ok(Self {
            id: TransactionId::new(),
            wallet_id,
            to_wallet_id,
            amount,
            transaction_type,
            status: TransactionStatus::Pending,
            idempotency_key,
            failure_reason: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Reconstitute from persistence.
    #[allow(clippy::too_many_arguments)]
    pub fn reconstitute(
        id:               TransactionId,
        wallet_id:        WalletId,
        to_wallet_id:     Option<WalletId>,
        amount:           Money,
        transaction_type: TransactionType,
        status:           TransactionStatus,
        idempotency_key:  IdempotencyKey,
        failure_reason:   Option<String>,
        created_at:       DateTime<Utc>,
        updated_at:       DateTime<Utc>,
    ) -> Self {
        Self {
            id, wallet_id, to_wallet_id, amount, transaction_type,
            status, idempotency_key, failure_reason, created_at, updated_at,
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    pub fn id(&self)               -> TransactionId        { self.id }
    pub fn wallet_id(&self)        -> WalletId             { self.wallet_id }
    pub fn to_wallet_id(&self)     -> Option<WalletId>     { self.to_wallet_id }
    pub fn amount(&self)           -> &Money               { &self.amount }
    pub fn transaction_type(&self) -> TransactionType      { self.transaction_type }
    pub fn status(&self)           -> TransactionStatus    { self.status }
    pub fn idempotency_key(&self)  -> &IdempotencyKey      { &self.idempotency_key }
    pub fn failure_reason(&self)   -> Option<&str>         { self.failure_reason.as_deref() }
    pub fn created_at(&self)       -> DateTime<Utc>        { self.created_at }
    pub fn updated_at(&self)       -> DateTime<Utc>        { self.updated_at }

    // ── State Transitions ─────────────────────────────────────────────────────

    pub fn complete(&mut self) -> Result<(), DomainError> {
        if self.status != TransactionStatus::Pending {
            return Err(DomainError::InvalidTransactionState(self.id.inner()));
        }
        self.status = TransactionStatus::Completed;
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn fail(&mut self, reason: impl Into<String>) -> Result<(), DomainError> {
        if self.status != TransactionStatus::Pending {
            return Err(DomainError::InvalidTransactionState(self.id.inner()));
        }
        self.status = TransactionStatus::Failed;
        self.failure_reason = Some(reason.into());
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<(), DomainError> {
        if self.status == TransactionStatus::RolledBack {
            return Err(DomainError::InvalidTransactionState(self.id.inner()));
        }
        self.status = TransactionStatus::RolledBack;
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            TransactionStatus::Completed
                | TransactionStatus::Failed
                | TransactionStatus::RolledBack
        )
    }
}