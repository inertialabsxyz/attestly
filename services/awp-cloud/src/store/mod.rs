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

/// A row from `accounts`. Stripe linkage is `None` until Step 4 attaches
/// billing.
#[derive(Clone, Debug)]
pub struct Account {
    pub id: Uuid,
    pub email: String,
    pub stripe_customer_id: Option<String>,
    pub retention_days: i32,
    pub created_at: DateTime<Utc>,
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

#[async_trait]
pub trait Db: Send + Sync + 'static {
    async fn ping(&self) -> Result<(), ApiError>;

    async fn create_account(&self, email: &str) -> Result<Account, ApiError>;

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
