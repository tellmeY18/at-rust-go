//! AT Protocol URI parsing and construction.
//!
//! AT-URIs follow the format `at://{authority}/{collection}/{rkey}` where:
//! - `authority` is a DID (e.g. `did:plc:xyz123`)
//! - `collection` is an NSID (e.g. `app.bsky.feed.post`)
//! - `rkey` is the record key

use std::fmt;

use crate::error::RepoError;

/// Parsed AT Protocol URI.
///
/// Represents a fully-qualified reference to a record in an AT Protocol
/// repository: `at://{authority}/{collection}/{rkey}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AtUri {
    /// The DID of the repository owner (e.g. `did:plc:abc123`).
    pub authority: String,
    /// The collection NSID (e.g. `app.bsky.feed.post`).
    pub collection: String,
    /// The record key within the collection.
    pub rkey: String,
}

impl AtUri {
    /// Create a new AT-URI from its components.
    ///
    /// Validates that:
    /// - `authority` starts with `did:`
    /// - `collection` contains at least one `.`
    /// - `rkey` is non-empty
    pub fn new(
        authority: impl Into<String>,
        collection: impl Into<String>,
        rkey: impl Into<String>,
    ) -> Result<Self, RepoError> {
        let authority = authority.into();
        let collection = collection.into();
        let rkey = rkey.into();

        if !authority.starts_with("did:") {
            return Err(RepoError::InvalidAtUri(format!(
                "authority must start with 'did:', got '{authority}'"
            )));
        }

        if !collection.contains('.') {
            return Err(RepoError::InvalidAtUri(format!(
                "collection must be an NSID containing at least one '.', got '{collection}'"
            )));
        }

        if rkey.is_empty() {
            return Err(RepoError::InvalidAtUri(
                "rkey must be non-empty".to_string(),
            ));
        }

        Ok(Self {
            authority,
            collection,
            rkey,
        })
    }

    /// Parse an AT-URI string.
    ///
    /// Expected format: `at://{authority}/{collection}/{rkey}`
    pub fn parse(uri: &str) -> Result<Self, RepoError> {
        let stripped = uri.strip_prefix("at://").ok_or_else(|| {
            RepoError::InvalidAtUri(format!("AT-URI must start with 'at://', got '{uri}'"))
        })?;

        let mut parts = stripped.splitn(3, '/');

        let authority = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| RepoError::InvalidAtUri("missing authority in AT-URI".to_string()))?;

        let collection = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| RepoError::InvalidAtUri("missing collection in AT-URI".to_string()))?;

        let rkey = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| RepoError::InvalidAtUri("missing rkey in AT-URI".to_string()))?;

        Self::new(authority, collection, rkey)
    }
}

impl fmt::Display for AtUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "at://{}/{}/{}",
            self.authority, self.collection, self.rkey
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_uri() {
        let uri = AtUri::parse("at://did:plc:abc123/app.bsky.feed.post/3jt5tsfbx2s2a").unwrap();
        assert_eq!(uri.authority, "did:plc:abc123");
        assert_eq!(uri.collection, "app.bsky.feed.post");
        assert_eq!(uri.rkey, "3jt5tsfbx2s2a");
    }

    #[test]
    fn test_new_valid() {
        let uri = AtUri::new("did:plc:xyz", "com.example.record", "abc").unwrap();
        assert_eq!(uri.authority, "did:plc:xyz");
        assert_eq!(uri.collection, "com.example.record");
        assert_eq!(uri.rkey, "abc");
    }

    #[test]
    fn test_display() {
        let uri = AtUri::new("did:plc:abc", "app.bsky.feed.post", "rkey1").unwrap();
        assert_eq!(uri.to_string(), "at://did:plc:abc/app.bsky.feed.post/rkey1");
    }

    #[test]
    fn test_roundtrip() {
        let original = "at://did:plc:abc123/app.bsky.feed.post/3jt5tsfbx2s2a";
        let parsed = AtUri::parse(original).unwrap();
        assert_eq!(parsed.to_string(), original);
    }

    #[test]
    fn test_parse_missing_prefix() {
        let result = AtUri::parse("https://example.com/foo/bar");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_rkey() {
        let result = AtUri::parse("at://did:plc:abc/app.bsky.feed.post");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_collection() {
        let result = AtUri::parse("at://did:plc:abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_new_invalid_authority() {
        let result = AtUri::new("plc:abc", "app.bsky.feed.post", "rkey1");
        assert!(result.is_err());
    }

    #[test]
    fn test_new_invalid_collection_no_dot() {
        let result = AtUri::new("did:plc:abc", "nocollection", "rkey1");
        assert!(result.is_err());
    }

    #[test]
    fn test_new_empty_rkey() {
        let result = AtUri::new("did:plc:abc", "app.bsky.feed.post", "");
        assert!(result.is_err());
    }
}
