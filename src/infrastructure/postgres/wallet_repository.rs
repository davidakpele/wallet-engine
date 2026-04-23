// src/infrastructure/postgres/wallet_repository.rs
use async_trait::async_trait;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::{
    entities::Wallet,
    errors::DomainError,
    repositories::WalletRepository,
    value_objects::{Currency, Money, WalletId},
};

pub struct PgWalletRepository {
    pool: Arc<PgPool>,
}

impl PgWalletRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    fn row_to_wallet(row: &sqlx::postgres::PgRow) -> Result<Wallet, DomainError> {
        let id:       Uuid    = row.try_get("id").map_err(|e| DomainError::InvalidAmount(e.to_string()))?;
        let user_id:  Uuid    = row.try_get("user_id").map_err(|e| DomainError::InvalidAmount(e.to_string()))?;
        let balance:  Decimal = row.try_get("balance").map_err(|e| DomainError::InvalidAmount(e.to_string()))?;
        let currency: String  = row.try_get("currency").map_err(|e| DomainError::InvalidAmount(e.to_string()))?;
        let version:  i64     = row.try_get("version").map_err(|e| DomainError::InvalidAmount(e.to_string()))?;
        let created_at        = row.try_get("created_at").map_err(|e| DomainError::InvalidAmount(e.to_string()))?;
        let updated_at        = row.try_get("updated_at").map_err(|e| DomainError::InvalidAmount(e.to_string()))?;

        let currency_obj = Currency::new(&currency)?;
        let money = Money::new(balance, currency_obj)?;

        Ok(Wallet::reconstitute(
            WalletId::from(id),
            user_id,
            money,
            version,
            created_at,
            updated_at,
        ))
    }
}

#[async_trait]
impl WalletRepository for PgWalletRepository {
    async fn save(&self, wallet: &Wallet) -> Result<(), DomainError> {
        sqlx::query!(
            r#"
            INSERT INTO wallets (id, user_id, balance, currency, version, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            wallet.id().inner(),
            wallet.user_id(),
            wallet.balance().amount(),
            wallet.balance().currency().code(),
            wallet.version(),
            wallet.created_at(),
            wallet.updated_at(),
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Insert wallet: {e}")))?;

        Ok(())
    }

    async fn find_by_id(&self, id: WalletId) -> Result<Option<Wallet>, DomainError> {
        let row = sqlx::query(
            "SELECT id, user_id, balance, currency, version, created_at, updated_at
               FROM wallets WHERE id = $1",
        )
        .bind(id.inner())
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Find wallet: {e}")))?;

        row.map(|r| Self::row_to_wallet(&r)).transpose()
    }

    async fn find_by_user_id(&self, user_id: Uuid) -> Result<Option<Wallet>, DomainError> {
        let row = sqlx::query(
            "SELECT id, user_id, balance, currency, version, created_at, updated_at
               FROM wallets WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Find wallet by user: {e}")))?;

        row.map(|r| Self::row_to_wallet(&r)).transpose()
    }

    async fn update_balance(&self, wallet: &Wallet) -> Result<(), DomainError> {
        let rows_affected = sqlx::query!(
            r#"
            UPDATE wallets
               SET balance    = $1,
                   version    = version + 1,
                   updated_at = NOW()
             WHERE id      = $2
               AND version = $3
            "#,
            wallet.balance().amount(),
            wallet.id().inner(),
            wallet.version() - 1, // Optimistic lock: check pre-update version
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Update wallet: {e}")))?
        .rows_affected();

        if rows_affected == 0 {
            return Err(DomainError::OptimisticLockConflict(wallet.id().inner()));
        }

        Ok(())
    }

    async fn find_for_update(
        &self,
        id: WalletId,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Wallet, DomainError> {
        // SELECT ... FOR UPDATE: row-level pessimistic lock
        let row = sqlx::query(
            "SELECT id, user_id, balance, currency, version, created_at, updated_at
               FROM wallets
              WHERE id = $1
                FOR UPDATE",
        )
        .bind(id.inner())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| DomainError::InvalidAmount(format!("Lock wallet: {e}")))?
        .ok_or(DomainError::WalletNotFound(id.inner()))?;

        Self::row_to_wallet(&row)
    }
}