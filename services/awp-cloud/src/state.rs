//! `AppState` — the dependency-injection seam between the HTTP layer and the
//! storage backends.
//!
//! Tests construct an in-memory `AppState` via [`AppState::in_memory`]; the
//! production binary builds a Postgres + filesystem (or S3) state in
//! `main.rs`. Handlers depend on `AppState` rather than concrete backends so
//! the test suite has zero external dependencies.

use std::sync::Arc;

use crate::blob::{memory::MemBlobStore, BlobStore};
use crate::store::{memory::MemDb, Db};
use crate::stripe::{MockStripeClient, StripeClient};

/// Billing configuration. Populated from environment in `main.rs`; tests
/// inject a deterministic config via [`BillingConfig::for_tests`].
#[derive(Clone, Debug)]
pub struct BillingConfig {
    pub team_price_id: String,
    /// Stripe subscription-item id the metered overage line is posted
    /// against. In production this is one of the items on the customer's
    /// subscription — Stripe distinguishes the flat-fee base price from the
    /// metered overage price by item.
    pub overage_subscription_item_id: String,
    pub webhook_secret: String,
    pub base_url: String,
    /// Shared secret protecting the `/v1/admin/bill-period` trigger.
    pub admin_key: String,
}

impl BillingConfig {
    pub fn from_env() -> Self {
        Self {
            team_price_id: std::env::var("STRIPE_TEAM_PRICE_ID")
                .unwrap_or_else(|_| "price_team_placeholder".into()),
            overage_subscription_item_id: std::env::var("STRIPE_OVERAGE_ITEM_ID")
                .unwrap_or_else(|_| "si_overage_placeholder".into()),
            webhook_secret: std::env::var("STRIPE_WEBHOOK_SECRET")
                .unwrap_or_else(|_| "whsec_placeholder".into()),
            base_url: std::env::var("AWP_CLOUD_BASE_URL")
                .unwrap_or_else(|_| "https://app.awp-cloud.xyz".into()),
            admin_key: std::env::var("AWP_ADMIN_KEY")
                .unwrap_or_else(|_| "admin-key-placeholder".into()),
        }
    }

    pub fn for_tests() -> Self {
        Self {
            team_price_id: "price_test".into(),
            overage_subscription_item_id: "si_test".into(),
            webhook_secret: "whsec_test".into(),
            base_url: "https://test.local".into(),
            admin_key: "admin-test".into(),
        }
    }
}

/// The fully-wired application state. Cheap to clone (everything inside is
/// behind an `Arc`).
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<dyn Db>,
    pub blob: Arc<dyn BlobStore>,
    pub stripe: Arc<dyn StripeClient>,
    pub billing: BillingConfig,
}

impl AppState {
    /// Build an `AppState` with the in-memory `MemDb`, `MemBlobStore`, and
    /// `MockStripeClient` backends. Used by the test suite and by
    /// `cargo run` in CI smoke mode.
    pub fn in_memory() -> Self {
        Self {
            db: Arc::new(MemDb::new()),
            blob: Arc::new(MemBlobStore::new()),
            stripe: Arc::new(MockStripeClient::new()),
            billing: BillingConfig::for_tests(),
        }
    }

    /// Build an `AppState` from arbitrary backends — the wiring used by
    /// `main.rs` to plug in Postgres + filesystem + the live Stripe client
    /// at runtime.
    pub fn new(
        db: Arc<dyn Db>,
        blob: Arc<dyn BlobStore>,
        stripe: Arc<dyn StripeClient>,
        billing: BillingConfig,
    ) -> Self {
        Self {
            db,
            blob,
            stripe,
            billing,
        }
    }
}
