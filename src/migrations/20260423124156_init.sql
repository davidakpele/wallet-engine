-- Add migration script here
-- migrations/20240101000001_initial_schema.sql

-- ─── Enums ────────────────────────────────────────────────────────────────────

CREATE TYPE transaction_type AS ENUM (
    'DEPOSIT',
    'WITHDRAWAL',
    'TRANSFER'
);

CREATE TYPE transaction_status AS ENUM (
    'PENDING',
    'COMPLETED',
    'FAILED',
    'ROLLED_BACK'
);

-- ─── Wallets ──────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wallets (
    id          UUID          PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID          NOT NULL,
    balance     NUMERIC(20,8) NOT NULL DEFAULT 0 CHECK (balance >= 0),
    currency    CHAR(3)       NOT NULL,
    version     BIGINT        NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ   NOT NULL DEFAULT NOW(),

    CONSTRAINT wallets_user_currency_unique UNIQUE (user_id, currency)
);

-- Row-level locking index
CREATE INDEX idx_wallets_user_id  ON wallets (user_id);
CREATE INDEX idx_wallets_currency ON wallets (currency);

-- ─── Transactions ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS transactions (
    id               UUID               PRIMARY KEY DEFAULT gen_random_uuid(),
    wallet_id        UUID               NOT NULL REFERENCES wallets(id),
    to_wallet_id     UUID               REFERENCES wallets(id),
    amount           NUMERIC(20,8)      NOT NULL CHECK (amount > 0),
    currency         CHAR(3)            NOT NULL,
    transaction_type transaction_type   NOT NULL,
    status           transaction_status NOT NULL DEFAULT 'PENDING',
    idempotency_key  VARCHAR(128)       NOT NULL,
    failure_reason   TEXT,
    created_at       TIMESTAMPTZ        NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ        NOT NULL DEFAULT NOW(),

    -- Idempotency: one transaction per key, ever
    CONSTRAINT transactions_idempotency_key_unique UNIQUE (idempotency_key)
);

CREATE INDEX idx_transactions_wallet_id       ON transactions (wallet_id);
CREATE INDEX idx_transactions_to_wallet_id    ON transactions (to_wallet_id);
CREATE INDEX idx_transactions_status          ON transactions (status);
CREATE INDEX idx_transactions_created_at      ON transactions (created_at DESC);
CREATE INDEX idx_transactions_idempotency_key ON transactions (idempotency_key);

-- ─── Ledger (double-entry) ────────────────────────────────────────────────────
-- Each transaction produces two ledger entries (debit + credit).

CREATE TABLE IF NOT EXISTS ledger_entries (
    id             UUID          PRIMARY KEY DEFAULT gen_random_uuid(),
    transaction_id UUID          NOT NULL REFERENCES transactions(id),
    wallet_id      UUID          NOT NULL REFERENCES wallets(id),
    amount         NUMERIC(20,8) NOT NULL,  -- Positive = credit, Negative = debit
    balance_after  NUMERIC(20,8) NOT NULL,
    created_at     TIMESTAMPTZ   NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_ledger_transaction_id ON ledger_entries (transaction_id);
CREATE INDEX idx_ledger_wallet_id      ON ledger_entries (wallet_id);