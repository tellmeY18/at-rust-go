//! Cursor-based pagination helpers.
//!
//! All list endpoints in atrg use cursor-based pagination with the format:
//! `?cursor=<opaque>&limit=<n>` and return `{"items": [...], "cursor": "<next>"}`.

use base64::Engine;
use serde::Deserialize;

/// Maximum items per page.
pub const MAX_LIMIT: u32 = 100;
/// Default items per page.
pub const DEFAULT_LIMIT: u32 = 50;

/// Pagination query parameters accepted by list endpoints.
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    /// Opaque cursor from a previous response.
    pub cursor: Option<String>,
    /// Number of items to return (max 100, default 50).
    pub limit: Option<u32>,
}

impl PaginationParams {
    /// Get the effective limit, clamped to [1, MAX_LIMIT].
    pub fn effective_limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }
}

/// Encode a cursor from a timestamp and record key.
pub fn encode_cursor(timestamp_ms: i64, rkey: &str) -> String {
    let raw = format!("{}:{}", timestamp_ms, rkey);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes())
}

/// Decode a cursor into (timestamp_ms, rkey).
pub fn decode_cursor(cursor: &str) -> Result<(i64, String), crate::error::AtrgError> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| crate::error::AtrgError::BadRequest("invalid cursor".to_string()))?;

    let s = String::from_utf8(bytes)
        .map_err(|_| crate::error::AtrgError::BadRequest("invalid cursor encoding".to_string()))?;

    let (ts_str, rkey) = s
        .split_once(':')
        .ok_or_else(|| crate::error::AtrgError::BadRequest("malformed cursor".to_string()))?;

    let ts: i64 = ts_str
        .parse()
        .map_err(|_| crate::error::AtrgError::BadRequest("invalid cursor timestamp".to_string()))?;

    Ok((ts, rkey.to_string()))
}

/// Build a paginated JSON response.
pub fn paginated_response<T: serde::Serialize>(
    items: Vec<T>,
    cursor: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "items": items,
        "cursor": cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let encoded = encode_cursor(1234567890, "abc123");
        let (ts, rkey) = decode_cursor(&encoded).unwrap();
        assert_eq!(ts, 1234567890);
        assert_eq!(rkey, "abc123");
    }

    #[test]
    fn decode_invalid_base64() {
        let result = decode_cursor("!!!not-base64!!!");
        assert!(result.is_err());
    }

    #[test]
    fn decode_missing_separator() {
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"noseparator");
        let result = decode_cursor(&encoded);
        assert!(result.is_err());
    }

    #[test]
    fn decode_non_numeric_timestamp() {
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"abc:key");
        let result = decode_cursor(&encoded);
        assert!(result.is_err());
    }

    #[test]
    fn effective_limit_clamps() {
        let p = PaginationParams {
            cursor: None,
            limit: Some(200),
        };
        assert_eq!(p.effective_limit(), 100);

        let p = PaginationParams {
            cursor: None,
            limit: Some(0),
        };
        assert_eq!(p.effective_limit(), 1);

        let p = PaginationParams {
            cursor: None,
            limit: None,
        };
        assert_eq!(p.effective_limit(), 50);
    }

    #[test]
    fn paginated_response_shape() {
        let resp = paginated_response(vec!["a", "b"], Some("cursor123".into()));
        assert!(resp["items"].is_array());
        assert_eq!(resp["cursor"], "cursor123");

        let resp2 = paginated_response(vec!["x"], None::<String>);
        assert!(resp2["cursor"].is_null());
    }
}
