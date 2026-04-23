// src/domain/repositories.rs
//
// These are the *ports* in Hexagonal Architecture — pure Rust traits with no
// infrastructure dependencies. The actual implementations live in
// `infrastructure/`.

use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::{
    entities::{Transaction, Wallet},
    errors::DomainError,
    value_objects::{IdempotencyKey, TransactionId, WalletId},
};

// ─── WalletRepository ─────────────────────────────────────────────────────────

#[async_trait]
pub trait WalletRepository: Send + Sync + 'static {
    /// Persist a newly-created wallet.
    async fn save(&self, wallet: &Wallet) -> Result<(), DomainError>;

    /// Find wallet by ID. Returns None if not found.
    async fn find_by_id(&self, id: WalletId) -> Result<Option<Wallet>, DomainError>;

    /// Find wallet by user ID.
    async fn find_by_user_id(&self, user_id: Uuid) -> Result<Option<Wallet>, DomainError>;

    /// Update wallet balance using optimistic locking (version check).
    /// Fails with `OptimisticLockConflict` if the persisted version differs.
    async fn update_balance(&self, wallet: &Wallet) -> Result<(), DomainError>;

    /// Acquire pessimistic row-level lock for the duration of a DB transaction.
    /// Returns the locked wallet.
    async fn find_for_update(&self, id: WalletId, tx: &mut sqlx::Transaction<'_, sqlx::Postgres>)
        -> Result<Wallet, DomainError>;
}

// ─── TransactionRepository ────────────────────────────────────────────────────

#[async_trait]
pub trait TransactionRepository: Send + Sync + 'static {
    /// Persist a new transaction.
    async fn save(&self, transaction: &Transaction) -> Result<(), DomainError>;

    /// Persist within an existing DB transaction (atomic with balance update).
    async fn save_in_tx(
        &self,
        transaction: &Transaction,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DomainError>;

    async fn find_by_id(&self, id: TransactionId) -> Result<Option<Transaction>, DomainError>;

    /// Look up by idempotency key to detect duplicates.
    async fn find_by_idempotency_key(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Option<Transaction>, DomainError>;

    /// Update transaction status.
    async fn update(
        &self,
        transaction: &Transaction,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DomainError>;
}

// ─── UnitOfWork ───────────────────────────────────────────────────────────────

/// Provides a transactional scope spanning both wallet and transaction updates.
/// This ensures atomic balance changes + transaction record in one DB commit.
#[async_trait]
pub trait UnitOfWork: Send + Sync + 'static {
    async fn begin(&self) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, DomainError>;
    async fn commit(tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), DomainError>;
    async fn rollback(tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), DomainError>;
}