//! Storage abstraction for `awp-cloud`.
//!
//! Two implementations:
//!
//! - [`memory::MemDb`] — tests and the `cargo run` default. Pure Rust, no
//!   external dependencies.
//! - [`postgres::PgDb`] — production. Reuses `sqlx` against the schema in
//!   `services/awp-cloud/migrations/`.
//!
//! The trait is intentionally narrow — every method maps to a discrete
//! handler need. There is no `query_raw` escape hatch.

pub mod memory;
pub mod postgres;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::error::ApiError;

/// A row from `accounts`. Stripe fields (`stripe_customer_id`,
/// `stripe_subscription_id`) are populated by the Step 4 billing webhook;
/// `billed_through` is the last UTC day we've already posted metered usage
/// for and is the gate against double-billing on retry.
#[derive(Clone, Debug)]
pub struct Account {
    pub id: Uuid,
    pub email: String,
    pub plan: Plan,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
    pub retention_days: i32,
    pub billed_through: Option<chrono::NaiveDate>,
    pub created_at: DateTime<Utc>,
}

/// Account plan tier. Stored as a TEXT column with the slug `team` or
/// `enterprise`; OSS users have no `accounts` row at all and never reach
/// this enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    Team,
    Enterprise,
}

impl Plan {
    pub fn as_slug(&self) -> &'static str {
        match self {
            Plan::Team => "team",
            Plan::Enterprise => "enterprise",
        }
    }

    pub fn from_slug(s: &str) -> Result<Self, ApiError> {
        match s {
            "team" => Ok(Plan::Team),
            "enterprise" => Ok(Plan::Enterprise),
            other => Err(ApiError::InvalidRequest(format!("unknown plan `{other}`"))),
        }
    }

    /// Bundled monthly attestation floor before metered overage kicks in.
    pub fn included_attestations(&self) -> i64 {
        match self {
            Plan::Team => 1_000_000,
            // Enterprise is contractually unlimited — we still record usage,
            // but the per-1k overage line item never fires.
            Plan::Enterprise => i64::MAX,
        }
    }

    /// Default retention horizon, in days.
    pub fn retention_days(&self) -> i32 {
        match self {
            Plan::Team => 365,
            Plan::Enterprise => 2555,
        }
    }
}

