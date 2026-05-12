//! S3-compatible blob store backend.

use async_trait::async_trait;

use crate::{compute_cid, BlobError, BlobStore};

/// Configuration for S3 blob storage.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct S3Config {
    /// S3 endpoint URL (e.g. "http://minio:9000")
    pub endpoint: String,
    /// Bucket name
    pub bucket: String,
    /// Region
    pub region: String,
    /// Access key
    pub access_key: String,
    /// Secret key
    pub secret_key: String,
    /// Use path-style addressing (required for MinIO)
    #[serde(default = "default_true")]
    pub path_style: bool,
}

fn default_true() -> bool {
    true
}

/// S3-compatible blob store.
pub struct S3BlobStore {
    bucket: s3::Bucket,
}

impl std::fmt::Debug for S3BlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3BlobStore").finish()
    }
}

impl S3BlobStore {
    /// Create a new S3 blob store from config.
    pub fn new(config: &S3Config) -> Result<Self, BlobError> {
        let region = s3::Region::Custom {
            region: config.region.clone(),
            endpoint: config.endpoint.clone(),
        };
        let credentials = s3::creds::Credentials::new(
            Some(&config.access_key),
            Some(&config.secret_key),
            None,
            None,
            None,
        )
        .map_err(|e| BlobError::Storage(e.into()))?;

        let mut bucket = *s3::Bucket::new(&config.bucket, region, credentials)
            .map_err(|e| BlobError::Storage(e.into()))?;

        if config.path_style {
            bucket = *bucket.with_path_style();
        }

        Ok(Self { bucket })
    }
}

#[async_trait]
impl BlobStore for S3BlobStore {
    async fn put(&self, data: &[u8]) -> Result<String, BlobError> {
        let cid = compute_cid(data);
        self.bucket
            .put_object(&cid, data)
            .await
            .map_err(|e| BlobError::Storage(e.into()))?;
        tracing::debug!(cid = %cid, "stored blob to S3");
        Ok(cid)
    }

    async fn get(&self, cid: &str) -> Result<Vec<u8>, BlobError> {
        let response = self
            .bucket
            .get_object(cid)
            .await
            .map_err(|e| BlobError::Storage(e.into()))?;
        if response.status_code() == 404 {
            return Err(BlobError::NotFound(cid.to_string()));
        }
        Ok(response.bytes().to_vec())
    }

    async fn exists(&self, cid: &str) -> Result<bool, BlobError> {
        match self.bucket.head_object(cid).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn delete(&self, cid: &str) -> Result<(), BlobError> {
        self.bucket
            .delete_object(cid)
            .await
            .map_err(|e| BlobError::Storage(e.into()))?;
        Ok(())
    }
}
