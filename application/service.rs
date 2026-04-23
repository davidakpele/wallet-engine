// src/application/service.rs
//
// The application service is the transaction script that orchestrates domain
// objects, repositories, and event publishing.  It contains ZERO infrastructure
// dependencies — only trait objects.

use std::sync::Arc;

use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::{
    application::{
        dto::{
            CreateWalletCommand, DepositCommand, DepositResult, TransactionDto,
            TransferCommand, TransferResult, WalletDto, WithdrawalCommand, WithdrawalResult,
        },
        errors::ApplicationError,
        ports::EventPublisher,
    },
    domain::{
        entities::{Transaction, TransactionType, Wallet},
        errors::DomainError,
        events::{
            DomainEvent, TransactionCompletedEvent, TransactionCreatedEvent,
            TransactionFailedEvent, WalletCreatedEvent,
        },
        repositories::{TransactionRepository, WalletRepository},
        value_objects::{Currency, IdempotencyKey, Money, TransactionId, WalletId},
    },
};

// ─── TransactionService ───────────────────────────────────────────────────────

pub struct TransactionService {
    wallet_repo:      Arc<dyn WalletRepository>,
    transaction_repo: Arc<dyn TransactionRepository>,
    event_publisher:  Arc<dyn EventPublisher>,
    db_pool:          Arc<sqlx::PgPool>,
}

impl TransactionService {
    pub fn new(
        wallet_repo:      Arc<dyn WalletRepository>,
        transaction_repo: Arc<dyn TransactionRepository>,
        event_publisher:  Arc<dyn EventPublisher>,
        db_pool:          Arc<sqlx::PgPool>,
    ) -> Self {
        Self { wallet_repo, transaction_repo, event_publisher, db_pool }
    }

    // ── Create Wallet ──────────────────────────────────────────────────────────

    #[instrument(skip(self), fields(user_id = %cmd.user_id))]
    pub async fn create_wallet(
        &self,
        cmd: CreateWalletCommand,
    ) -> Result<WalletDto, ApplicationError> {
        let currency = Currency::new(&cmd.currency)?;
        let wallet = Wallet::create(cmd.user_id, currency);

        self.wallet_repo.save(&wallet).await?;

        let event = DomainEvent::WalletCreated(WalletCreatedEvent {
            event_id:    Uuid::new_v4(),
            wallet_id:   wallet.id(),
            user_id:     wallet.user_id(),
            currency:    wallet.balance().currency().code().to_string(),
            occurred_at: wallet.created_at(),
        });
        self.publish_event_best_effort(event).await;

        info!(wallet_id = %wallet.id(), "Wallet created");
        Ok(WalletDto::from(&wallet))
    }

    // ── Deposit ───────────────────────────────────────────────────────────────

