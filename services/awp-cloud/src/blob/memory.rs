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

    async fn tamper_for_test(&self, sha256_hex: &str, bytes: &[u8]) -> Result<(), ApiError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| ApiError::Internal("mem_blob poisoned".into()))?;
        g.insert(sha256_hex.to_string(), bytes.to_vec());
        Ok(())
    }
}
