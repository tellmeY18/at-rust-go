# atrg-firehose

**AT Protocol firehose consumer for at-rust-go — subscribes to `com.atproto.sync.subscribeRepos` with bounded backpressure.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

<!-- Uncomment when published:
[![crates.io](https://img.shields.io/crates/v/atrg-firehose.svg)](https://crates.io/crates/atrg-firehose)
[![docs.rs](https://docs.rs/atrg-firehose/badge.svg)](https://docs.rs/atrg-firehose)
-->

## What this crate provides

- **`spawn_firehose`** — spawns a background Tokio task that connects to a relay via WebSocket, decodes CAR-encoded repo events, and delivers them through a bounded channel with backpressure
- **`FirehoseEvent`** — top-level event enum covering commits, handles, tombstones, and other relay messages
- **`FirehoseCommit`** — decoded commit payload containing repo DID, revision, and a list of `RepoOp`s
- **`RepoOp`** / **`OpAction`** — individual record operations (create, update, delete) with collection, rkey, and decoded record data
- **`FirehoseConfig`** — configuration struct (relay URL, resume cursor, channel capacity)
- **`FirehoseHandler<S>`** — type alias for async event handler functions, generic over application state
- **`FirehoseMetrics`** — runtime metrics (events received, errors, reconnects)
- **Built-in backoff** — automatic reconnection with exponential backoff on connection failures
- **CAR decoding** — internal module for decoding the CAR (Content Addressable aRchive) blocks from repo commits

This crate has zero dependency on `atrg-core`, so it can be used standalone.

## Usage

```toml
[dependencies]
atrg-firehose = "0.1"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

```rust
use std::sync::Arc;
use atrg_firehose::{spawn_firehose, FirehoseConfig, FirehoseEvent};

#[derive(Clone)]
struct MyState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = FirehoseConfig {
        relay: "wss://bsky.network".to_string(),
        cursor: None,          // start from head; or Some(seq) to resume
        channel_capacity: 1024,
    };

    let handler = Arc::new(|event: FirehoseEvent, _state: MyState| {
        Box::pin(async move {
            if let FirehoseEvent::Commit(commit) = &event {
                for op in &commit.ops {
                    println!("{} {} {}", commit.repo, op.action, op.path);
                }
            }
            Ok(())
        }) as futures::future::BoxFuture<'static, anyhow::Result<()>>
    });

    let _handle = spawn_firehose(config, MyState, handler).await?;

    // The consumer runs in the background until the JoinHandle is dropped.
    tokio::signal::ctrl_c().await?;
    Ok(())
}
```

## Configuration defaults

| Field              | Default              | Description                        |
|--------------------|----------------------|------------------------------------|
| `relay`            | `wss://bsky.network` | Relay WebSocket URL                |
| `cursor`           | `None`               | Resume cursor (sequence number)    |
| `channel_capacity` | `1024`               | Bounded backpressure channel size  |

## How it differs from atrg-stream

`atrg-stream` consumes **Jetstream** (a filtered, JSON-based event stream).
`atrg-firehose` consumes the **raw relay firehose** (`subscribeRepos`), which delivers every repo commit across the entire network as CAR-encoded blocks. Use the firehose when you need full-network coverage; use Jetstream when you need filtered, lower-bandwidth events.

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).