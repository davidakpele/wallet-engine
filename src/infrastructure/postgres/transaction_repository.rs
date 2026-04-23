// src/infrastructure/postgres/transaction_repository.rs
use async_trait::async_trait;
use rust_decimal::Decimal;
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::{
    entities::{Transaction, TransactionStatus, TransactionType},
    errors::DomainError,
    repositories::TransactionRepository,
    value_objects::{Currency, IdempotencyKey, Money, TransactionId, WalletId},
};

pub struct PgTransactionRepository {
    pool: Arc<sqlx::PgPool>,
}

impl PgTransactionRepository {
    pub fn new(pool: Arc<sqlx::PgPool>) -> Self {
        Self { pool }
    }

    fn row_to_transaction(row: &sqlx::postgres::PgRow) -> Result<Transaction, DomainError> {
        let id:               Uuid            = row.try_get("id").unwrap();
        let wallet_id:        Uuid            = row.try_get("wallet_id").unwrap();
        let to_wallet_id:     Option<Uuid>    = row.try_get("to_wallet_id").unwrap();
        let amount:           Decimal         = row.try_get("amount").unwrap();
        let currency:         String          = row.try_get("currency").unwrap();
        let transaction_type: TransactionType = row.try_get("transaction_type").unwrap();
        let status:           TransactionStatus = row.try_get("status").unwrap();
        let idempotency_key:  String          = row.try_get("idempotency_key").unwrap();
        let failure_reason:   Option<String>  = row.try_get("failure_reason").unwrap();
        let created_at                        = row.try_get("created_at").unwrap();
        let updated_at                        = row.try_get("updated_at").unwrap();

        let money = Money::new(amount, Currency::new(&currency)?)?;

        Ok(Transaction::reconstitute(
            TransactionId::from(id),
            WalletId::from(wallet_id),
            to_wallet_id.map(WalletId::from),
            money,
            transaction_type,
            status,
            IdempotencyKey::new(idempotency_key)?,
            failure_reason,
            created_at,
            updated_at,
        ))
    }
}

#[async_trait]
impl TransactionRepository for PgTransactionRepository {
    async fn save(&self, transaction: &Transaction) -> Result<(), DomainError> {
        sqlx::query!(
            r#"
            INSERT INTO transactions
                (id, wallet_id, to_wallet_id, amount, currency, transaction_type,
                 status, idempotency_key, failure_reason, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6::transaction_type, $7::transaction_status,
                    $8, $9, $10, $11)
            ON CONFLICT (idempotency_key) DO NOTHING
            "#,
            transaction.id().inner(),
            transaction.wallet_id().inner(),
            transaction.to_wallet_id().map(|id| id.inner()),
            transaction.amount().amount(),
            transaction.amount().currency().code(),
            transaction.transaction_type() as TransactionType,
            transaction.status() as TransactionStatus,
            transaction.idempotency_key().value(),
            transaction.failure_reason(),
            transaction.created_at(),
            transaction.updated_at(),
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Insert transaction: {e}")))?;

        Ok(())
    }

    async fn save_in_tx(
        &self,
        transaction: &Transaction,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DomainError> {
        sqlx::query!(
            r#"
            INSERT INTO transactions
                (id, wallet_id, to_wallet_id, amount, currency, transaction_type,
                 status, idempotency_key, failure_reason, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6::transaction_type, $7::transaction_status,
                    $8, $9, $10, $11)
            ON CONFLICT (idempotency_key) DO NOTHING
            "#,
            transaction.id().inner(),
            transaction.wallet_id().inner(),
            transaction.to_wallet_id().map(|id| id.inner()),
            transaction.amount().amount(),
            transaction.amount().currency().code(),
            transaction.transaction_type() as TransactionType,
            transaction.status() as TransactionStatus,
            transaction.idempotency_key().value(),
            transaction.failure_reason(),
            transaction.created_at(),
            transaction.updated_at(),
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Insert transaction in tx: {e}")))?;

        Ok(())
    }

    async fn find_by_id(&self, id: TransactionId) -> Result<Option<Transaction>, DomainError> {
        let row = sqlx::query(
            r#"SELECT id, wallet_id, to_wallet_id, amount, currency,
                      transaction_type, status, idempotency_key,
                      failure_reason, created_at, updated_at
                 FROM transactions WHERE id = $1"#,
        )
        .bind(id.inner())
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Find transaction: {e}")))?;

        row.map(|r| Self::row_to_transaction(&r)).transpose()
    }

    async fn find_by_idempotency_key(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Option<Transaction>, DomainError> {
        let row = sqlx::query(
            r#"SELECT id, wallet_id, to_wallet_id, amount, currency,
                      transaction_type, status, idempotency_key,
                      failure_reason, created_at, updated_at
                 FROM transactions WHERE idempotency_key = $1"#,
        )
        .bind(key.value())
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Idempotency lookup: {e}")))?;

        row.map(|r| Self::row_to_transaction(&r)).transpose()
    }

    async fn update(
        &self,
        transaction: &Transaction,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DomainError> {
        sqlx::query!(
            r#"
            UPDATE transactions
               SET status         = $1::transaction_status,
                   failure_reason = $2,
                   updated_at     = NOW()
             WHERE id = $3
            "#,
            transaction.status() as TransactionStatus,
            transaction.failure_reason(),
            transaction.id().inner(),
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Update transaction: {e}")))?;

        Ok(())
    }
}