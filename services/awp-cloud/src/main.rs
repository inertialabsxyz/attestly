//! `awp-cloud` binary entrypoint. Reads configuration from environment
//! variables, constructs the Postgres + filesystem backends, runs database
//! migrations, mounts the static viewer assets at `/static`, and starts the
//! Axum server.
//!
//! Environment:
//!
//! - `DATABASE_URL` — Postgres connection string. Required.
//! - `BLOB_ROOT`    — filesystem path for blob storage. Defaults to `./data/blobs`.
//! - `BIND_ADDR`    — `host:port` to listen on. Defaults to `0.0.0.0:8080`.
//! - `AWP_CLOUD_BASE_URL` — Public URL embedded in share-link responses.
//!   Defaults to `https://app.awp-cloud.xyz`.
//! - `RUST_LOG`     — tracing filter. Defaults to `info`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use tower_http::trace::TraceLayer;

use awp_cloud::blob::filesystem::FsBlobStore;
use awp_cloud::store::postgres::PgDb;
use awp_cloud::{router, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,awp_cloud=debug".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let blob_root: PathBuf = std::env::var("BLOB_ROOT")
        .unwrap_or_else(|_| "./data/blobs".to_string())
        .into();
    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()
        .context("BIND_ADDR is not a valid socket address")?;

    tracing::info!(%database_url, blob_root = %blob_root.display(), %bind_addr, "starting awp-cloud");

    let db = PgDb::connect(&database_url, 20)
        .await
        .context("postgres connect")?;
    db.apply_migrations().await.context("apply migrations")?;

    let blob = FsBlobStore::new(&blob_root);
    let state = AppState::new(Arc::new(db), Arc::new(blob));

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
