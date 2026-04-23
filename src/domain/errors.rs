// src/domain/errors.rs
use thiserror::Error;
use uuid::Uuid;

/// All domain-level errors. These are pure business-rule violations —
/// no infrastructure concerns leak into this layer.
#[derive(Debug, Error, Clone)]
pub enum DomainError {
    #[error("Insufficient funds: wallet {wallet_id} has {available}, requested {requested}")]
    InsufficientFunds {
        wallet_id: Uuid,
        available:  String,
        requested:  String,
    },

    #[error("Wallet {0} not found")]
    WalletNotFound(Uuid),

    #[error("Transaction {0} not found")]
    TransactionNotFound(Uuid),

    #[error("Currency mismatch: expected {expected}, got {got}")]
    CurrencyMismatch { expected: String, got: String },

    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    #[error("Duplicate transaction (idempotency key already used): {0}")]
    DuplicateTransaction(String),

    #[error("Transaction {0} is not in a state that allows this operation")]
    InvalidTransactionState(Uuid),

    #[error("Cannot transfer to the same wallet")]
    SelfTransfer,

    #[error("Optimistic lock conflict: wallet {0} was modified concurrently")]
    OptimisticLockConflict(Uuid),

    #[error("Amount must be positive")]
    NonPositiveAmount,

    #[error("Rate limit exceeded for wallet {0}")]
    RateLimitExceeded(Uuid),
}