//! In-memory blob store. Tests and `cargo run`'s default.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::blob::BlobStore;
use crate::error::ApiError;

pub struct MemBlobStore {
    inner: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemBlobStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemBlobStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BlobStore for MemBlobStore {
    async fn put(&self, sha256_hex: &str, bytes: &[u8]) -> Result<(), ApiError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| ApiError::Internal("mem_blob poisoned".into()))?;
        match g.get(sha256_hex) {
            Some(existing) if existing == bytes => Ok(()),
            Some(_) => Err(ApiError::Internal(
                "blob collision: same key, different bytes".into(),
            )),
            None => {
                g.insert(sha256_hex.to_string(), bytes.to_vec());
                Ok(())
            }
        }
    }

    async fn get(&self, sha256_hex: &str) -> Result<Option<Vec<u8>>, ApiError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| ApiError::Internal("mem_blob poisoned".into()))?;
        Ok(g.get(sha256_hex).cloned())
    }

    async fn health(&self) -> Result<(), ApiError> {
        // The only way an in-memory store is unreachable is a poisoned lock —
        // the same condition `put`/`get` surface as `Internal`.
        let _guard = self
            .inner
            .lock()
            .map_err(|_| ApiError::Internal("mem_blob poisoned".into()))?;
        Ok(())
    }

    async fn tamper_for_test(&self, sha256_hex: &str, bytes: &[u8]) -> Result<(), ApiError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| ApiError::Internal("mem_blob poisoned".into()))?;
        g.insert(sha256_hex.to_string(), bytes.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_passes_on_a_live_store() {
        let store = MemBlobStore::new();
        assert!(store.health().await.is_ok());
    }

    #[tokio::test]
    async fn health_passes_after_a_put() {
        // The probe must not be disturbed by, or disturb, stored blobs.
        let store = MemBlobStore::new();
        let key = "a".repeat(64);
        store.put(&key, b"bytes").await.unwrap();
        assert!(store.health().await.is_ok());
        assert_eq!(store.get(&key).await.unwrap(), Some(b"bytes".to_vec()));
    }
}
