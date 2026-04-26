# Jetstream Event Streaming

Jetstream provides real-time AT Protocol events (posts, follows, likes, etc.) via WebSocket.

## Setup

Enable Jetstream in `atrg.toml`:

```toml
[jetstream]
host = "jetstream1.us-east.bsky.network"
collections = ["app.bsky.feed.post"]
channel_capacity = 1024
max_lag_events = 10000
```

Register a handler:

```rust
AtrgApp::new()
    .on_event(|event: JetstreamEvent, state| Box::pin(async move {
        if let Some(commit) = &event.commit {
            if commit.collection == "app.bsky.feed.post" {
                if let Some(record) = &commit.record {
                    let text = record["text"].as_str().unwrap_or("");
                    sqlx::query("INSERT OR IGNORE INTO posts (did, rkey, text) VALUES (?, ?, ?)")
                        .bind(&event.did)
                        .bind(&commit.rkey)
                        .bind(text)
                        .execute(&state.db)
                        .await?;
                }
            }
        }
        Ok(())
    }))
    .run()
    .await?;
```

## Backpressure

Events flow through a bounded channel. When your handler falls behind:

1. Channel fills up → WebSocket reads pause (natural backpressure)
2. At `max_lag_events` → events are dropped with a warning log
3. Metrics track `events_dropped` for monitoring

## ZSTD Dictionary

For compressed Jetstream streams, provide a dictionary:

```toml
[jetstream]
zstd_dict = "https://example.com/jetstream-dict.bin"  # auto-downloaded + cached
# or
zstd_dict = "/path/to/local/dict.bin"
```

## Ordering

Events are delivered in arrival order per connection. Per-account ordering is preserved by Jetstream. Cross-account ordering is NOT guaranteed.