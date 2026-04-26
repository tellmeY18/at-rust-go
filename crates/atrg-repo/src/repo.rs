//! High-level client for AT Protocol record repository operations.
//!
//! Wraps `com.atproto.repo.*` XRPC calls with typed helpers.

use serde::de::DeserializeOwned;
use tracing::debug;

use crate::at_uri::AtUri;
use crate::blob;
use crate::error::RepoError;
use crate::tid::Tid;
use crate::types::{BlobRef, Page, Record, StrongRef};

/// High-level client for AT Protocol record repository operations.
///
/// Wraps `com.atproto.repo.*` XRPC calls with typed helpers.
/// Automatically uses the provided PDS endpoint for all operations.
///
/// # Example
///
/// ```rust,no_run
/// # use atrg_repo::Repo;
/// # async fn example(http: &reqwest::Client) {
/// let repo = Repo::new(http, "https://pds.example.com", "token", "did:plc:abc123");
/// let record = serde_json::json!({ "text": "Hello world" });
/// let strong_ref = repo.create_record("app.bsky.feed.post", &record).await.unwrap();
/// # }
/// ```
pub struct Repo {
    http: reqwest::Client,
    pds_endpoint: String,
    access_token: String,
    did: String,
}

impl Repo {
    /// Create a new `Repo` client with explicit parameters.
    pub fn new(http: &reqwest::Client, pds_endpoint: &str, access_token: &str, did: &str) -> Self {
        Self {
            http: http.clone(),
            pds_endpoint: pds_endpoint.trim_end_matches('/').to_string(),
            access_token: access_token.to_string(),
            did: did.to_string(),
        }
    }

    /// Create a `Repo` client from an authenticated session.
    pub fn from_session(
        http: &reqwest::Client,
        session: &atrg_auth::AtrgSession,
        pds_endpoint: &str,
    ) -> Self {
        Self::new(http, pds_endpoint, &session.access_token, &session.did)
    }

    /// Return a reference to the DID this repo operates on.
    pub fn did(&self) -> &str {
        &self.did
    }

    /// Return a reference to the PDS endpoint.
    pub fn pds_endpoint(&self) -> &str {
        &self.pds_endpoint
    }

    /// Get a record by AT-URI.
    ///
    /// Calls `com.atproto.repo.getRecord`.
    pub async fn get_record<T: DeserializeOwned>(
        &self,
        uri: &AtUri,
    ) -> Result<Record<T>, RepoError> {
        let url = format!("{}/xrpc/com.atproto.repo.getRecord", self.pds_endpoint);

        debug!(uri = %uri, "getting record");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[
                ("repo", uri.authority.as_str()),
                ("collection", uri.collection.as_str()),
                ("rkey", uri.rkey.as_str()),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 404 {
                return Err(RepoError::NotFound);
            }
            return Err(RepoError::Pds(format!(
                "getRecord failed ({}): {}",
                status, body
            )));
        }

        let json: serde_json::Value = resp.json().await?;

        let record_uri = json["uri"].as_str().unwrap_or_default().to_string();
        let cid = json["cid"].as_str().unwrap_or_default().to_string();
        let value: T = serde_json::from_value(json["value"].clone())
            .map_err(|e| RepoError::Internal(e.into()))?;

        Ok(Record {
            uri: record_uri,
            cid,
            value,
        })
    }

