//! Blob upload helpers for AT Protocol PDS endpoints.

use crate::error::RepoError;
use crate::types::{BlobLink, BlobRef};

/// Upload a blob to the authenticated user's PDS.
///
/// Calls `com.atproto.repo.uploadBlob` with the given data and MIME type.
/// Returns a [`BlobRef`] that can be embedded in record fields.
pub async fn upload_blob(
    http: &reqwest::Client,
    pds_endpoint: &str,
    access_token: &str,
    data: Vec<u8>,
    mime_type: &str,
) -> Result<BlobRef, RepoError> {
    let url = format!("{}/xrpc/com.atproto.repo.uploadBlob", pds_endpoint);

    tracing::debug!(
        pds = pds_endpoint,
        mime = mime_type,
        size = data.len(),
        "uploading blob"
    );

    let response = http
        .post(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", mime_type)
        .body(data)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<no body>".to_string());
        return Err(RepoError::Pds(format!(
            "uploadBlob failed ({}): {}",
            status, body
        )));
    }

    let body: serde_json::Value = response.json().await?;

    let blob = body
        .get("blob")
        .ok_or_else(|| RepoError::Pds("uploadBlob response missing 'blob' field".to_string()))?;

    let cid = blob
        .get("ref")
        .and_then(|r| r.get("$link"))
        .and_then(|l| l.as_str())
        .ok_or_else(|| {
            RepoError::Pds("uploadBlob response missing 'ref.$link' field".to_string())
        })?;

    let mime = blob
        .get("mimeType")
        .and_then(|m| m.as_str())
        .unwrap_or("application/octet-stream");

    let size = blob.get("size").and_then(|s| s.as_u64()).unwrap_or(0);

    Ok(BlobRef {
        blob_type: "blob".to_string(),
        reference: BlobLink {
            link: cid.to_string(),
        },
        mime_type: mime.to_string(),
        size,
    })
}

