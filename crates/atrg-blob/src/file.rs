//! Filesystem-based blob store for development and testing.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::{compute_cid, BlobError, BlobStore};

/// A filesystem-backed blob store. Stores blobs as files named by their CID.
pub struct FileBlobStore {
    directory: PathBuf,
}

impl FileBlobStore {
    /// Create a new file blob store at the given directory.
    /// Creates the directory if it doesn't exist.
    pub async fn new(directory: impl Into<PathBuf>) -> Result<Self, BlobError> {
        let directory = directory.into();
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|e| BlobError::Storage(e.into()))?;
        Ok(Self { directory })
    }

    fn blob_path(&self, cid: &str) -> PathBuf {
        self.directory.join(cid)
    }
}

#[async_trait]
impl BlobStore for FileBlobStore {
    async fn put(&self, data: &[u8]) -> Result<String, BlobError> {
        let cid = compute_cid(data);
        let path = self.blob_path(&cid);
        if !path.exists() {
            tokio::fs::write(&path, data)
                .await
                .map_err(|e| BlobError::Storage(e.into()))?;
            tracing::debug!(cid = %cid, "stored blob to filesystem");
        }
        Ok(cid)
    }

    async fn get(&self, cid: &str) -> Result<Vec<u8>, BlobError> {
        let path = self.blob_path(cid);
        tokio::fs::read(&path)
            .await
            .map_err(|_| BlobError::NotFound(cid.to_string()))
    }

    async fn exists(&self, cid: &str) -> Result<bool, BlobError> {
        Ok(self.blob_path(cid).exists())
    }

    async fn delete(&self, cid: &str) -> Result<(), BlobError> {
        let path = self.blob_path(cid);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| BlobError::Storage(e.into()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_roundtrip() {
        let dir = std::env::temp_dir().join(format!("atrg_blob_test_{}", std::process::id()));
        let store = FileBlobStore::new(&dir).await.unwrap();

        let data = b"hello blob world";
        let cid = store.put(data).await.unwrap();
        assert!(cid.starts_with("sha256-"));

        let retrieved = store.get(&cid).await.unwrap();
        assert_eq!(retrieved, data);

        assert!(store.exists(&cid).await.unwrap());

        store.delete(&cid).await.unwrap();
        assert!(!store.exists(&cid).await.unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_dedup() {
        let dir = std::env::temp_dir().join(format!("atrg_blob_dedup_{}", std::process::id()));
        let store = FileBlobStore::new(&dir).await.unwrap();

        let data = b"same content";
        let cid1 = store.put(data).await.unwrap();
        let cid2 = store.put(data).await.unwrap();
        assert_eq!(cid1, cid2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
