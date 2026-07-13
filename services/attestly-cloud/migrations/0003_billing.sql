-- 0003_billing.sql — plan tier, Stripe subscription linkage, and per-period
-- billing watermarks.
--
-- Added in Step 4 (gtm phase 2 pricing & billing). The `accounts` table
-- already has `stripe_customer_id`; this migration:
--
--   * captures the active subscription id and a plan slug so the dashboard
--     can render "Team" vs "Enterprise" without round-tripping Stripe;
--   * adds a watermark of the last billing period we already posted usage
--     for, so the end-of-period billing trigger is safe to re-run without
--     double-charging customers;
--   * adds a deletion-cursor column the retention sweeper updates on each
--     pass — purely a debugging aid, not a correctness gate (the sweep
--     itself is idempotent).

ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS plan                    TEXT        NOT NULL DEFAULT 'team',
    ADD COLUMN IF NOT EXISTS stripe_subscription_id  TEXT        NULL,
    ADD COLUMN IF NOT EXISTS billed_through          DATE        NULL,
    ADD COLUMN IF NOT EXISTS last_swept_at           TIMESTAMPTZ NULL;

CREATE INDEX IF NOT EXISTS idx_accounts_stripe_customer
    ON accounts(stripe_customer_id) WHERE stripe_customer_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_accounts_stripe_subscription
    ON accounts(stripe_subscription_id) WHERE stripe_subscription_id IS NOT NULL;
