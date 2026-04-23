// tests/integration_test.rs
//
// Integration tests using testcontainers to spin up real Postgres + RabbitMQ.
// Run with: cargo test --test integration_test

use std::sync::Arc;
use rust_decimal_macros::dec;
use sqlx::postgres::PgPoolOptions;
use testcontainers::{runners::AsyncRunner, ImageExt};
use testcontainers_modules::{postgres::Postgres, rabbitmq::RabbitMq};
use uuid::Uuid;

use wallet_engine::{
    application::{
        dto::{CreateWalletCommand, DepositCommand, TransferCommand, WithdrawalCommand},
        service::TransactionService,
    },
    infrastructure::{
        postgres::{PgTransactionRepository, PgWalletRepository},
        rabbitmq::RabbitMQPublisher,
    },
};

// ─── Test Helpers ─────────────────────────────────────────────────────────────

async fn setup() -> (Arc<TransactionService>, Arc<sqlx::PgPool>) {
    // Spin up ephemeral Postgres
    let pg = Postgres::default()
        .with_env_var("POSTGRES_DB", "wallet_test")
        .with_env_var("POSTGRES_USER", "test")
        .with_env_var("POSTGRES_PASSWORD", "test")
        .start()
        .await
        .expect("Failed to start Postgres container");

    let pg_port = pg.get_host_port_ipv4(5432).await.unwrap();
    let db_url = format!("postgres://test:test@127.0.0.1:{}/wallet_test", pg_port);

    let pool = Arc::new(
        PgPoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .expect("Failed to connect to Postgres"),
    );

    // Apply migrations
    sqlx::migrate!("./migrations")
        .run(pool.as_ref())
        .await
        .expect("Migration failed");

    // Spin up RabbitMQ
    let rmq = RabbitMq::default()
        .start()
        .await
        .expect("Failed to start RabbitMQ container");
    let rmq_port = rmq.get_host_port_ipv4(5672).await.unwrap();
    let amqp_url = format!("amqp://guest:guest@127.0.0.1:{}", rmq_port);

    let wallet_repo      = Arc::new(PgWalletRepository::new(Arc::clone(&pool)));
    let transaction_repo = Arc::new(PgTransactionRepository::new(Arc::clone(&pool)));
    let publisher        = Arc::new(
        RabbitMQPublisher::new(&amqp_url)
            .await
            .expect("Failed to connect to RabbitMQ"),
    );

    let service = Arc::new(TransactionService::new(
        wallet_repo,
        transaction_repo,
        publisher,
        Arc::clone(&pool),
    ));

    (service, pool)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Full happy-path flow: create wallet → deposit → verify balance.
#[tokio::test]
async fn test_deposit_increases_balance() {
    let (service, _pool) = setup().await;

    let user_id = Uuid::new_v4();

    // Create wallet
    let wallet = service
        .create_wallet(CreateWalletCommand {
            user_id,
            currency: "USD".into(),
        })
        .await
        .expect("create_wallet failed");

    assert_eq!(wallet.balance, "0");

    // Deposit
    let result = service
        .deposit(DepositCommand {
            idempotency_key: Uuid::new_v4().to_string(),
            wallet_id:       wallet.id,
            amount:          "250.00".into(),
            currency:        "USD".into(),
        })
        .await
        .expect("deposit failed");

    assert_eq!(result.wallet.balance, "250.00");
    assert_eq!(result.transaction.status, wallet_engine::domain::entities::TransactionStatus::Completed);
}

/// Deposit is idempotent: second call with same key returns cached result.
#[tokio::test]
async fn test_deposit_idempotency() {
    let (service, _pool) = setup().await;
    let user_id = Uuid::new_v4();

    let wallet = service
        .create_wallet(CreateWalletCommand { user_id, currency: "USD".into() })
        .await
        .unwrap();

    let key = Uuid::new_v4().to_string();

    let r1 = service
        .deposit(DepositCommand {
            idempotency_key: key.clone(),
            wallet_id:       wallet.id,
            amount:          "100.00".into(),
            currency:        "USD".into(),
        })
        .await
        .unwrap();

    // Second call — same key
    let r2 = service
        .deposit(DepositCommand {
            idempotency_key: key.clone(),
            wallet_id:       wallet.id,
            amount:          "100.00".into(),
            currency:        "USD".into(),
        })
        .await
        .unwrap();

    // Same transaction ID, balance unchanged (credited once only)
    assert_eq!(r1.transaction.id, r2.transaction.id);
    assert_eq!(r2.wallet.balance, "100.00");
}

/// Withdrawal fails when balance is insufficient.
#[tokio::test]
async fn test_withdrawal_insufficient_funds() {
    let (service, _pool) = setup().await;
    let user_id = Uuid::new_v4();

    let wallet = service
        .create_wallet(CreateWalletCommand { user_id, currency: "USD".into() })
        .await
        .unwrap();

    // Deposit 50
    service
        .deposit(DepositCommand {
            idempotency_key: Uuid::new_v4().to_string(),
            wallet_id:       wallet.id,
            amount:          "50.00".into(),
            currency:        "USD".into(),
        })
        .await
        .unwrap();

    // Try to withdraw 100
    let err = service
        .withdraw(WithdrawalCommand {
            idempotency_key: Uuid::new_v4().to_string(),
            wallet_id:       wallet.id,
            amount:          "100.00".into(),
            currency:        "USD".into(),
        })
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            wallet_engine::application::errors::ApplicationError::Domain(
                wallet_engine::domain::errors::DomainError::InsufficientFunds { .. }
            )
        ),
        "Expected InsufficientFunds, got: {:?}",
        err
    );
}

