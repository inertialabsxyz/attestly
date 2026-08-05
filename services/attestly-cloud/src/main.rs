//! `attestly-cloud` binary entrypoint. Reads configuration from environment
//! variables, constructs the Postgres + filesystem backends, runs database
//! migrations, mounts the static viewer assets at `/static`, and starts the
//! Axum server.
//!
//! Environment:
//!
//! - `DATABASE_URL` — Postgres connection string. Required.
//! - `BLOB_ROOT`    — filesystem path for blob storage. Defaults to `./data/blobs`.
//! - `BIND_ADDR`    — `host:port` to listen on. Defaults to `0.0.0.0:8080`.
//! - `ATTESTLY_CLOUD_BASE_URL` — Public URL embedded in share-link responses.
//!   Defaults to `https://app.attestly.xyz`.
//! - `ATTESTLY_ADMIN_KEY` — Shared secret protecting the admin routes.
//!   **Required**: the binary refuses to start if it is unset, blank, or still
//!   the `admin-key-placeholder` sentinel.
//! - `RUST_LOG`     — tracing filter. Defaults to `info`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use tower_http::trace::TraceLayer;

use attestly_cloud::blob::filesystem::FsBlobStore;
use attestly_cloud::store::postgres::PgDb;
use attestly_cloud::store::Db;
use attestly_cloud::stripe::LiveStripeClient;
use attestly_cloud::sweeper;
use attestly_cloud::{admin_key_is_insecure, router, AppState, BillingConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,attestly_cloud=debug".into()),
        )
        .init();

    // Refuse to boot on a placeholder admin key *before* touching Postgres, so
    // a misconfigured deploy fails immediately and loudly rather than serving
    // the admin routes behind a secret that is public knowledge.
    let billing = BillingConfig::from_env();
    if admin_key_is_insecure(&billing.admin_key) {
        tracing::error!(
            "refusing to start: ATTESTLY_ADMIN_KEY is unset or still the placeholder; \
             set it to a strong secret (e.g. `flyctl secrets set ATTESTLY_ADMIN_KEY=...`)"
        );
        anyhow::bail!("refusing to start: ATTESTLY_ADMIN_KEY must be set to a strong secret");
    }

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let blob_root: PathBuf = std::env::var("BLOB_ROOT")
        .unwrap_or_else(|_| "./data/blobs".to_string())
        .into();
    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()
        .context("BIND_ADDR is not a valid socket address")?;

    tracing::info!(%database_url, blob_root = %blob_root.display(), %bind_addr, "starting attestly-cloud");

    let db = PgDb::connect(&database_url, 20)
        .await
        .context("postgres connect")?;
    db.apply_migrations().await.context("apply migrations")?;

    // Create the blob root once, at boot. `BlobStore::health` deliberately only
    // *observes* the root (creating it there would mask an unmounted volume),
    // so without this a fresh local run would report degraded until the first
    // ingest. On Fly the volume mount already provides the directory.
    std::fs::create_dir_all(&blob_root)
        .with_context(|| format!("create blob root {}", blob_root.display()))?;
    let blob = FsBlobStore::new(&blob_root);
    let stripe = LiveStripeClient::from_env().context("stripe client")?;
    let db: Arc<dyn Db> = Arc::new(db);
    let state = AppState::new(db.clone(), Arc::new(blob), Arc::new(stripe), billing);

    // Run the retention sweeper as a background task. 24h cadence is
    // adequate granularity for day-scoped retention.
    let sweep_db = db.clone();
    tokio::spawn(async move {
        sweeper::run_forever(sweep_db, Duration::from_secs(24 * 60 * 60)).await;
    });

    let app: Router = router(state)
        .nest_service("/static", tower_http::services::ServeDir::new("web"))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("bind {bind_addr}"))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve")?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
