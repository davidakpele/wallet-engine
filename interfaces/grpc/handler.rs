// src/interfaces/grpc/handler.rs
//
// Thin adapter: converts Protobuf messages → application DTOs → Protobuf responses.
// Contains zero business logic.

use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::instrument;
use uuid::Uuid;

use crate::application::{
    dto::{
        CreateWalletCommand, DepositCommand, TransferCommand, WithdrawalCommand,
    },
    errors::ApplicationError,
    service::TransactionService,
};
use crate::domain::errors::DomainError;
use crate::domain::value_objects::TransactionId;

// Import generated protobuf types
// (Generated at build time into $OUT_DIR/wallet.v1.rs)
pub mod proto {
    tonic::include_proto!("wallet.v1");
}

use proto::{
    wallet_service_server::WalletService,
    CreateWalletRequest, CreateWalletResponse,
    DepositRequest, DepositResponse,
    GetTransactionRequest, GetTransactionResponse,
    GetWalletRequest, GetWalletResponse,
    TransferRequest, TransferResponse,
    WithdrawalRequest, WithdrawalResponse,
};

pub struct WalletGrpcHandler {
    service: Arc<TransactionService>,
}

impl WalletGrpcHandler {
    pub fn new(service: Arc<TransactionService>) -> Self {
        Self { service }
    }
}

/// Map application errors → gRPC Status codes
fn to_status(err: ApplicationError) -> Status {
    match err {
        ApplicationError::Domain(DomainError::WalletNotFound(_))
        | ApplicationError::Domain(DomainError::TransactionNotFound(_)) => {
            Status::not_found(err.to_string())
        }
        ApplicationError::Domain(DomainError::InsufficientFunds { .. }) => {
            Status::failed_precondition(err.to_string())
        }
        ApplicationError::Domain(DomainError::DuplicateTransaction(_)) => {
            Status::already_exists(err.to_string())
        }
        ApplicationError::Domain(DomainError::RateLimitExceeded(_))
        | ApplicationError::RateLimitExceeded => {
            Status::resource_exhausted(err.to_string())
        }
        ApplicationError::Validation(msg) => Status::invalid_argument(msg),
        _ => Status::internal(err.to_string()),
    }
}

#[tonic::async_trait]
impl WalletService for WalletGrpcHandler {
    #[instrument(skip(self))]
    async fn create_wallet(
        &self,
        request: Request<CreateWalletRequest>,
    ) -> Result<Response<CreateWalletResponse>, Status> {
        let req = request.into_inner();
        let user_id = Uuid::parse_str(&req.user_id)
            .map_err(|_| Status::invalid_argument("Invalid user_id UUID"))?;

        let result = self
            .service
            .create_wallet(CreateWalletCommand { user_id, currency: req.currency })
            .await
            .map_err(to_status)?;

        let wallet = proto::Wallet {
            id:         result.id.to_string(),
            user_id:    result.user_id.to_string(),
            balance:    Some(proto::Money {
                amount:   result.balance,
                currency: result.currency,
            }),
            version:    result.version,
            created_at: None, // Omitted for brevity; add prost_types::Timestamp as needed
            updated_at: None,
        };

        Ok(Response::new(CreateWalletResponse { wallet: Some(wallet) }))
    }

    #[instrument(skip(self))]
    async fn get_wallet(
        &self,
        request: Request<GetWalletRequest>,
    ) -> Result<Response<GetWalletResponse>, Status> {
        let wallet_id = Uuid::parse_str(&request.into_inner().wallet_id)
            .map_err(|_| Status::invalid_argument("Invalid wallet_id UUID"))?;

        let wallet = self
            .service
            .get_wallet(crate::domain::value_objects::WalletId::from(wallet_id))
            .await
            .map_err(to_status)?;

        Ok(Response::new(GetWalletResponse {
            wallet: Some(proto::Wallet {
                id:         wallet.id.to_string(),
                user_id:    wallet.user_id.to_string(),
                balance:    Some(proto::Money {
                    amount:   wallet.balance,
                    currency: wallet.currency,
                }),
                version:    wallet.version,
                created_at: None,
                updated_at: None,
            }),
        }))
    }

    #[instrument(skip(self), fields(idempotency_key = %request.get_ref().idempotency_key))]
    async fn deposit(
        &self,
        request: Request<DepositRequest>,
    ) -> Result<Response<DepositResponse>, Status> {
        let req = request.into_inner();
        let wallet_id = Uuid::parse_str(&req.wallet_id)
            .map_err(|_| Status::invalid_argument("Invalid wallet_id UUID"))?;
        let money = req.amount.ok_or_else(|| Status::invalid_argument("Missing amount"))?;

        let result = self
            .service
            .deposit(DepositCommand {
                idempotency_key: req.idempotency_key,
                wallet_id,
                amount:   money.amount,
                currency: money.currency,
            })
            .await
            .map_err(to_status)?;

        Ok(Response::new(DepositResponse {
            transaction: Some(txn_dto_to_proto(result.transaction)),
            wallet:      Some(wallet_dto_to_proto(result.wallet)),
        }))
    }