/// Transfer moves funds atomically between two wallets.
#[tokio::test]
async fn test_transfer_atomic() {
    let (service, _pool) = setup().await;

    // Alice
    let alice_wallet = service
        .create_wallet(CreateWalletCommand { user_id: Uuid::new_v4(), currency: "USD".into() })
        .await
        .unwrap();

    // Bob
    let bob_wallet = service
        .create_wallet(CreateWalletCommand { user_id: Uuid::new_v4(), currency: "USD".into() })
        .await
        .unwrap();

    // Fund Alice with $500
    service
        .deposit(DepositCommand {
            idempotency_key: Uuid::new_v4().to_string(),
            wallet_id:       alice_wallet.id,
            amount:          "500.00".into(),
            currency:        "USD".into(),
        })
        .await
        .unwrap();

    // Transfer $200 Alice → Bob
    let result = service
        .transfer(TransferCommand {
            idempotency_key: Uuid::new_v4().to_string(),
            from_wallet_id:  alice_wallet.id,
            to_wallet_id:    bob_wallet.id,
            amount:          "200.00".into(),
            currency:        "USD".into(),
        })
        .await
        .expect("Transfer failed");

    assert_eq!(result.from_wallet.balance, "300.00", "Alice should have $300");
    assert_eq!(result.to_wallet.balance,   "200.00", "Bob should have $200");
}

/// Transfer to the same wallet is rejected.
#[tokio::test]
async fn test_self_transfer_rejected() {
    let (service, _pool) = setup().await;

    let wallet = service
        .create_wallet(CreateWalletCommand { user_id: Uuid::new_v4(), currency: "USD".into() })
        .await
        .unwrap();

    let err = service
        .transfer(TransferCommand {
            idempotency_key: Uuid::new_v4().to_string(),
            from_wallet_id:  wallet.id,
            to_wallet_id:    wallet.id, // Same!
            amount:          "10.00".into(),
            currency:        "USD".into(),
        })
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            wallet_engine::application::errors::ApplicationError::Domain(
                wallet_engine::domain::errors::DomainError::SelfTransfer
            )
        ),
        "Expected SelfTransfer error"
    );
}

/// Concurrent deposits must not produce duplicate balance credits.
/// Sends 10 concurrent deposits with unique idempotency keys.
#[tokio::test]
async fn test_concurrent_deposits_no_race() {
    let (service, _pool) = setup().await;

    let wallet = service
        .create_wallet(CreateWalletCommand { user_id: Uuid::new_v4(), currency: "USD".into() })
        .await
        .unwrap();

    let service = Arc::clone(&service);
    let wallet_id = wallet.id;

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let svc = Arc::clone(&service);
            tokio::spawn(async move {
                svc.deposit(DepositCommand {
                    idempotency_key: Uuid::new_v4().to_string(),
                    wallet_id,
                    amount:   "10.00".into(),
                    currency: "USD".into(),
                })
                .await
            })
        })
        .collect();

    for h in handles {
        h.await.expect("Task panicked").expect("Deposit failed");
    }

    // Final balance should be exactly $100 (10 × $10)
    let final_wallet = service
        .get_wallet(wallet_engine::domain::value_objects::WalletId::from(wallet_id))
        .await
        .unwrap();

    assert_eq!(final_wallet.balance, "100.00",
        "Expected $100 after 10 concurrent $10 deposits, got: {}",
        final_wallet.balance
    );
}