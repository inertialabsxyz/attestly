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

/// The fully-wired application state. Cheap to clone (everything inside is
/// behind an `Arc`).
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<dyn Db>,
    pub blob: Arc<dyn BlobStore>,
}

impl AppState {
    /// Build an `AppState` with the in-memory `MemDb` and `MemBlobStore`
    /// backends. Used by the test suite and by `cargo run` in CI smoke mode.
    pub fn in_memory() -> Self {
        Self {
            db: Arc::new(MemDb::new()),
            blob: Arc::new(MemBlobStore::new()),
        }
    }

    /// Build an `AppState` from arbitrary backends — the wiring used by
    /// `main.rs` to plug in Postgres + filesystem at runtime.
    pub fn new(db: Arc<dyn Db>, blob: Arc<dyn BlobStore>) -> Self {
        Self { db, blob }
    }
}
