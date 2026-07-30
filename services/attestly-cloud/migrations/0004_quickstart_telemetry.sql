-- 0004_quickstart_telemetry.sql — self-serve signup + anonymous SDK telemetry.
--
-- Added in Step 5 (gtm phase 2 quickstart, docs, conversion telemetry).
--
-- Two unrelated surfaces share one migration because both ship together:
--
--   * `accounts.password_hash` — Argon2id hash of the user's password from
--     the `/v1/account/signup` flow. NULL for accounts created via Stripe
--     checkout (those have no password — they auth purely with their API
--     key, surfaced once in the welcome banner).
--
--   * `telemetry_events` — daily aggregate rows POSTed by the SDK. Holds
--     **no PII and no foreign key to `accounts`**: it is read-only telemetry
--     keyed on a self-generated install UUID, used by the GTM team to
--     identify conversion-ready OSS users (>10k attestations/day on
--     FileSink). The deliberate lack of a join to `accounts` is the privacy
--     guarantee — a telemetry row never resolves back to an identified user
--     unless that user separately signs up.

ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS password_hash TEXT NULL;

CREATE TABLE IF NOT EXISTS telemetry_events (
    install_id                 UUID         NOT NULL,
    day                        DATE         NOT NULL,
    sdk_version                TEXT         NOT NULL,
    sink_type                  TEXT         NOT NULL,
    python_version             TEXT         NOT NULL,
    os                         TEXT         NOT NULL,
    attestations_emitted_today BIGINT       NOT NULL DEFAULT 0,
    last_seen_at               TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (install_id, day)
);

CREATE INDEX IF NOT EXISTS idx_telemetry_day ON telemetry_events(day);
CREATE INDEX IF NOT EXISTS idx_telemetry_volume
    ON telemetry_events(day, attestations_emitted_today DESC)
    WHERE sink_type = 'FileSink';
