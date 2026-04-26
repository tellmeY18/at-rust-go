//! Error types for AT Protocol repository operations.

use std::fmt;

/// Errors that can occur during repository operations.
#[derive(Debug)]
pub enum RepoError {
    /// The PDS returned an error response.
    Pds(String),
    /// The requested record was not found.
    NotFound,
    /// The provided AT-URI is malformed.
    InvalidAtUri(String),
    /// The blob exceeds the maximum allowed size.
    BlobTooLarge {
        /// Actual size in bytes.
        size: usize,
        /// Maximum allowed size in bytes.
        max: usize,
    },
    /// The provided TID is malformed.
    InvalidTid(String),
    /// A network error occurred communicating with the PDS.
    Network(reqwest::Error),
    /// An internal error occurred.
    Internal(anyhow::Error),
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pds(msg) => write!(f, "PDS error: {msg}"),
            Self::NotFound => write!(f, "record not found"),
            Self::InvalidAtUri(uri) => write!(f, "invalid AT-URI: {uri}"),
            Self::BlobTooLarge { size, max } => {
                write!(f, "blob too large: {size} bytes exceeds max {max} bytes")
            }
            Self::InvalidTid(msg) => write!(f, "invalid TID: {msg}"),
            Self::Network(err) => write!(f, "network error: {err}"),
            Self::Internal(err) => write!(f, "internal error: {err}"),
        }
    }
}

impl std::error::Error for RepoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Network(err) => Some(err),
            Self::Internal(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for RepoError {
    fn from(err: reqwest::Error) -> Self {
        Self::Network(err)
    }
}

impl From<anyhow::Error> for RepoError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_pds_error() {
        let err = RepoError::Pds("invalid token".to_string());
        assert_eq!(err.to_string(), "PDS error: invalid token");
    }

    #[test]
    fn display_not_found() {
        let err = RepoError::NotFound;
        assert_eq!(err.to_string(), "record not found");
    }

    #[test]
    fn display_invalid_at_uri() {
        let err = RepoError::InvalidAtUri("bad://uri".to_string());
        assert_eq!(err.to_string(), "invalid AT-URI: bad://uri");
    }

    #[test]
    fn display_blob_too_large() {
        let err = RepoError::BlobTooLarge {
            size: 2_000_000,
            max: 1_000_000,
        };
        assert_eq!(
            err.to_string(),
            "blob too large: 2000000 bytes exceeds max 1000000 bytes"
        );
    }

    #[test]
    fn display_invalid_tid() {
        let err = RepoError::InvalidTid("too short".to_string());
        assert_eq!(err.to_string(), "invalid TID: too short");
    }

    #[test]
    fn display_internal_error() {
        let err = RepoError::Internal(anyhow::anyhow!("something broke"));
        assert_eq!(err.to_string(), "internal error: something broke");
    }

    #[test]
    fn from_anyhow() {
        let err: RepoError = anyhow::anyhow!("test").into();
        assert!(matches!(err, RepoError::Internal(_)));
    }

    #[test]
    fn test_source_for_internal() {
        use std::error::Error;
        let err = RepoError::Internal(anyhow::anyhow!("x"));
        assert!(err.source().is_some());
    }

    #[test]
    fn test_source_for_not_found() {
        use std::error::Error;
        let err = RepoError::NotFound;
        assert!(err.source().is_none());
    }

    #[test]
    fn test_source_for_pds() {
        use std::error::Error;
        let err = RepoError::Pds("fail".to_string());
        assert!(err.source().is_none());
    }

    #[test]
    fn test_source_for_invalid_at_uri() {
        use std::error::Error;
        let err = RepoError::InvalidAtUri("bad".to_string());
        assert!(err.source().is_none());
    }

    #[test]
    fn test_source_for_blob_too_large() {
        use std::error::Error;
        let err = RepoError::BlobTooLarge { size: 100, max: 50 };
        assert!(err.source().is_none());
    }

    #[test]
    fn test_source_for_invalid_tid() {
        use std::error::Error;
        let err = RepoError::InvalidTid("bad".to_string());
        assert!(err.source().is_none());
    }
}
