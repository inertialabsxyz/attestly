//! Content-addressed blob storage abstraction.
//!
//! `attestly-cloud` stores the canonical attestation bytes in this layer, keyed by
//! their SHA-256. Two implementations:
//!
//! - [`memory::MemBlobStore`] — tests and `cargo run` default.
//! - [`filesystem::FsBlobStore`] — local docker-compose dev and the first
//!   staging deploy (Fly volume).
//!
//! An S3-compatible backend (Backblaze B2 / Cloudflare R2) is the production
//! target; it lands when the staging deploy moves off Fly volumes. The trait
//! is the seam — production swap is a single line change in `main.rs`.

pub mod filesystem;
pub mod memory;

use async_trait::async_trait;

use crate::error::ApiError;

#[async_trait]
pub trait BlobStore: Send + Sync + 'static {
    /// Store `bytes` under the SHA-256 derived key. Idempotent — re-storing
    /// the same key is a no-op (the bytes must match; if they don't we
    /// surface as `Internal`, since this should be impossible for honest
    /// callers).
    async fn put(&self, sha256_hex: &str, bytes: &[u8]) -> Result<(), ApiError>;

    /// Fetch the bytes for a previously-stored key, or `None` if the key is
    /// unknown.
    async fn get(&self, sha256_hex: &str) -> Result<Option<Vec<u8>>, ApiError>;

    /// Replace the bytes at a key. **For tests only** — production callers
    /// must not mutate at-rest bytes. The handler-layer tamper test calls
    /// this to simulate a corruption between write and read.
    async fn tamper_for_test(&self, sha256_hex: &str, bytes: &[u8]) -> Result<(), ApiError>;
}
