// src/domain/value_objects.rs
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

use crate::domain::errors::DomainError;

// ─── Money ────────────────────────────────────────────────────────────────────

/// Immutable value object representing a precise monetary amount.
/// Uses `rust_decimal` to avoid all floating-point arithmetic errors.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Money {
    amount:   Decimal,
    currency: Currency,
}

impl Money {
    /// Create a new Money value, enforcing non-negativity.
    pub fn new(amount: Decimal, currency: Currency) -> Result<Self, DomainError> {
        if amount < Decimal::ZERO {
            return Err(DomainError::InvalidAmount(format!(
                "Amount cannot be negative: {}",
                amount
            )));
        }
        Ok(Self { amount, currency })
    }

    /// Parse from a string representation (e.g. "100.50").
    pub fn from_str_amount(s: &str, currency: Currency) -> Result<Self, DomainError> {
        let amount = Decimal::from_str(s)
            .map_err(|_| DomainError::InvalidAmount(format!("Cannot parse '{}' as decimal", s)))?;
        Self::new(amount, currency)
    }

    pub fn zero(currency: Currency) -> Self {
        Self { amount: Decimal::ZERO, currency }
    }

    pub fn amount(&self)   -> Decimal  { self.amount }
    pub fn currency(&self) -> &Currency { &self.currency }

    pub fn is_positive(&self) -> bool { self.amount > Decimal::ZERO }
    pub fn is_zero(&self)     -> bool { self.amount == Decimal::ZERO }

    /// Add two Money values — requires same currency.
    pub fn checked_add(&self, other: &Money) -> Result<Money, DomainError> {
        self.assert_same_currency(other)?;
        Ok(Self {
            amount:   self.amount + other.amount,
            currency: self.currency.clone(),
        })
    }

    /// Subtract — returns error if result would be negative (insufficient funds).
    pub fn checked_sub(&self, other: &Money) -> Result<Money, DomainError> {
        self.assert_same_currency(other)?;
        if self.amount < other.amount {
            return Err(DomainError::InsufficientFunds {
                wallet_id: Uuid::nil(), // Filled in by the caller
                available: self.amount.to_string(),
                requested: other.amount.to_string(),
            });
        }
        Ok(Self {
            amount:   self.amount - other.amount,
            currency: self.currency.clone(),
        })
    }

    fn assert_same_currency(&self, other: &Money) -> Result<(), DomainError> {
        if self.currency != other.currency {
            return Err(DomainError::CurrencyMismatch {
                expected: self.currency.code().to_string(),
                got:      other.currency.code().to_string(),
            });
        }
        Ok(())
    }

    pub fn to_string_amount(&self) -> String {
        self.amount.to_string()
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.currency.code())
    }
}

// ─── Currency ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Currency(String);

impl Currency {
    pub fn new(code: &str) -> Result<Self, DomainError> {
        let code = code.trim().to_uppercase();
        if code.len() != 3 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(DomainError::InvalidAmount(format!(
                "Invalid currency code: '{}'",
                code
            )));
        }
        Ok(Self(code))
    }

    pub fn code(&self) -> &str { &self.0 }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── TransactionId ────────────────────────────────────────────────────────────

/// Newtype wrapper ensuring type-safe transaction identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(Uuid);

impl TransactionId {
    pub fn new()        -> Self { Self(Uuid::new_v4()) }
    pub fn from(id: Uuid) -> Self { Self(id) }
    pub fn inner(&self) -> Uuid { self.0 }
}

impl fmt::Display for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── WalletId ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WalletId(Uuid);

impl WalletId {
    pub fn new()          -> Self { Self(Uuid::new_v4()) }
    pub fn from(id: Uuid) -> Self { Self(id) }
    pub fn inner(&self)   -> Uuid { self.0 }
}

impl fmt::Display for WalletId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── IdempotencyKey ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(key: impl Into<String>) -> Result<Self, DomainError> {
        let key = key.into();
        if key.is_empty() || key.len() > 128 {
            return Err(DomainError::InvalidAmount(
                "Idempotency key must be 1-128 characters".into(),
            ));
        }
        Ok(Self(key))
    }

    pub fn value(&self) -> &str { &self.0 }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_money_add_same_currency() {
        let usd = Currency::new("USD").unwrap();
        let a = Money::new(dec!(10.50), usd.clone()).unwrap();
        let b = Money::new(dec!(5.25),  usd.clone()).unwrap();
        let sum = a.checked_add(&b).unwrap();
        assert_eq!(sum.amount(), dec!(15.75));
    }

    #[test]
    fn test_money_sub_insufficient() {
        let usd = Currency::new("USD").unwrap();
        let a = Money::new(dec!(5.00), usd.clone()).unwrap();
        let b = Money::new(dec!(10.00), usd.clone()).unwrap();
        assert!(matches!(a.checked_sub(&b), Err(DomainError::InsufficientFunds { .. })));
    }

    #[test]
    fn test_currency_mismatch() {
        let usd = Money::new(dec!(10.0), Currency::new("USD").unwrap()).unwrap();
        let eur = Money::new(dec!(10.0), Currency::new("EUR").unwrap()).unwrap();
        assert!(matches!(usd.checked_add(&eur), Err(DomainError::CurrencyMismatch { .. })));
    }

    #[test]
    fn test_invalid_currency_code() {
        assert!(Currency::new("US").is_err());
        assert!(Currency::new("USDD").is_err());
        assert!(Currency::new("123").is_err());
    }
}