    #[instrument(skip(self), fields(idempotency_key = %request.get_ref().idempotency_key))]
    async fn withdraw(
        &self,
        request: Request<WithdrawalRequest>,
    ) -> Result<Response<WithdrawalResponse>, Status> {
        let req = request.into_inner();
        let wallet_id = Uuid::parse_str(&req.wallet_id)
            .map_err(|_| Status::invalid_argument("Invalid wallet_id"))?;
        let money = req.amount.ok_or_else(|| Status::invalid_argument("Missing amount"))?;

        let result = self
            .service
            .withdraw(WithdrawalCommand {
                idempotency_key: req.idempotency_key,
                wallet_id,
                amount:   money.amount,
                currency: money.currency,
            })
            .await
            .map_err(to_status)?;

        Ok(Response::new(WithdrawalResponse {
            transaction: Some(txn_dto_to_proto(result.transaction)),
            wallet:      Some(wallet_dto_to_proto(result.wallet)),
        }))
    }

    #[instrument(skip(self), fields(idempotency_key = %request.get_ref().idempotency_key))]
    async fn transfer(
        &self,
        request: Request<TransferRequest>,
    ) -> Result<Response<TransferResponse>, Status> {
        let req = request.into_inner();
        let from_wallet_id = Uuid::parse_str(&req.from_wallet_id)
            .map_err(|_| Status::invalid_argument("Invalid from_wallet_id"))?;
        let to_wallet_id = Uuid::parse_str(&req.to_wallet_id)
            .map_err(|_| Status::invalid_argument("Invalid to_wallet_id"))?;
        let money = req.amount.ok_or_else(|| Status::invalid_argument("Missing amount"))?;

        let result = self
            .service
            .transfer(TransferCommand {
                idempotency_key: req.idempotency_key,
                from_wallet_id,
                to_wallet_id,
                amount:   money.amount,
                currency: money.currency,
            })
            .await
            .map_err(to_status)?;

        Ok(Response::new(TransferResponse {
            transaction:  Some(txn_dto_to_proto(result.transaction)),
            from_wallet:  Some(wallet_dto_to_proto(result.from_wallet)),
            to_wallet:    Some(wallet_dto_to_proto(result.to_wallet)),
        }))
    }

    #[instrument(skip(self))]
    async fn get_transaction(
        &self,
        request: Request<GetTransactionRequest>,
    ) -> Result<Response<GetTransactionResponse>, Status> {
        let txn_id = Uuid::parse_str(&request.into_inner().transaction_id)
            .map_err(|_| Status::invalid_argument("Invalid transaction_id UUID"))?;

        let txn = self
            .service
            .get_transaction(TransactionId::from(txn_id))
            .await
            .map_err(to_status)?;

        Ok(Response::new(GetTransactionResponse {
            transaction: Some(txn_dto_to_proto(txn)),
        }))
    }
}

// ─── DTO → Proto Converters ───────────────────────────────────────────────────

fn wallet_dto_to_proto(w: crate::application::dto::WalletDto) -> proto::Wallet {
    proto::Wallet {
        id:         w.id.to_string(),
        user_id:    w.user_id.to_string(),
        balance:    Some(proto::Money { amount: w.balance, currency: w.currency }),
        version:    w.version,
        created_at: None,
        updated_at: None,
    }
}

fn txn_dto_to_proto(t: crate::application::dto::TransactionDto) -> proto::Transaction {
    use proto::{TransactionStatus, TransactionType};
    use crate::domain::entities::{
        TransactionType as DomainType, TransactionStatus as DomainStatus
    };

    let txn_type = match t.transaction_type {
        DomainType::Deposit    => TransactionType::Deposit as i32,
        DomainType::Withdrawal => TransactionType::Withdrawal as i32,
        DomainType::Transfer   => TransactionType::Transfer as i32,
    };

    let status = match t.status {
        DomainStatus::Pending    => TransactionStatus::Pending as i32,
        DomainStatus::Completed  => TransactionStatus::Completed as i32,
        DomainStatus::Failed     => TransactionStatus::Failed as i32,
        DomainStatus::RolledBack => TransactionStatus::RolledBack as i32,
    };

    proto::Transaction {
        id:              t.id.to_string(),
        wallet_id:       t.wallet_id.to_string(),
        to_wallet_id:    t.to_wallet_id.map(|id| id.to_string()),
        amount:          Some(proto::Money { amount: t.amount, currency: t.currency }),
        r#type:          txn_type,
        status,
        idempotency_key: t.idempotency_key,
        failure_reason:  t.failure_reason,
        created_at:      None,
        updated_at:      None,
    }
}