//! `awp-cloud` — hosted AWP ingest, search, and share-link service.
//!
//! The HTTP contract is locked in [`API.md`](../API.md) at the service root.
//! Step 3 (LangGraph SDK `CloudSink`) and Step 4 (Stripe billing) both code
//! against that contract.
//!
//! ## Architecture
//!
//! - [`store::Db`] — storage of attestation metadata, accounts, API keys,
//!   share links, and usage. Production impl is Postgres
//!   ([`store::postgres::PgDb`]); tests and the in-memory dev mode use
//!   [`store::memory::MemDb`].
//! - [`blob::BlobStore`] — content-addressed storage of canonical attestation
//!   bytes. Production impl is S3-compatible
//!   ([`blob::filesystem::FsBlobStore`] in local dev; an S3 impl arrives with
//!   the staging deploy). Tests use [`blob::memory::MemBlobStore`].
//! - [`canonical`] — server-side re-verification of attestation signatures
//!   using `awp-core`. The same code path runs at ingest **and** at retrieval
//!   so an attestation can never round-trip through the service without
//!   re-verifying. This is the load-bearing tamper guarantee.
//! - [`auth`] — `x-api-key` header validation, Argon2id hashing of stored
//!   keys, per-account scoping.
//! - [`handlers`] — Axum route handlers. One submodule per resource family.
//!
//! ## Construction
//!
//! Tests construct the [`Router`] by hand with in-memory backends so the test
//! suite has no external dependencies:
//!
//! ```ignore
//! use awp_cloud::{router, AppState};
//! use std::sync::Arc;
//!
//! let state = AppState::in_memory();
//! let app = router(state);
//! ```
//!
//! The binary entrypoint reads environment variables and constructs the
//! Postgres + filesystem (or S3) backends instead.

pub mod auth;
pub mod blob;
pub mod canonical;
pub mod error;
pub mod handlers;
pub mod state;
pub mod store;
pub mod stripe;
pub mod sweeper;
pub mod web;

pub use state::{AppState, BillingConfig};

use axum::response::Redirect;
use axum::routing::{delete, get, post};
use axum::Router;

/// Build the application router from a fully-constructed [`AppState`].
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::permanent("/dashboard") }))
        .route("/healthz", get(handlers::health::healthz))
        .route("/v1/attestations", post(handlers::attestations::ingest))
        .route("/v1/attestations", get(handlers::attestations::search))
        .route("/v1/attestations/:id", get(handlers::attestations::get_one))
        .route("/v1/share-links", post(handlers::share_links::create))
        .route(
            "/v1/share-links/:token",
            get(handlers::share_links::get_json),
        )
        .route(
            "/v1/share-links/:token",
            delete(handlers::share_links::revoke),
        )
        .route("/share/:token", get(handlers::share_links::render_page))
        .route("/v1/export", get(handlers::export::stream))
        .route(
            "/v1/billing/checkout",
            post(handlers::billing::create_checkout),
        )
        .route("/v1/billing/portal", post(handlers::billing::create_portal))
        .route(
            "/v1/billing/webhook",
            post(handlers::billing::handle_stripe_webhook),
        )
        .route(
            "/v1/admin/bill-period",
            post(handlers::billing::bill_period),
        )
        .route(
            "/billing/checkout",
            get(handlers::billing::landing_checkout),
        )
        .route("/v1/account", get(handlers::account::get))
        .route("/v1/account/usage", get(handlers::account::usage))
        .route("/v1/account/api-keys", get(handlers::account::list_keys))
        .route("/v1/account/api-keys", post(handlers::account::create_key))
        .route(
            "/v1/account/api-keys/:id",
            delete(handlers::account::revoke_key),
        )
        .route("/dashboard", get(handlers::account::render_dashboard))
        .route("/quickstart", get(handlers::quickstart::render_quickstart))
        .route("/v1/account/signup", post(handlers::quickstart::signup))
        .route(
            "/v1/telemetry/usage",
            post(handlers::telemetry::ingest_usage),
        )
        .route(
            "/v1/admin/telemetry/conversion-ready",
            get(handlers::telemetry::conversion_ready),
        )
        .with_state(state)
}