/// Fetch an image from a URL and upload it as a blob to the user's PDS.
///
/// Downloads the resource at `image_url`, determines its MIME type from the
/// `Content-Type` response header (defaulting to `application/octet-stream`),
/// and uploads it via [`upload_blob`].
pub async fn upload_blob_from_url(
    http: &reqwest::Client,
    pds_endpoint: &str,
    access_token: &str,
    image_url: &str,
) -> Result<BlobRef, RepoError> {
    tracing::debug!(url = image_url, "fetching image for blob upload");

    let response = http.get(image_url).send().await?;

    if !response.status().is_success() {
        return Err(RepoError::Pds(format!(
            "failed to fetch image from {}: {}",
            image_url,
            response.status()
        )));
    }

    let mime_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let data = response.bytes().await?.to_vec();

    upload_blob(http, pds_endpoint, access_token, data, &mime_type).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // Both `upload_blob` and `upload_blob_from_url` are thin HTTP wrappers around
    // `com.atproto.repo.uploadBlob`. Full integration tests require a mock PDS server
    // (e.g. wiremock). The tests below cover the non-HTTP logic and response parsing paths.

    /// Helper: build a fake `uploadBlob` JSON response body.
    fn fake_upload_response(link: &str, mime: &str, size: u64) -> serde_json::Value {
        serde_json::json!({
            "blob": {
                "ref": { "$link": link },
                "mimeType": mime,
                "size": size
            }
        })
    }

    #[test]
    fn parse_blob_ref_from_valid_response() {
        let json = fake_upload_response("bafkrei1234", "image/png", 4096);

        let blob = json.get("blob").unwrap();
        let cid = blob
            .get("ref")
            .and_then(|r| r.get("$link"))
            .and_then(|l| l.as_str())
            .unwrap();
        let mime = blob
            .get("mimeType")
            .and_then(|m| m.as_str())
            .unwrap_or("application/octet-stream");
        let size = blob.get("size").and_then(|s| s.as_u64()).unwrap_or(0);

        let blob_ref = BlobRef {
            blob_type: "blob".to_string(),
            reference: BlobLink {
                link: cid.to_string(),
            },
            mime_type: mime.to_string(),
            size,
        };

        assert_eq!(blob_ref.reference.link, "bafkrei1234");
        assert_eq!(blob_ref.mime_type, "image/png");
        assert_eq!(blob_ref.size, 4096);
    }

    #[test]
    fn parse_blob_ref_missing_blob_field() {
        let json = serde_json::json!({});
        let result = json.get("blob");
        assert!(result.is_none(), "missing 'blob' field should yield None");
    }

    #[test]
    fn parse_blob_ref_missing_ref_link() {
        let json = serde_json::json!({ "blob": { "mimeType": "image/png", "size": 100 } });
        let blob = json.get("blob").unwrap();
        let cid = blob
            .get("ref")
            .and_then(|r| r.get("$link"))
            .and_then(|l| l.as_str());
        assert!(cid.is_none(), "missing ref.$link should yield None");
    }

    #[test]
    fn parse_blob_ref_defaults_mime_and_size() {
        // Response with ref but missing mimeType and size
        let json = serde_json::json!({
            "blob": {
                "ref": { "$link": "bafkreiblob" }
            }
        });
        let blob = json.get("blob").unwrap();
        let mime = blob
            .get("mimeType")
            .and_then(|m| m.as_str())
            .unwrap_or("application/octet-stream");
        let size = blob.get("size").and_then(|s| s.as_u64()).unwrap_or(0);

        assert_eq!(mime, "application/octet-stream");
        assert_eq!(size, 0);
    }

    #[test]
    fn upload_blob_url_construction() {
        let endpoint = "https://pds.example.com";
        let url = format!("{}/xrpc/com.atproto.repo.uploadBlob", endpoint);
        assert_eq!(
            url,
            "https://pds.example.com/xrpc/com.atproto.repo.uploadBlob"
        );
    }

    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ---- upload_blob ----

    #[tokio::test]
    async fn upload_blob_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .and(header("Authorization", "Bearer tok123"))
            .and(header("Content-Type", "image/png"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "blob": {
                    "ref": { "$link": "bafkreiblob" },
                    "mimeType": "image/png",
                    "size": 64
                }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let result = upload_blob(&http, &server.uri(), "tok123", vec![0u8; 64], "image/png").await;

        let blob_ref = result.unwrap();
        assert_eq!(blob_ref.reference.link, "bafkreiblob");
        assert_eq!(blob_ref.mime_type, "image/png");
        assert_eq!(blob_ref.size, 64);
        assert_eq!(blob_ref.blob_type, "blob");
    }

    #[tokio::test]
    async fn upload_blob_pds_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .respond_with(ResponseTemplate::new(413).set_body_string("payload too large"))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let result = upload_blob(&http, &server.uri(), "tok", vec![0u8; 10], "image/png").await;

        match result {
            Err(RepoError::Pds(msg)) => {
                assert!(
                    msg.contains("413"),
                    "error should contain status code: {msg}"
                );
                assert!(msg.contains("payload too large"));
            }
            other => panic!("expected Pds error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn upload_blob_missing_blob_field() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let result = upload_blob(&http, &server.uri(), "tok", vec![1], "image/png").await;

        match result {
            Err(RepoError::Pds(msg)) => assert!(msg.contains("missing 'blob'")),
            other => panic!("expected Pds error for missing blob field, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn upload_blob_missing_ref_link() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "blob": { "mimeType": "image/png", "size": 10 }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let result = upload_blob(&http, &server.uri(), "tok", vec![1], "image/png").await;

        match result {
            Err(RepoError::Pds(msg)) => assert!(msg.contains("ref.$link")),
            other => panic!("expected Pds error for missing ref.$link, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn upload_blob_defaults_mime_and_size() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "blob": {
                    "ref": { "$link": "bafkrei999" }
                }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let blob_ref = upload_blob(
            &http,
            &server.uri(),
            "tok",
            vec![1],
            "application/octet-stream",
        )
        .await
        .unwrap();

        assert_eq!(blob_ref.mime_type, "application/octet-stream");
        assert_eq!(blob_ref.size, 0);
    }

    // ---- upload_blob_from_url ----

    #[tokio::test]
    async fn upload_blob_from_url_success() {
        let server = MockServer::start().await;

        // Mock the image fetch endpoint
        Mock::given(method("GET"))
            .and(path("/images/photo.jpg"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "image/jpeg")
                    .set_body_bytes(vec![0xFFu8, 0xD8, 0xFF, 0xE0]),
            )
            .mount(&server)
            .await;

        // Mock the upload endpoint
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "blob": {
                    "ref": { "$link": "bafkreiimg" },
                    "mimeType": "image/jpeg",
                    "size": 4
                }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let image_url = format!("{}/images/photo.jpg", server.uri());
        let blob_ref = upload_blob_from_url(&http, &server.uri(), "tok", &image_url)
            .await
            .unwrap();

        assert_eq!(blob_ref.reference.link, "bafkreiimg");
        assert_eq!(blob_ref.mime_type, "image/jpeg");
        assert_eq!(blob_ref.size, 4);
    }

    #[tokio::test]
    async fn upload_blob_from_url_fetch_failure() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/images/gone.jpg"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let image_url = format!("{}/images/gone.jpg", server.uri());
        let result = upload_blob_from_url(&http, &server.uri(), "tok", &image_url).await;

        match result {
            Err(RepoError::Pds(msg)) => {
                assert!(msg.contains("failed to fetch image"));
                assert!(msg.contains("404"));
            }
            other => panic!("expected Pds error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn upload_blob_from_url_defaults_mime_type() {
        let server = MockServer::start().await;

        // Return response without Content-Type header
        Mock::given(method("GET"))
            .and(path("/data/blob"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 8]))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .and(header("Content-Type", "application/octet-stream"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "blob": {
                    "ref": { "$link": "bafkreidefault" },
                    "mimeType": "application/octet-stream",
                    "size": 8
                }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let url = format!("{}/data/blob", server.uri());
        let blob_ref = upload_blob_from_url(&http, &server.uri(), "tok", &url)
            .await
            .unwrap();

        assert_eq!(blob_ref.mime_type, "application/octet-stream");
    }
}