    /// Process a deposit into a wallet.
    ///
    /// Guarantees:
    /// - Idempotent (same key → same response, no duplicate balance change)
    /// - Atomic  (balance update + transaction record in one DB transaction)
    /// - Event   (TransactionCreated, TransactionCompleted)
    #[instrument(skip(self), fields(
        idempotency_key = %cmd.idempotency_key,
        wallet_id        = %cmd.wallet_id,
        amount           = %cmd.amount,
    ))]
    pub async fn deposit(
        &self,
        cmd: DepositCommand,
    ) -> Result<DepositResult, ApplicationError> {
        let idempotency_key = IdempotencyKey::new(&cmd.idempotency_key)
            .map_err(ApplicationError::Domain)?;

        // ── Idempotency check ────────────────────────────────────────────────
        if let Some(existing) = self
            .transaction_repo
            .find_by_idempotency_key(&idempotency_key)
            .await?
        {
            warn!(transaction_id = %existing.id(), "Duplicate deposit request — returning cached result");
            let wallet = self
                .wallet_repo
                .find_by_id(existing.wallet_id())
                .await?
                .ok_or_else(|| ApplicationError::Domain(DomainError::WalletNotFound(
                    existing.wallet_id().inner(),
                )))?;
            return Ok(DepositResult {
                transaction: TransactionDto::from(&existing),
                wallet:      WalletDto::from(&wallet),
            });
        }

        let wallet_id = WalletId::from(cmd.wallet_id);
        let currency  = Currency::new(&cmd.currency)?;
        let amount    = Money::from_str_amount(&cmd.amount, currency)?;

        // ── Create pending transaction record ────────────────────────────────
        let mut txn = Transaction::create(
            wallet_id,
            None,
            amount.clone(),
            TransactionType::Deposit,
            idempotency_key.clone(),
        )?;

        // ── Execute atomically in a DB transaction ───────────────────────────
        let mut db_tx = self.db_pool.begin().await.map_err(|e| {
            ApplicationError::infrastructure(format!("Failed to begin DB transaction: {e}"))
        })?;

        // Publish creation event before DB writes (fire-and-forget)
        self.publish_event_best_effort(DomainEvent::TransactionCreated(
            TransactionCreatedEvent {
                event_id:         Uuid::new_v4(),
                transaction_id:   txn.id(),
                wallet_id:        txn.wallet_id(),
                to_wallet_id:     None,
                amount:           amount.to_string_amount(),
                currency:         amount.currency().code().to_string(),
                transaction_type: TransactionType::Deposit,
                idempotency_key:  idempotency_key.value().to_string(),
                occurred_at:      txn.created_at(),
            },
        ))
        .await;

        // Lock the wallet row for update
        let mut wallet = self
            .wallet_repo
            .find_for_update(wallet_id, &mut db_tx)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to lock wallet");
                ApplicationError::Domain(e)
            })?;

        // Apply domain logic
        let result = wallet.credit(&amount);

        match result {
            Ok(()) => {
                txn.complete().map_err(ApplicationError::Domain)?;
                // Persist both atomically
                self.transaction_repo.save_in_tx(&txn, &mut db_tx).await?;
                self.persist_wallet_in_tx(&wallet, &mut db_tx).await?;

                db_tx.commit().await.map_err(|e| {
                    ApplicationError::infrastructure(format!("DB commit failed: {e}"))
                })?;

                self.publish_event_best_effort(DomainEvent::TransactionCompleted(
                    TransactionCompletedEvent {
                        event_id:         Uuid::new_v4(),
                        transaction_id:   txn.id(),
                        wallet_id:        wallet.id(),
                        amount:           amount.to_string_amount(),
                        currency:         amount.currency().code().to_string(),
                        transaction_type: TransactionType::Deposit,
                        new_balance:      wallet.balance().to_string_amount(),
                        occurred_at:      txn.updated_at(),
                    },
                ))
                .await;

                info!(
                    transaction_id = %txn.id(),
                    wallet_id      = %wallet.id(),
                    new_balance    = %wallet.balance(),
                    "Deposit completed"
                );

                Ok(DepositResult {
                    transaction: TransactionDto::from(&txn),
                    wallet:      WalletDto::from(&wallet),
                })
            }
            Err(domain_err) => {
                txn.fail(domain_err.to_string())
                    .map_err(ApplicationError::Domain)?;
                // Persist the failed transaction so we can return idempotent response
                let _ = self.transaction_repo.save_in_tx(&txn, &mut db_tx).await;
                let _ = db_tx.commit().await;

                self.publish_event_best_effort(DomainEvent::TransactionFailed(
                    TransactionFailedEvent {
                        event_id:       Uuid::new_v4(),
                        transaction_id: txn.id(),
                        wallet_id:      wallet.id(),
                        reason:         domain_err.to_string(),
                        occurred_at:    txn.updated_at(),
                    },
                ))
                .await;

                error!(error = %domain_err, "Deposit failed");
                Err(ApplicationError::Domain(domain_err))
            }
        }
    }

    // ── Withdrawal ────────────────────────────────────────────────────────────

    #[instrument(skip(self), fields(
        idempotency_key = %cmd.idempotency_key,
        wallet_id        = %cmd.wallet_id,
        amount           = %cmd.amount,
    ))]
    pub async fn withdraw(
        &self,
        cmd: WithdrawalCommand,
    ) -> Result<WithdrawalResult, ApplicationError> {
        let idempotency_key = IdempotencyKey::new(&cmd.idempotency_key)?;

        // Idempotency check
        if let Some(existing) = self
            .transaction_repo
            .find_by_idempotency_key(&idempotency_key)
            .await?
        {
            warn!(transaction_id = %existing.id(), "Duplicate withdrawal — returning cached");
            let wallet = self
                .wallet_repo
                .find_by_id(existing.wallet_id())
                .await?
                .ok_or_else(|| ApplicationError::Domain(
                    DomainError::WalletNotFound(existing.wallet_id().inner()),
                ))?;
            return Ok(WithdrawalResult {
                transaction: TransactionDto::from(&existing),
                wallet:      WalletDto::from(&wallet),
            });
        }

        let wallet_id = WalletId::from(cmd.wallet_id);
        let currency  = Currency::new(&cmd.currency)?;
        let amount    = Money::from_str_amount(&cmd.amount, currency)?;

        let mut txn = Transaction::create(
            wallet_id,
            None,
            amount.clone(),
            TransactionType::Withdrawal,
            idempotency_key,
        )?;

        let mut db_tx = self.db_pool.begin().await.map_err(|e| {
            ApplicationError::infrastructure(format!("Begin DB tx: {e}"))
        })?;

        let mut wallet = self
            .wallet_repo
            .find_for_update(wallet_id, &mut db_tx)
            .await?;

        match wallet.debit(&amount) {
            Ok(()) => {
                txn.complete().map_err(ApplicationError::Domain)?;
                self.transaction_repo.save_in_tx(&txn, &mut db_tx).await?;
                self.persist_wallet_in_tx(&wallet, &mut db_tx).await?;
                db_tx.commit().await.map_err(|e| {
                    ApplicationError::infrastructure(format!("Commit: {e}"))
                })?;

                info!(
                    transaction_id = %txn.id(),
                    new_balance    = %wallet.balance(),
                    "Withdrawal completed"
                );

                Ok(WithdrawalResult {
                    transaction: TransactionDto::from(&txn),
                    wallet:      WalletDto::from(&wallet),
                })
            }
            Err(e) => {
                txn.fail(e.to_string()).ok();
                let _ = self.transaction_repo.save_in_tx(&txn, &mut db_tx).await;
                let _ = db_tx.commit().await;
                Err(ApplicationError::Domain(e))
            }
        }
    }

    // ── Transfer ──────────────────────────────────────────────────────────────

    /// Double-entry transfer: debit source, credit destination atomically.
    #[instrument(skip(self), fields(
        idempotency_key = %cmd.idempotency_key,
        from_wallet_id   = %cmd.from_wallet_id,
        to_wallet_id     = %cmd.to_wallet_id,
        amount           = %cmd.amount,
    ))]
    pub async fn transfer(
        &self,
        cmd: TransferCommand,
    ) -> Result<TransferResult, ApplicationError> {
        if cmd.from_wallet_id == cmd.to_wallet_id {
            return Err(ApplicationError::Domain(DomainError::SelfTransfer));
        }

        let idempotency_key = IdempotencyKey::new(&cmd.idempotency_key)?;

        // Idempotency check
        if let Some(existing) = self
            .transaction_repo
            .find_by_idempotency_key(&idempotency_key)
            .await?
        {
            warn!("Duplicate transfer — returning cached");
            let from_wallet = self
                .wallet_repo
                .find_by_id(existing.wallet_id())
                .await?
                .ok_or_else(|| ApplicationError::Domain(
                    DomainError::WalletNotFound(existing.wallet_id().inner()),
                ))?;
            let to_wallet_id = existing
                .to_wallet_id()
                .ok_or_else(|| ApplicationError::internal("Missing to_wallet_id on transfer"))?;
            let to_wallet = self
                .wallet_repo
                .find_by_id(to_wallet_id)
                .await?
                .ok_or_else(|| ApplicationError::Domain(
                    DomainError::WalletNotFound(to_wallet_id.inner()),
                ))?;
            return Ok(TransferResult {
                transaction: TransactionDto::from(&existing),
                from_wallet: WalletDto::from(&from_wallet),
                to_wallet:   WalletDto::from(&to_wallet),
            });
        }

        let from_id  = WalletId::from(cmd.from_wallet_id);
        let to_id    = WalletId::from(cmd.to_wallet_id);
        let currency = Currency::new(&cmd.currency)?;
        let amount   = Money::from_str_amount(&cmd.amount, currency)?;

        let mut txn = Transaction::create(
            from_id,
            Some(to_id),
            amount.clone(),
            TransactionType::Transfer,
            idempotency_key,
        )?;

        let mut db_tx = self.db_pool.begin().await.map_err(|e| {
            ApplicationError::infrastructure(format!("Begin DB tx: {e}"))
        })?;

        // Lock both wallets in a consistent order to avoid deadlocks:
        // always lock the lower UUID first.
        let (first_id, second_id) = if from_id.inner() < to_id.inner() {
            (from_id, to_id)
        } else {
            (to_id, from_id)
        };

        let mut first  = self.wallet_repo.find_for_update(first_id, &mut db_tx).await?;
        let mut second = self.wallet_repo.find_for_update(second_id, &mut db_tx).await?;

        let (from_wallet, to_wallet) = if from_id.inner() < to_id.inner() {
            (&mut first, &mut second)
        } else {
            (&mut second, &mut first)
        };

        let debit_result  = from_wallet.debit(&amount);
        let credit_result = to_wallet.credit(&amount);

        match (debit_result, credit_result) {
            (Ok(()), Ok(())) => {
                txn.complete().map_err(ApplicationError::Domain)?;
                self.transaction_repo.save_in_tx(&txn, &mut db_tx).await?;
                self.persist_wallet_in_tx(from_wallet, &mut db_tx).await?;
                self.persist_wallet_in_tx(to_wallet, &mut db_tx).await?;
                db_tx.commit().await.map_err(|e| {
                    ApplicationError::infrastructure(format!("Commit: {e}"))
                })?;

                info!(
                    transaction_id = %txn.id(),
                    from_balance   = %from_wallet.balance(),
                    to_balance     = %to_wallet.balance(),
                    "Transfer completed"
                );

                Ok(TransferResult {
                    transaction: TransactionDto::from(&txn),
                    from_wallet: WalletDto::from(from_wallet as &Wallet),
                    to_wallet:   WalletDto::from(to_wallet as &Wallet),
                })
            }
            (Err(e), _) | (_, Err(e)) => {
                txn.fail(e.to_string()).ok();
                let _ = self.transaction_repo.save_in_tx(&txn, &mut db_tx).await;
                let _ = db_tx.rollback().await;
                Err(ApplicationError::Domain(e))
            }
        }
    }

    // ── Get Wallet ────────────────────────────────────────────────────────────

    pub async fn get_wallet(&self, id: WalletId) -> Result<WalletDto, ApplicationError> {
        let wallet = self
            .wallet_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| ApplicationError::Domain(DomainError::WalletNotFound(id.inner())))?;
        Ok(WalletDto::from(&wallet))
    }

    // ── Get Transaction ───────────────────────────────────────────────────────

    pub async fn get_transaction(
        &self,
        id: TransactionId,
    ) -> Result<TransactionDto, ApplicationError> {
        let txn = self
            .transaction_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| ApplicationError::Domain(DomainError::TransactionNotFound(id.inner())))?;
        Ok(TransactionDto::from(&txn))
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    async fn persist_wallet_in_tx(
        &self,
        wallet: &Wallet,
        db_tx:  &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ApplicationError> {
        sqlx::query!(
            r#"
            UPDATE wallets
               SET balance   = $1,
                   version   = $2,
                   updated_at = NOW()
             WHERE id      = $3
               AND version = $4
            "#,
            wallet.balance().amount(),
            wallet.version(),
            wallet.id().inner(),
            wallet.version() - 1, // Pre-bump version
        )
        .execute(&mut **db_tx)
        .await
        .map_err(|e| ApplicationError::infrastructure(format!("Wallet update: {e}")))?;

        Ok(())
    }

    /// Publish events without letting failures propagate to the caller.
    /// Events go to DLQ if the broker is temporarily unavailable.
    async fn publish_event_best_effort(&self, event: DomainEvent) {
        if let Err(e) = self.event_publisher.publish(event).await {
            warn!(error = %e, "Failed to publish domain event (best-effort)");
        }
    }
}