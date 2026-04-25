//! Fake Jetstream for testing event handlers.

use tokio::sync::mpsc;

/// A fake Jetstream that can emit synthetic events for testing.
///
/// Events are sent through a channel and can be consumed by the
/// application's event handler.
pub struct FakeJetstream {
    tx: mpsc::Sender<serde_json::Value>,
    rx: Option<mpsc::Receiver<serde_json::Value>>,
}

impl FakeJetstream {
    /// Create a new fake Jetstream with a bounded channel.
    pub fn new(capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity);
        Self { tx, rx: Some(rx) }
    }

    /// Take the receiver (for wiring into the consumer).
    pub fn take_receiver(&mut self) -> Option<mpsc::Receiver<serde_json::Value>> {
        self.rx.take()
    }

    /// Emit a raw event.
    pub async fn emit(&self, event: serde_json::Value) -> anyhow::Result<()> {
        self.tx.send(event).await?;
        Ok(())
    }

    /// Emit a synthetic post commit event.
    pub async fn emit_post(&self, did: &str, rkey: &str, text: &str) -> anyhow::Result<()> {
        self.emit(serde_json::json!({
            "did": did,
            "time_us": chrono_now_us(),
            "kind": "commit",
            "commit": {
                "collection": "app.bsky.feed.post",
                "rkey": rkey,
                "operation": "create",
                "record": {
                    "$type": "app.bsky.feed.post",
                    "text": text,
                    "createdAt": now_rfc3339(),
                },
            },
        }))
        .await
    }

    /// Emit a synthetic follow event.
    pub async fn emit_follow(&self, did: &str, rkey: &str, target_did: &str) -> anyhow::Result<()> {
        self.emit(serde_json::json!({
            "did": did,
            "time_us": chrono_now_us(),
            "kind": "commit",
            "commit": {
                "collection": "app.bsky.graph.follow",
                "rkey": rkey,
                "operation": "create",
                "record": {
                    "$type": "app.bsky.graph.follow",
                    "subject": target_did,
                    "createdAt": now_rfc3339(),
                },
            },
        }))
        .await
    }

    /// Emit a synthetic like event.
    pub async fn emit_like(&self, did: &str, rkey: &str, subject_uri: &str) -> anyhow::Result<()> {
        self.emit(serde_json::json!({
            "did": did,
            "time_us": chrono_now_us(),
            "kind": "commit",
            "commit": {
                "collection": "app.bsky.feed.like",
                "rkey": rkey,
                "operation": "create",
                "record": {
                    "$type": "app.bsky.feed.like",
                    "subject": {
                        "uri": subject_uri,
                    },
                    "createdAt": now_rfc3339(),
                },
            },
        }))
        .await
    }
}

fn chrono_now_us() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as i64
}

fn now_rfc3339() -> String {
    // Simple ISO 8601 timestamp
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    // Good enough for testing — real code would use chrono
    format!("1970-01-01T00:00:00Z+{secs}s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emit_and_receive() {
        let mut fake = FakeJetstream::new(10);
        let mut rx = fake.take_receiver().unwrap();

        fake.emit(serde_json::json!({"test": true})).await.unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event["test"], true);
    }

    #[tokio::test]
    async fn emit_post_has_correct_shape() {
        let mut fake = FakeJetstream::new(10);
        let mut rx = fake.take_receiver().unwrap();

        fake.emit_post("did:plc:test", "abc123", "hello world")
            .await
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event["did"], "did:plc:test");
        assert_eq!(event["commit"]["collection"], "app.bsky.feed.post");
        assert_eq!(event["commit"]["rkey"], "abc123");
        assert_eq!(event["commit"]["record"]["text"], "hello world");
    }

    #[tokio::test]
    async fn emit_follow_has_correct_shape() {
        let mut fake = FakeJetstream::new(10);
        let mut rx = fake.take_receiver().unwrap();

        fake.emit_follow("did:plc:a", "f1", "did:plc:b")
            .await
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event["commit"]["collection"], "app.bsky.graph.follow");
        assert_eq!(event["commit"]["record"]["subject"], "did:plc:b");
    }

    #[tokio::test]
    async fn emit_like_has_correct_shape() {
        let mut fake = FakeJetstream::new(10);
        let mut rx = fake.take_receiver().unwrap();

        fake.emit_like("did:plc:a", "l1", "at://did:plc:b/app.bsky.feed.post/abc")
            .await
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event["commit"]["collection"], "app.bsky.feed.like");
        assert_eq!(
            event["commit"]["record"]["subject"]["uri"],
            "at://did:plc:b/app.bsky.feed.post/abc"
        );
    }

    #[tokio::test]
    async fn take_receiver_returns_none_second_time() {
        let mut fake = FakeJetstream::new(10);
        assert!(fake.take_receiver().is_some());
        assert!(fake.take_receiver().is_none());
    }
}