    /// List records in a collection.
    ///
    /// Calls `com.atproto.repo.listRecords`.
    pub async fn list_records<T: DeserializeOwned>(
        &self,
        collection: &str,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<Record<T>>, RepoError> {
        let url = format!("{}/xrpc/com.atproto.repo.listRecords", self.pds_endpoint);

        debug!(collection, cursor, limit, "listing records");

        let mut query = vec![("repo", self.did.as_str()), ("collection", collection)];

        let limit_str;
        if let Some(l) = limit {
            limit_str = l.to_string();
            query.push(("limit", &limit_str));
        }

        if let Some(c) = cursor {
            query.push(("cursor", c));
        }

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&query)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RepoError::Pds(format!(
                "listRecords failed ({}): {}",
                status, body
            )));
        }

        let json: serde_json::Value = resp.json().await?;

        let cursor_out = json["cursor"].as_str().map(String::from);

        let records_json = json["records"].as_array().cloned().unwrap_or_default();

        let mut records = Vec::with_capacity(records_json.len());
        for r in records_json {
            let uri = r["uri"].as_str().unwrap_or_default().to_string();
            let cid = r["cid"].as_str().unwrap_or_default().to_string();
            let value: T = serde_json::from_value(r["value"].clone())
                .map_err(|e| RepoError::Internal(e.into()))?;
            records.push(Record { uri, cid, value });
        }

        Ok(Page {
            records,
            cursor: cursor_out,
        })
    }

    /// Create a new record with an auto-generated TID as the rkey.
    ///
    /// Calls `com.atproto.repo.createRecord`.
    pub async fn create_record(
        &self,
        collection: &str,
        record: &serde_json::Value,
    ) -> Result<StrongRef, RepoError> {
        let url = format!("{}/xrpc/com.atproto.repo.createRecord", self.pds_endpoint);

        debug!(collection, "creating record");

        let body = serde_json::json!({
            "repo": self.did,
            "collection": collection,
            "record": record,
        });

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RepoError::Pds(format!(
                "createRecord failed ({}): {}",
                status, body
            )));
        }

        let json: serde_json::Value = resp.json().await?;

        Ok(StrongRef {
            uri: json["uri"].as_str().unwrap_or_default().to_string(),
            cid: json["cid"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// Create or update a record at a specific rkey.
    ///
    /// Calls `com.atproto.repo.putRecord`.
    pub async fn put_record(
        &self,
        collection: &str,
        rkey: &str,
        record: &serde_json::Value,
    ) -> Result<StrongRef, RepoError> {
        let url = format!("{}/xrpc/com.atproto.repo.putRecord", self.pds_endpoint);

        debug!(collection, rkey, "putting record");

        let body = serde_json::json!({
            "repo": self.did,
            "collection": collection,
            "rkey": rkey,
            "record": record,
        });

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RepoError::Pds(format!(
                "putRecord failed ({}): {}",
                status, body
            )));
        }

        let json: serde_json::Value = resp.json().await?;

        Ok(StrongRef {
            uri: json["uri"].as_str().unwrap_or_default().to_string(),
            cid: json["cid"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// Delete a record.
    ///
    /// Calls `com.atproto.repo.deleteRecord`.
    pub async fn delete_record(&self, uri: &AtUri) -> Result<(), RepoError> {
        let url = format!("{}/xrpc/com.atproto.repo.deleteRecord", self.pds_endpoint);

        debug!(%uri, "deleting record");

        let body = serde_json::json!({
            "repo": uri.authority,
            "collection": uri.collection,
            "rkey": uri.rkey,
        });

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 404 {
                return Err(RepoError::NotFound);
            }
            return Err(RepoError::Pds(format!(
                "deleteRecord failed ({}): {}",
                status, body
            )));
        }

        Ok(())
    }

    /// Upload a blob to the authenticated user's PDS.
    ///
    /// Delegates to [`blob::upload_blob`].
    pub async fn upload_blob(&self, data: Vec<u8>, mime_type: &str) -> Result<BlobRef, RepoError> {
        blob::upload_blob(
            &self.http,
            &self.pds_endpoint,
            &self.access_token,
            data,
            mime_type,
        )
        .await
    }

    /// Generate a new [`Tid`] for use as a record key.
    pub fn new_tid() -> Tid {
        Tid::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_new_trims_trailing_slash() {
        let http = reqwest::Client::new();
        let repo = Repo::new(&http, "https://pds.example.com/", "tok", "did:plc:abc");
        assert_eq!(repo.pds_endpoint(), "https://pds.example.com");
    }

    #[test]
    fn test_repo_new_no_trailing_slash_unchanged() {
        let http = reqwest::Client::new();
        let repo = Repo::new(&http, "https://pds.example.com", "tok", "did:plc:abc");
        assert_eq!(repo.pds_endpoint(), "https://pds.example.com");
    }

    #[test]
    fn test_repo_did() {
        let http = reqwest::Client::new();
        let repo = Repo::new(&http, "https://pds.example.com", "tok", "did:plc:abc");
        assert_eq!(repo.did(), "did:plc:abc");
    }

    #[test]
    fn test_from_session() {
        use atrg_auth::{AtrgSession, AuthSource};

        let session = AtrgSession {
            did: "did:plc:session123".to_string(),
            handle: "alice.test".to_string(),
            access_token: "access_tok_xyz".to_string(),
            refresh_token: Some("ref_tok".to_string()),
            expires_at: 9999999999,
            source: AuthSource::Atrg,
        };

        let http = reqwest::Client::new();
        let repo = Repo::from_session(&http, &session, "https://pds.example.com/");

        assert_eq!(repo.did(), "did:plc:session123");
        assert_eq!(repo.pds_endpoint(), "https://pds.example.com");
    }

    #[test]
    fn test_from_session_atproto_jwt_source() {
        use atrg_auth::{AtrgSession, AuthSource};

        let session = AtrgSession {
            did: "did:web:bob.test".to_string(),
            handle: "bob.test".to_string(),
            access_token: "jwt_token".to_string(),
            refresh_token: None,
            expires_at: 9999999999,
            source: AuthSource::AtprotoJwt,
        };

        let http = reqwest::Client::new();
        let repo = Repo::from_session(&http, &session, "https://other-pds.example.com");

        assert_eq!(repo.did(), "did:web:bob.test");
        assert_eq!(repo.pds_endpoint(), "https://other-pds.example.com");
    }

    #[test]
    fn test_new_tid_returns_valid() {
        let tid = Repo::new_tid();
        assert_eq!(tid.as_str().len(), 13);
    }

    #[test]
    fn test_new_tid_parses_back() {
        let tid = Repo::new_tid();
        let parsed = Tid::parse(tid.as_str());
        assert!(parsed.is_ok(), "generated TID should parse successfully");
        assert_eq!(parsed.unwrap().as_str(), tid.as_str());
    }

    #[test]
    fn test_new_tid_successive_are_distinct() {
        let a = Repo::new_tid();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = Repo::new_tid();
        assert_ne!(a.as_str(), b.as_str());
    }

    // HTTP integration tests for get_record, list_records, create_record,
    // put_record, delete_record, and upload_blob require a mock PDS server.
    // These methods are thin XRPC wrappers with no pre-validation logic
    // that can be unit-tested independently of the network call.
    //
    // To test them, add `mockito` to [dev-dependencies] and stand up a
    // mock PDS that responds to `/xrpc/com.atproto.repo.*` endpoints.
}
