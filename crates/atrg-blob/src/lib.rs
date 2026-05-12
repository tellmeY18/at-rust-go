#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Content-addressed blob storage for at-rust-go.
//! Provides S3 and filesystem backends.

pub mod cid;
pub mod file;
#[cfg(feature = "s3")]
pub mod s3;

pub use cid::compute_cid;

use async_trait::async_trait;

/// Errors from blob store operations.
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    /// The requested blob was not found.
    #[error("blob not found: {0}")]
    NotFound(String),
    /// An error occurred in the underlying storage backend.
    #[error("blob store error: {0}")]
    Storage(#[from] anyhow::Error),
}

/// Trait for content-addressed blob storage backends.
#[async_trait]
pub trait BlobStore: Send + Sync + 'static {
    /// Store a blob, returning its content-addressed identifier (CID).
    async fn put(&self, data: &[u8]) -> Result<String, BlobError>;
    /// Retrieve a blob by CID.
    async fn get(&self, cid: &str) -> Result<Vec<u8>, BlobError>;
    /// Check if a blob exists.
    async fn exists(&self, cid: &str) -> Result<bool, BlobError>;
    /// Delete a blob by CID.
    async fn delete(&self, cid: &str) -> Result<(), BlobError>;
}
