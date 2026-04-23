// src/application/errors.rs
use thiserror::Error;

use crate::domain::errors::DomainError;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("Domain error: {0}")]
    Domain(#[from] DomainError),

    #[error("Infrastructure error: {0}")]
    Infrastructure(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Event publish error: {0}")]
    EventPublish(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Internal error: {0}")]
    Internal(String),
}

impl ApplicationError {
    pub fn infrastructure(msg: impl Into<String>) -> Self {
        Self::Infrastructure(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}