/// A row from `api_keys`. The `hashed_key` is never returned to clients; we
/// only ever check it on the auth path. The unhashed key is shown exactly
/// once at creation time (via the dashboard or the `seed` binary).
#[derive(Clone, Debug)]
pub struct ApiKeyRecord {
    pub id: Uuid,
    pub account_id: Uuid,
    pub project_name: String,
    pub hashed_key: String,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Metadata index row for an attestation. The signed canonical bytes live in
/// blob storage; this row points to them.
#[derive(Clone, Debug, Serialize)]
pub struct AttestationIndex {
    pub id: Uuid,
    pub account_id: Uuid,
    pub agent_id: String,
    pub agent_pubkey_hex: String,
    pub customer_id: Option<String>,
    pub received_at: DateTime<Utc>,
    pub blob_sha256_hex: String,
}

/// Filters for `GET /v1/attestations`. `cursor` is opaque to handlers; the
/// store decides how to interpret it.
#[derive(Clone, Debug, Default)]
pub struct SearchFilters {
    pub agent_id: Option<String>,
    pub customer_id: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub limit: usize,
    pub cursor: Option<String>,
}

#[derive(Debug)]
pub struct SearchPage {
    pub rows: Vec<AttestationIndex>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ShareLink {
    pub token: String,
    pub account_id: Uuid,
    pub attestation_ids: Vec<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Result of inserting an attestation. `Inserted` is the 201 path; `Existed`
/// is the idempotent 200 path.
#[derive(Debug)]
pub enum IngestOutcome {
    Inserted(AttestationIndex),
    Existed(AttestationIndex),
}

/// One UTC-day bucket of usage for a single account. Returned by
/// [`Db::usage_for_period`] and used to drive both the dashboard's usage
/// chart and the metered-billing trigger.
#[derive(Clone, Debug, Serialize)]
pub struct DailyUsage {
    pub day: chrono::NaiveDate,
    pub count: i64,
}

#[async_trait]
pub trait Db: Send + Sync + 'static {
    async fn ping(&self) -> Result<(), ApiError>;

    async fn create_account(&self, email: &str, plan: Plan) -> Result<Account, ApiError>;

    /// Look up an account by its stored Stripe customer id — the webhook
    /// dispatch path uses this to map `customer.subscription.updated` events
    /// back to the local account row.
    async fn find_account_by_stripe_customer(
        &self,
        stripe_customer_id: &str,
    ) -> Result<Option<Account>, ApiError>;

    /// Idempotent: returns the existing account if one already exists for
    /// `email`, otherwise creates a new one. Used by `checkout.session.completed`
    /// so a re-delivered webhook doesn't double-issue accounts.
    async fn upsert_account_by_email(
        &self,
        email: &str,
        plan: Plan,
    ) -> Result<(Account, bool), ApiError>;

    async fn fetch_account(&self, id: Uuid) -> Result<Option<Account>, ApiError>;

    async fn set_account_stripe_ids(
        &self,
        account_id: Uuid,
        stripe_customer_id: &str,
        stripe_subscription_id: Option<&str>,
    ) -> Result<(), ApiError>;

    async fn set_account_plan(&self, account_id: Uuid, plan: Plan) -> Result<(), ApiError>;

    /// Atomically advance the watermark of the last UTC day we've already
    /// billed for. The metered-billing trigger uses this as the gate against
    /// double-charging on retry.
    async fn set_billed_through(
        &self,
        account_id: Uuid,
        through: chrono::NaiveDate,
    ) -> Result<(), ApiError>;

    /// Sum of `attestation_count` rows for `account_id` in the half-open
    /// range `[from, to)`. Drives the metered-billing surcharge.
    async fn account_usage_in_range(
        &self,
        account_id: Uuid,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Result<i64, ApiError>;

    /// Per-day usage points for the dashboard chart. Ordered ascending.
    async fn usage_for_period(
        &self,
        account_id: Uuid,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Result<Vec<DailyUsage>, ApiError>;

    async fn list_active_api_keys(&self, account_id: Uuid) -> Result<Vec<ApiKeyRecord>, ApiError>;

    /// Revoke an API key by id. Returns `true` if a row was updated, `false`
    /// if the key did not belong to `account_id` or was already revoked.
    async fn revoke_api_key(&self, account_id: Uuid, key_id: Uuid) -> Result<bool, ApiError>;

    /// Delete every attestation row for `account_id` whose `received_at`
    /// strictly precedes `older_than`. Returns the row count. Drives the
    /// retention sweeper. The blob store keeps its content-addressed bytes —
    /// other accounts may reference the same blob, and orphan-GC is a
    /// separate concern not in scope for Step 4.
    async fn delete_attestations_older_than(
        &self,
        account_id: Uuid,
        older_than: DateTime<Utc>,
    ) -> Result<u64, ApiError>;

    /// Update the bookkeeping column the sweeper writes after each pass.
    /// Not load-bearing — only used for debugging.
    async fn mark_swept(&self, account_id: Uuid, at: DateTime<Utc>) -> Result<(), ApiError>;

    /// Every account, for cron-style enumeration (retention sweeper, end-of-
    /// period billing). Returns the full row so callers don't need a second
    /// fetch.
    async fn list_accounts(&self) -> Result<Vec<Account>, ApiError>;

    async fn create_api_key(
        &self,
        account_id: Uuid,
        project_name: &str,
        hashed_key: &str,
    ) -> Result<ApiKeyRecord, ApiError>;

    /// Find an account by an API key's hashed value. Returns `None` if no
    /// matching key exists (or if the matching key has been revoked).
    async fn find_account_by_hashed_key(
        &self,
        hashed_key: &str,
    ) -> Result<Option<Account>, ApiError>;

    /// Look up *every* active hashed key — the Argon2 comparison is too
    /// expensive to do per-row in SQL, so the auth layer fetches the active
    /// hashes for an account candidate and verifies in Rust. For a small key
    /// table (per-account) this is fine; for hot paths we'd cache.
    async fn list_active_hashed_keys(&self) -> Result<Vec<(String, Uuid)>, ApiError>;

    async fn insert_attestation(&self, index: AttestationIndex) -> Result<IngestOutcome, ApiError>;

    async fn fetch_attestation(
        &self,
        account_id: Uuid,
        id: Uuid,
    ) -> Result<Option<AttestationIndex>, ApiError>;

    async fn search_attestations(
        &self,
        account_id: Uuid,
        filters: SearchFilters,
    ) -> Result<SearchPage, ApiError>;

    /// Stream all of an account's attestations, ordered by `received_at` asc,
    /// for `/v1/export`. We return them all at once here for backend
    /// simplicity; the HTTP layer streams them out line-by-line.
    async fn list_all_attestations(
        &self,
        account_id: Uuid,
    ) -> Result<Vec<AttestationIndex>, ApiError>;

    async fn create_share_link(&self, link: ShareLink) -> Result<(), ApiError>;

    async fn fetch_share_link(&self, token: &str) -> Result<Option<ShareLink>, ApiError>;

    async fn revoke_share_link(&self, account_id: Uuid, token: &str) -> Result<bool, ApiError>;

    /// Increment the daily usage counter for `account_id` on the UTC day
    /// containing `at`. Used as the source of truth for metered billing in
    /// Step 4. Scope here: *recording*. Step 4 adds the billing surface.
    async fn record_usage(&self, account_id: Uuid, at: DateTime<Utc>) -> Result<(), ApiError>;

    /// Sum of `attestation_count` rows in `usage` for `account_id`. Used by
    /// the tests and the dashboard; Stripe metered-billing reads this in
    /// Step 4.
    async fn account_usage_total(&self, account_id: Uuid) -> Result<i64, ApiError>;
}
