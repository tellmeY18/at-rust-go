//! Shared types for AT Protocol record repository operations.

use serde::{Deserialize, Serialize};

/// A strong reference to a record (URI + CID).
///
/// Used as the return type for record creation and update operations,
/// providing both the AT-URI and the content hash of the written record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrongRef {
    /// The AT-URI of the record (e.g. `at://did:plc:xxx/app.bsky.feed.post/rkey`).
    pub uri: String,
    /// The CID (content identifier) hash of the record.
    pub cid: String,
}

/// A reference to an uploaded blob.
///
/// Returned by blob upload operations. Embed this in record values
/// to reference uploaded media (images, video, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobRef {
    /// Always `"blob"`.
    #[serde(rename = "$type")]
    pub blob_type: String,
    /// The CID link to the blob content.
    #[serde(rename = "ref")]
    pub reference: BlobLink,
    /// The MIME type of the blob (e.g. `"image/png"`).
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// The size of the blob in bytes.
    pub size: u64,
}

/// A CID link used within a [`BlobRef`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobLink {
    /// The CID string.
    #[serde(rename = "$link")]
    pub link: String,
}

/// A paginated response of records.
///
/// AT Protocol APIs use cursor-based pagination. When `cursor` is `Some`,
/// pass it to the next request to fetch the following page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    /// The records in this page.
    pub records: Vec<T>,
    /// Opaque cursor for fetching the next page. `None` if this is the last page.
    pub cursor: Option<String>,
}

/// A single record with its repository metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record<T> {
    /// The AT-URI of the record.
    pub uri: String,
    /// The CID (content identifier) hash of the record.
    pub cid: String,
    /// The record value.
    pub value: T,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_ref_serde_round_trip() {
        let strong = StrongRef {
            uri: "at://did:plc:abc123/app.bsky.feed.post/3k2la".into(),
            cid: "bafyreib2rxk3rybkba".into(),
        };
        let json = serde_json::to_string(&strong).unwrap();
        let decoded: StrongRef = serde_json::from_str(&json).unwrap();
        assert_eq!(strong, decoded);
    }

    #[test]
    fn blob_ref_serde_round_trip() {
        let blob = BlobRef {
            blob_type: "blob".into(),
            reference: BlobLink {
                link: "bafyreib2rxk3rybkba".into(),
            },
            mime_type: "image/png".into(),
            size: 12345,
        };
        let json = serde_json::to_string(&blob).unwrap();
        let decoded: BlobRef = serde_json::from_str(&json).unwrap();
        assert_eq!(blob, decoded);
    }

    #[test]
    fn blob_ref_json_field_names() {
        let blob = BlobRef {
            blob_type: "blob".into(),
            reference: BlobLink {
                link: "bafyreib2rxk3rybkba".into(),
            },
            mime_type: "image/jpeg".into(),
            size: 999,
        };
        let val: serde_json::Value = serde_json::to_value(&blob).unwrap();
        assert!(val.get("$type").is_some());
        assert!(val.get("ref").is_some());
        assert!(val.get("mimeType").is_some());
        let ref_obj = val.get("ref").unwrap();
        assert!(ref_obj.get("$link").is_some());
    }

    #[test]
    fn page_serde_round_trip() {
        let page: Page<StrongRef> = Page {
            records: vec![StrongRef {
                uri: "at://did:plc:abc/col.name/rkey".into(),
                cid: "bafyxyz".into(),
            }],
            cursor: Some("next_cursor".into()),
        };
        let json = serde_json::to_string(&page).unwrap();
        let decoded: Page<StrongRef> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.records.len(), 1);
        assert_eq!(decoded.cursor.as_deref(), Some("next_cursor"));
    }

    #[test]
    fn page_last_page_has_no_cursor() {
        let page: Page<StrongRef> = Page {
            records: vec![],
            cursor: None,
        };
        let json = serde_json::to_string(&page).unwrap();
        let decoded: Page<StrongRef> = serde_json::from_str(&json).unwrap();
        assert!(decoded.cursor.is_none());
    }

    #[test]
    fn record_serde_round_trip() {
        let record: Record<serde_json::Value> = Record {
            uri: "at://did:plc:abc/col.name/rkey".into(),
            cid: "bafyxyz".into(),
            value: serde_json::json!({"text": "hello"}),
        };
        let json = serde_json::to_string(&record).unwrap();
        let decoded: Record<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.uri, "at://did:plc:abc/col.name/rkey");
        assert_eq!(decoded.value["text"], "hello");
    }
}
