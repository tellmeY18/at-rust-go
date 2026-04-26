//! High-level client for AT Protocol record repository operations.
//!
//! Wraps `com.atproto.repo.*` XRPC calls with typed helpers.

use serde::de::DeserializeOwned;
#[allow(unused_imports)]
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

    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn mock_repo(server: &MockServer) -> Repo {
        let http = reqwest::Client::new();
        Repo::new(&http, &server.uri(), "test_token", "did:plc:testuser")
    }

    // ---- get_record ----

    #[tokio::test]
    async fn get_record_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.repo.getRecord"))
            .and(query_param("repo", "did:plc:testuser"))
            .and(query_param("collection", "app.bsky.feed.post"))
            .and(query_param("rkey", "3k2la"))
            .and(header("Authorization", "Bearer test_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "uri": "at://did:plc:testuser/app.bsky.feed.post/3k2la",
                "cid": "bafyabc",
                "value": { "text": "hello world" }
            })))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let uri = AtUri::parse("at://did:plc:testuser/app.bsky.feed.post/3k2la").unwrap();
        let record: Record<serde_json::Value> = repo.get_record(&uri).await.unwrap();

        assert_eq!(record.uri, "at://did:plc:testuser/app.bsky.feed.post/3k2la");
        assert_eq!(record.cid, "bafyabc");
        assert_eq!(record.value["text"], "hello world");
    }

    #[tokio::test]
    async fn get_record_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.repo.getRecord"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": "RecordNotFound",
                "message": "not found"
            })))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let uri = AtUri::parse("at://did:plc:testuser/app.bsky.feed.post/missing").unwrap();
        let result: Result<Record<serde_json::Value>, _> = repo.get_record(&uri).await;

        assert!(matches!(result, Err(RepoError::NotFound)));
    }

    #[tokio::test]
    async fn get_record_pds_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.repo.getRecord"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal"))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let uri = AtUri::parse("at://did:plc:testuser/app.bsky.feed.post/rk").unwrap();
        let result: Result<Record<serde_json::Value>, _> = repo.get_record(&uri).await;

        match result {
            Err(RepoError::Pds(msg)) => assert!(msg.contains("500")),
            other => panic!("expected Pds error, got {:?}", other),
        }
    }

    // ---- list_records ----

    #[tokio::test]
    async fn list_records_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.repo.listRecords"))
            .and(query_param("repo", "did:plc:testuser"))
            .and(query_param("collection", "app.bsky.feed.post"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "records": [
                    { "uri": "at://did:plc:testuser/app.bsky.feed.post/1", "cid": "cid1", "value": { "text": "a" } },
                    { "uri": "at://did:plc:testuser/app.bsky.feed.post/2", "cid": "cid2", "value": { "text": "b" } }
                ],
                "cursor": "next123"
            })))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let page: Page<Record<serde_json::Value>> = repo
            .list_records("app.bsky.feed.post", None, None)
            .await
            .unwrap();

        assert_eq!(page.records.len(), 2);
        assert_eq!(page.records[0].value["text"], "a");
        assert_eq!(page.cursor.as_deref(), Some("next123"));
    }

    #[tokio::test]
    async fn list_records_with_cursor_and_limit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.repo.listRecords"))
            .and(query_param("cursor", "abc"))
            .and(query_param("limit", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "records": [],
                "cursor": null
            })))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let page: Page<Record<serde_json::Value>> = repo
            .list_records("app.bsky.feed.post", Some("abc"), Some(5))
            .await
            .unwrap();

        assert!(page.records.is_empty());
        assert!(page.cursor.is_none());
    }

    #[tokio::test]
    async fn list_records_pds_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.repo.listRecords"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let result: Result<Page<Record<serde_json::Value>>, _> =
            repo.list_records("col", None, None).await;

        assert!(matches!(result, Err(RepoError::Pds(_))));
    }

    // ---- create_record ----

    #[tokio::test]
    async fn create_record_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.createRecord"))
            .and(header("Authorization", "Bearer test_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "uri": "at://did:plc:testuser/app.bsky.feed.post/newrkey",
                "cid": "bafynew"
            })))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let record = serde_json::json!({ "text": "new post" });
        let strong = repo
            .create_record("app.bsky.feed.post", &record)
            .await
            .unwrap();

        assert_eq!(
            strong.uri,
            "at://did:plc:testuser/app.bsky.feed.post/newrkey"
        );
        assert_eq!(strong.cid, "bafynew");
    }

    #[tokio::test]
    async fn create_record_pds_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.createRecord"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let result = repo.create_record("col", &serde_json::json!({})).await;

        match result {
            Err(RepoError::Pds(msg)) => assert!(msg.contains("400")),
            other => panic!("expected Pds error, got {:?}", other),
        }
    }

    // ---- put_record ----

    #[tokio::test]
    async fn put_record_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.putRecord"))
            .and(header("Authorization", "Bearer test_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "uri": "at://did:plc:testuser/app.bsky.actor.profile/self",
                "cid": "bafyput"
            })))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let record = serde_json::json!({ "displayName": "Alice" });
        let strong = repo
            .put_record("app.bsky.actor.profile", "self", &record)
            .await
            .unwrap();

        assert_eq!(strong.cid, "bafyput");
    }

    #[tokio::test]
    async fn put_record_pds_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.putRecord"))
            .respond_with(ResponseTemplate::new(502).set_body_string("bad gateway"))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let result = repo.put_record("col", "rk", &serde_json::json!({})).await;

        assert!(matches!(result, Err(RepoError::Pds(_))));
    }

    // ---- delete_record ----

    #[tokio::test]
    async fn delete_record_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.deleteRecord"))
            .and(header("Authorization", "Bearer test_token"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let uri = AtUri::parse("at://did:plc:testuser/app.bsky.feed.post/3k2la").unwrap();
        repo.delete_record(&uri).await.unwrap();
    }

    #[tokio::test]
    async fn delete_record_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.deleteRecord"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let uri = AtUri::parse("at://did:plc:testuser/app.bsky.feed.post/gone").unwrap();
        let result = repo.delete_record(&uri).await;

        assert!(matches!(result, Err(RepoError::NotFound)));
    }

    #[tokio::test]
    async fn delete_record_pds_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.deleteRecord"))
            .respond_with(ResponseTemplate::new(500).set_body_string("error"))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let uri = AtUri::parse("at://did:plc:testuser/app.test/rk").unwrap();
        let result = repo.delete_record(&uri).await;

        assert!(matches!(result, Err(RepoError::Pds(_))));
    }

    // ---- upload_blob (via Repo) ----

    #[tokio::test]
    async fn upload_blob_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .and(header("Authorization", "Bearer test_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "blob": {
                    "ref": { "$link": "bafyblob123" },
                    "mimeType": "image/png",
                    "size": 2048
                }
            })))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let blob_ref = repo.upload_blob(vec![0u8; 100], "image/png").await.unwrap();

        assert_eq!(blob_ref.reference.link, "bafyblob123");
        assert_eq!(blob_ref.mime_type, "image/png");
        assert_eq!(blob_ref.size, 2048);
    }

    #[tokio::test]
    async fn upload_blob_pds_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .respond_with(ResponseTemplate::new(413).set_body_string("too large"))
            .mount(&server)
            .await;

        let repo = mock_repo(&server).await;
        let result = repo.upload_blob(vec![0u8; 100], "image/png").await;

        assert!(matches!(result, Err(RepoError::Pds(_))));
    }
}
