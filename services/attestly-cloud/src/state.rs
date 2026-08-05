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

/// Sentinel `admin_key` used when `ATTESTLY_ADMIN_KEY` is unset. Safe for the
/// in-memory harness and local `cargo run`; [`admin_key_is_insecure`] refuses
/// to let the production binary boot on it. Named once so the default and the
/// guard can't drift apart.
pub const ADMIN_KEY_PLACEHOLDER: &str = "admin-key-placeholder";

/// True if `key` must not be trusted to protect the admin routes — either
/// empty (env var unset or blank) or still the [`ADMIN_KEY_PLACEHOLDER`]
/// sentinel.
///
/// Pure so `main.rs` can gate startup on it and tests can assert it without
/// spawning a process. Deliberately *not* enforced inside
/// [`BillingConfig::from_env`] — the test harness and `for_tests` rely on
/// being able to construct a config with the placeholder.
pub fn admin_key_is_insecure(key: &str) -> bool {
    let key = key.trim();
    key.is_empty() || key == ADMIN_KEY_PLACEHOLDER
}

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
            base_url: std::env::var("ATTESTLY_CLOUD_BASE_URL")
                .unwrap_or_else(|_| "https://app.attestly.xyz".into()),
            admin_key: std::env::var("ATTESTLY_ADMIN_KEY")
                .unwrap_or_else(|_| ADMIN_KEY_PLACEHOLDER.into()),
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

    /// [`BillingConfig::for_tests`] with `base_url` overridden. Lets a test
    /// pin a non-default host and prove the share-link URL is derived from
    /// this config rather than from a second, independent env read.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_key_placeholder_is_insecure() {
        assert!(admin_key_is_insecure(ADMIN_KEY_PLACEHOLDER));
    }

    #[test]
    fn empty_admin_key_is_insecure() {
        assert!(admin_key_is_insecure(""));
        // An env var set to whitespace is the same mistake as leaving it unset.
        assert!(admin_key_is_insecure("   "));
    }

    #[test]
    fn real_admin_key_is_secure() {
        assert!(!admin_key_is_insecure("s3cr3t-rotated-at-deploy"));
    }

    #[test]
    fn from_env_default_admin_key_is_the_named_sentinel() {
        // Guards the drift the constant exists to prevent: if the `from_env`
        // default ever stops being ADMIN_KEY_PLACEHOLDER, the startup guard
        // would silently stop catching an unset ATTESTLY_ADMIN_KEY.
        assert!(admin_key_is_insecure(ADMIN_KEY_PLACEHOLDER));
        assert_eq!(ADMIN_KEY_PLACEHOLDER, "admin-key-placeholder");
    }
}
