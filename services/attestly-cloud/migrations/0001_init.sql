-- 0001_init.sql — accounts, api_keys, attestations, share_links.
-- usage table is created in 0002_usage.sql so the migration ordering is
-- explicit. Idempotent: all DDL is `IF NOT EXISTS`.

CREATE TABLE IF NOT EXISTS accounts (
    id                  UUID        PRIMARY KEY,
    email               TEXT        NOT NULL UNIQUE,
    stripe_customer_id  TEXT        NULL,
    retention_days      INTEGER     NOT NULL DEFAULT 365,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS api_keys (
    id            UUID        PRIMARY KEY,
    account_id    UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    project_name  TEXT        NOT NULL,
    -- Argon2id PHC string. Compared in Rust, not in SQL.
    hashed_key    TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at    TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS idx_api_keys_account ON api_keys(account_id) WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS attestations (
    id                UUID        PRIMARY KEY,
    account_id        UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    agent_id          TEXT        NOT NULL,
    agent_pubkey_hex  TEXT        NOT NULL,
    customer_id       TEXT        NULL,
    received_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    blob_sha256_hex   TEXT        NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_attestations_account_received_at
    ON attestations (account_id, received_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_attestations_account_agent
    ON attestations (account_id, agent_id);
CREATE INDEX IF NOT EXISTS idx_attestations_account_customer
    ON attestations (account_id, customer_id) WHERE customer_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS share_links (
    token            TEXT        PRIMARY KEY,
    account_id       UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    attestation_ids  UUID[]      NOT NULL,
    expires_at       TIMESTAMPTZ NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at       TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS idx_share_links_account ON share_links(account_id);
