//! Filesystem-backed blob store. Used by docker-compose dev and the initial
//! Fly volume deploy. Production target is S3-compatible, but the trait seam
//! means swapping is a one-liner in `main.rs`.
//!
//! Layout: `<root>/<aa>/<bb>/<full-hex>`. Sharded two levels on the hash
//! prefix so we don't get a million-file flat directory.

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::blob::BlobStore;
use crate::error::ApiError;

pub struct FsBlobStore {
    root: PathBuf,
}

impl FsBlobStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, hex: &str) -> PathBuf {
        // Caller has already validated `hex` length & charset.
        let (a, rest) = hex.split_at(2);
        let (b, _) = rest.split_at(2);
        self.root.join(a).join(b).join(hex)
    }
}

#[async_trait]
impl BlobStore for FsBlobStore {
    async fn put(&self, sha256_hex: &str, bytes: &[u8]) -> Result<(), ApiError> {
        if sha256_hex.len() != 64 || !sha256_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ApiError::InvalidRequest(
                "blob key must be 64 hex chars".into(),
            ));
        }
        let path = self.path_for(sha256_hex);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| ApiError::Internal(format!("blob mkdir: {e}")))?;
        }
        if fs::try_exists(&path)
            .await
            .map_err(|e| ApiError::Internal(format!("blob exists: {e}")))?
        {
            // Idempotent: trust the content addressing and skip.
            return Ok(());
        }
        // Write to a sibling temp and rename so a crash mid-write doesn't leave
        // a half-blob at the canonical path.
        let tmp = path.with_extension("tmp");
        let mut f = fs::File::create(&tmp)
            .await
            .map_err(|e| ApiError::Internal(format!("blob create: {e}")))?;
        f.write_all(bytes)
            .await
            .map_err(|e| ApiError::Internal(format!("blob write: {e}")))?;
        f.flush()
            .await
            .map_err(|e| ApiError::Internal(format!("blob flush: {e}")))?;
        drop(f);
        fs::rename(&tmp, &path)
            .await
            .map_err(|e| ApiError::Internal(format!("blob rename: {e}")))?;
        Ok(())
    }

    async fn get(&self, sha256_hex: &str) -> Result<Option<Vec<u8>>, ApiError> {
        let path = self.path_for(sha256_hex);
        match fs::read(&path).await {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ApiError::Internal(format!("blob read: {e}"))),
        }
    }

    async fn health(&self) -> Result<(), ApiError> {
        // The failure this catches is the Fly volume not being mounted at
        // `BLOB_ROOT` — the store is then silently writing into the container's
        // ephemeral layer. `create_dir_all` is a no-op when the root already
        // exists, and on a missing mount it fails, which is exactly the signal
        // we want. No blob is written, so this is safe at healthcheck cadence.
        fs::create_dir_all(&self.root)
            .await
            .map_err(|e| ApiError::Internal(format!("blob root unavailable: {e}")))?;
        let meta = fs::metadata(&self.root)
            .await
            .map_err(|e| ApiError::Internal(format!("blob root unavailable: {e}")))?;
        if meta.permissions().readonly() {
            return Err(ApiError::Internal("blob root is read-only".into()));
        }
        Ok(())
    }

    async fn tamper_for_test(&self, sha256_hex: &str, bytes: &[u8]) -> Result<(), ApiError> {
        let path = self.path_for(sha256_hex);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| ApiError::Internal(format!("blob mkdir: {e}")))?;
        }
        fs::write(&path, bytes)
            .await
            .map_err(|e| ApiError::Internal(format!("blob tamper write: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_passes_on_a_usable_root() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path());
        assert!(store.health().await.is_ok());
    }

    #[tokio::test]
    async fn health_creates_a_missing_root_without_writing_a_blob() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("not-yet-created");
        let store = FsBlobStore::new(&root);
        assert!(store.health().await.is_ok());
        // The probe is liveness-only: it must leave the store empty.
        let mut entries = fs::read_dir(&root).await.unwrap();
        assert!(
            entries.next_entry().await.unwrap().is_none(),
            "health probe must not write any blob"
        );
    }

    #[tokio::test]
    async fn health_fails_when_the_root_is_not_a_directory() {
        // Stands in for the real failure mode this probe exists to catch: the
        // Fly volume is not mounted at BLOB_ROOT, so the path is unusable.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a-file");
        fs::write(&file, b"not a directory").await.unwrap();
        let store = FsBlobStore::new(&file);
        let err = store.health().await.unwrap_err();
        assert!(
            matches!(err, ApiError::Internal(ref m) if m.contains("blob root unavailable")),
            "expected a blob-root failure, got {err:?}"
        );
    }
}
