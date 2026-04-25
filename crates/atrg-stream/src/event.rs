//! Jetstream event types.
//!
//! These types model the JSON events emitted by the Jetstream firehose.
//! They are defined here (rather than depending on `atproto-jetstream`)
//! so that atrg controls the exact shape and serde behaviour.

use serde::{Deserialize, Serialize};

/// A single event from the Jetstream firehose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JetstreamEvent {
    /// The DID of the account this event is about.
    pub did: String,

    /// Unix microseconds timestamp.
    #[serde(default)]
    pub time_us: i64,

    /// The event kind (`"commit"`, `"identity"`, `"account"`).
    #[serde(default)]
    pub kind: String,

    /// Commit data (present for `"commit"` events).
    pub commit: Option<CommitData>,

    /// Identity data (present for `"identity"` events).
    pub identity: Option<serde_json::Value>,

    /// Account data (present for `"account"` events).
    pub account: Option<serde_json::Value>,
}

/// Data from a commit event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitData {
    /// The NSID collection (e.g. `"app.bsky.feed.post"`).
    pub collection: String,

    /// The record key.
    pub rkey: String,

    /// The operation: `"create"`, `"update"`, or `"delete"`.
    #[serde(default)]
    pub operation: String,

    /// The record payload (present for create/update, absent for delete).
    pub record: Option<serde_json::Value>,

    /// The CID of the record.
    pub cid: Option<String>,

    /// Revision string.
    pub rev: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_commit_event() {
        let json = r#"{
            "did": "did:plc:abc123",
            "time_us": 1700000000000000,
            "kind": "commit",
            "commit": {
                "collection": "app.bsky.feed.post",
                "rkey": "3k2la7fx2as2a",
                "operation": "create",
                "record": {"text": "hello world", "$type": "app.bsky.feed.post"},
                "cid": "bafyreig2",
                "rev": "123"
            }
        }"#;

        let event: JetstreamEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.did, "did:plc:abc123");
        assert_eq!(event.kind, "commit");
        assert_eq!(event.time_us, 1700000000000000);

        let commit = event.commit.unwrap();
        assert_eq!(commit.collection, "app.bsky.feed.post");
        assert_eq!(commit.rkey, "3k2la7fx2as2a");
        assert_eq!(commit.operation, "create");
        assert_eq!(commit.record.unwrap()["text"], "hello world");
        assert_eq!(commit.cid.as_deref(), Some("bafyreig2"));
    }

    #[test]
    fn deserialize_identity_event() {
        let json = r#"{
            "did": "did:plc:abc123",
            "time_us": 1700000000000000,
            "kind": "identity",
            "identity": {"handle": "alice.bsky.social"}
        }"#;

        let event: JetstreamEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.kind, "identity");
        assert!(event.commit.is_none());
        assert!(event.identity.is_some());
    }

    #[test]
    fn deserialize_minimal_event() {
        let json = r#"{"did": "did:plc:abc123"}"#;
        let event: JetstreamEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.did, "did:plc:abc123");
        assert_eq!(event.kind, "");
        assert_eq!(event.time_us, 0);
        assert!(event.commit.is_none());
        assert!(event.identity.is_none());
        assert!(event.account.is_none());
    }

    #[test]
    fn deserialize_delete_commit() {
        let json = r#"{
            "did": "did:plc:abc123",
            "kind": "commit",
            "commit": {
                "collection": "app.bsky.feed.post",
                "rkey": "3k2la7fx2as2a",
                "operation": "delete"
            }
        }"#;

        let event: JetstreamEvent = serde_json::from_str(json).unwrap();
        let commit = event.commit.unwrap();
        assert_eq!(commit.operation, "delete");
        assert!(commit.record.is_none());
        assert!(commit.cid.is_none());
    }

    #[test]
    fn roundtrip_serialization() {
        let event = JetstreamEvent {
            did: "did:plc:test".to_string(),
            time_us: 123456,
            kind: "commit".to_string(),
            commit: Some(CommitData {
                collection: "app.bsky.feed.post".to_string(),
                rkey: "abc".to_string(),
                operation: "create".to_string(),
                record: Some(serde_json::json!({"text": "hi"})),
                cid: Some("bafytest".to_string()),
                rev: None,
            }),
            identity: None,
            account: None,
        };

        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: JetstreamEvent = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.did, event.did);
        assert_eq!(deserialized.kind, event.kind);
        assert_eq!(
            deserialized.commit.as_ref().unwrap().collection,
            "app.bsky.feed.post"
        );
    }
}
