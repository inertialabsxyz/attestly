-- 0002_usage.sql — daily attestation usage counters per account.
--
-- Step 2 populates this table on every ingest; Step 4 reads it to emit
-- Stripe metered-billing records. Granularity is `account_id × UTC day`,
-- which is enough for the 1M-attestations/month Team-tier floor and the
-- per-1k surcharge above it.

CREATE TABLE IF NOT EXISTS usage (
    account_id          UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    day                 DATE        NOT NULL,
    attestation_count   BIGINT      NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, day)
);

CREATE INDEX IF NOT EXISTS idx_usage_day ON usage(day);
