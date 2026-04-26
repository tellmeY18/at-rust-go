//! Firehose event types for `com.atproto.sync.subscribeRepos`.
//!
//! These types model the decoded CBOR frames received from the AT Protocol
//! relay firehose. Each frame is decoded from binary WebSocket messages
//! containing DAG-CBOR encoded headers and bodies.

use serde::{Deserialize, Serialize};

/// A decoded firehose event from the relay.
#[derive(Debug, Clone)]
pub enum FirehoseEvent {
    /// A repository commit containing record operations.
    Commit(FirehoseCommit),
    /// A handle change event.
    Handle {
        /// Sequence number for cursor tracking.
        seq: i64,
        /// DID of the account whose handle changed.
        did: String,
        /// The new handle.
        handle: String,
    },
    /// An identity update event.
    Identity {
        /// Sequence number for cursor tracking.
        seq: i64,
        /// DID of the account whose identity was updated.
        did: String,
    },
    /// A repository tombstone (account deletion).
    Tombstone {
        /// Sequence number for cursor tracking.
        seq: i64,
        /// DID of the deleted account.
        did: String,
    },
    /// Informational message from the relay.
    Info {
        /// The info event name.
        name: String,
        /// Optional human-readable message.
        message: Option<String>,
    },
}

impl FirehoseEvent {
    /// Return the sequence number if present.
    ///
    /// `Info` events do not carry a sequence number and return `None`.
    pub fn seq(&self) -> Option<i64> {
        match self {
            Self::Commit(c) => Some(c.seq),
            Self::Handle { seq, .. } => Some(*seq),
            Self::Identity { seq, .. } => Some(*seq),
            Self::Tombstone { seq, .. } => Some(*seq),
            Self::Info { .. } => None,
        }
    }
}

/// A repository commit with decoded operations.
#[derive(Debug, Clone)]
pub struct FirehoseCommit {
    /// Sequence number for cursor tracking.
    pub seq: i64,
    /// DID of the repository owner.
    pub repo: String,
    /// The repository revision.
    pub rev: String,
    /// Operations in this commit.
    pub ops: Vec<RepoOp>,
    /// Wall-clock time of the commit (ISO 8601).
    pub time: String,
}

/// A single operation within a commit.
#[derive(Debug, Clone)]
pub struct RepoOp {
    /// The action performed.
    pub action: OpAction,
    /// The path (collection/rkey).
    pub path: String,
    /// The decoded record value (if action is Create or Update).
    pub record: Option<serde_json::Value>,
    /// The CID of the record as a hex string.
    pub cid: Option<String>,
}

impl RepoOp {
    /// Extract the collection NSID from the path.
    ///
    /// The path format is `collection/rkey`. Returns an empty string if
    /// the path does not contain a slash.
    pub fn collection(&self) -> &str {
        self.path.split('/').next().unwrap_or("")
    }

    /// Extract the record key from the path.
    ///
    /// The path format is `collection/rkey`. Returns an empty string if
    /// the path does not contain a second segment.
    pub fn rkey(&self) -> &str {
        self.path.split('/').nth(1).unwrap_or("")
    }
}

/// The type of operation in a repository commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpAction {
    /// A new record was created.
    Create,
    /// An existing record was updated.
    Update,
    /// A record was deleted.
    Delete,
}

impl OpAction {
    /// Parse an action string from the firehose CBOR payload.
    ///
    /// Returns `None` for unrecognised action strings.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_from_commit() {
        let event = FirehoseEvent::Commit(FirehoseCommit {
            seq: 42,
            repo: "did:plc:abc".to_string(),
            rev: "rev1".to_string(),
            ops: vec![],
            time: "2024-01-01T00:00:00Z".to_string(),
        });
        assert_eq!(event.seq(), Some(42));
    }

    #[test]
    fn seq_from_handle() {
        let event = FirehoseEvent::Handle {
            seq: 99,
            did: "did:plc:abc".to_string(),
            handle: "alice.test".to_string(),
        };
        assert_eq!(event.seq(), Some(99));
    }

    #[test]
    fn seq_from_identity() {
        let event = FirehoseEvent::Identity {
            seq: 7,
            did: "did:plc:abc".to_string(),
        };
        assert_eq!(event.seq(), Some(7));
    }

    #[test]
    fn seq_from_tombstone() {
        let event = FirehoseEvent::Tombstone {
            seq: 100,
            did: "did:plc:abc".to_string(),
        };
        assert_eq!(event.seq(), Some(100));
    }

    #[test]
    fn seq_from_info_is_none() {
        let event = FirehoseEvent::Info {
            name: "OutdatedCursor".to_string(),
            message: Some("cursor too old".to_string()),
        };
        assert_eq!(event.seq(), None);
    }

    #[test]
    fn repo_op_collection_and_rkey() {
        let op = RepoOp {
            action: OpAction::Create,
            path: "app.bsky.feed.post/3k2la7fx2as2a".to_string(),
            record: None,
            cid: None,
        };
        assert_eq!(op.collection(), "app.bsky.feed.post");
        assert_eq!(op.rkey(), "3k2la7fx2as2a");
    }

    #[test]
    fn repo_op_empty_path() {
        let op = RepoOp {
            action: OpAction::Delete,
            path: String::new(),
            record: None,
            cid: None,
        };
        assert_eq!(op.collection(), "");
        assert_eq!(op.rkey(), "");
    }

    #[test]
    fn repo_op_no_rkey() {
        let op = RepoOp {
            action: OpAction::Update,
            path: "app.bsky.feed.post".to_string(),
            record: None,
            cid: None,
        };
        assert_eq!(op.collection(), "app.bsky.feed.post");
        assert_eq!(op.rkey(), "");
    }

    #[test]
    fn op_action_from_str() {
        assert_eq!(OpAction::parse("create"), Some(OpAction::Create));
        assert_eq!(OpAction::parse("update"), Some(OpAction::Update));
        assert_eq!(OpAction::parse("delete"), Some(OpAction::Delete));
        assert_eq!(OpAction::parse("unknown"), None);
        assert_eq!(OpAction::parse(""), None);
    }
}
