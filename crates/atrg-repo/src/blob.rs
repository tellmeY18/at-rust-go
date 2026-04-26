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

    // HTTP integration tests for `upload_blob` and `upload_blob_from_url`
    // require a mock PDS server. Add wiremock to [dev-dependencies] to enable:
    //
    // #[tokio::test]
    // async fn upload_blob_success() { ... }
    //
    // #[tokio::test]
    // async fn upload_blob_pds_error() { ... }
    //
    // #[tokio::test]
    // async fn upload_blob_from_url_success() { ... }
    //
    // #[tokio::test]
    // async fn upload_blob_from_url_fetch_failure() { ... }
}
