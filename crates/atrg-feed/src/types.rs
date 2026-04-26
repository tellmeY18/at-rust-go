//! Types for feed generator responses.
//!
//! Contains the wire types for `app.bsky.feed.describeFeedGenerator` and
//! `app.bsky.feed.getFeedSkeleton` XRPC endpoints.

use serde::{Deserialize, Serialize};

/// Configuration for a single feed.
#[derive(Debug, Clone, Deserialize)]
pub struct FeedConfig {
    /// Short identifier for the feed (e.g. `"my-feed"`).
    pub id: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Optional description shown to users.
    pub description: Option<String>,
    /// Optional avatar blob reference (CID link).
    pub avatar: Option<String>,
}

/// The skeleton response for `app.bsky.feed.getFeedSkeleton`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedSkeleton {
    /// The feed items (post AT-URIs).
    pub feed: Vec<SkeletonItem>,
    /// Cursor for pagination. `None` means no more results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// A single item in a feed skeleton.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkeletonItem {
    /// AT-URI of the post (e.g. `at://did:plc:xxx/app.bsky.feed.post/rkey`).
    pub post: String,
}

impl SkeletonItem {
    /// Create a new skeleton item from an AT-URI string.
    pub fn new(post_uri: impl Into<String>) -> Self {
        Self {
            post: post_uri.into(),
        }
    }
}

/// Description of a single feed within a feed generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedDescription {
    /// The AT-URI of the feed generator record.
    pub uri: String,
    /// CID of the feed generator record (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
}

/// Response body for `app.bsky.feed.describeFeedGenerator`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeFeedGeneratorResponse {
    /// DID of the feed generator service.
    pub did: String,
    /// List of feeds served by this generator.
    pub feeds: Vec<FeedDescription>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_item_new() {
        let item = SkeletonItem::new("at://did:plc:abc/app.bsky.feed.post/123");
        assert_eq!(item.post, "at://did:plc:abc/app.bsky.feed.post/123");
    }

    #[test]
    fn feed_skeleton_serializes_without_cursor() {
        let skeleton = FeedSkeleton {
            feed: vec![SkeletonItem::new("at://did:plc:abc/app.bsky.feed.post/1")],
            cursor: None,
        };
        let json = serde_json::to_value(&skeleton).unwrap();
        assert!(json.get("cursor").is_none());
        assert_eq!(
            json["feed"][0]["post"],
            "at://did:plc:abc/app.bsky.feed.post/1"
        );
    }

    #[test]
    fn feed_skeleton_serializes_with_cursor() {
        let skeleton = FeedSkeleton {
            feed: vec![],
            cursor: Some("abc123".to_string()),
        };
        let json = serde_json::to_value(&skeleton).unwrap();
        assert_eq!(json["cursor"], "abc123");
    }

    #[test]
    fn describe_response_serializes() {
        let resp = DescribeFeedGeneratorResponse {
            did: "did:web:feeds.example.com".to_string(),
            feeds: vec![FeedDescription {
                uri: "at://did:web:feeds.example.com/app.bsky.feed.generator/my-feed".to_string(),
                cid: None,
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["did"], "did:web:feeds.example.com");
        assert_eq!(json["feeds"].as_array().unwrap().len(), 1);
        assert!(json["feeds"][0].get("cid").is_none());
    }
}
