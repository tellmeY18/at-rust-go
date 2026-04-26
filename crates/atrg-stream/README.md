# atrg-stream

**Bounded, backpressure-aware Jetstream consumer for AT Protocol event streaming.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

- **`spawn_consumer`** — spawns a background `tokio` task that connects to an AT Protocol Jetstream relay and delivers events through a bounded channel
- **`JetstreamEvent`** — re-exported event type from the Jetstream protocol
- **`JetstreamMetrics`** — runtime metrics: events received, events dropped, reconnects, current backoff
- **`StreamConfig`** — standalone configuration (no dependency on `atrg-core`) for relay host, collections, channel capacity, lag thresholds, and optional ZSTD dictionary
- **`EventHandler<S>`** — type alias for async event handler closures that receive an event and a clone of your app state
- **Built-in backpressure** — events flow through a `tokio::sync::mpsc` channel; when the handler falls behind, the consumer pauses WebSocket reads rather than buffering unbounded
- **Lag detection & shedding** — warns via `tracing` when the channel reaches `max_lag_events` and drops the oldest events to keep memory bounded
- **ZSTD dictionary support** — auto-fetches and caches dictionaries from a URL, or loads from a local path
- **Automatic reconnection** with exponential backoff

## Usage

```toml
[dependencies]
atrg-stream = "0.1"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

```rust
use atrg_stream::{spawn_consumer, JetstreamEvent, StreamConfig};
use std::sync::Arc;

#[derive(Clone)]
struct MyState { /* your app state */ }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = StreamConfig {
        host: "jetstream1.us-east.bsky.network".into(),
        collections: vec!["app.bsky.feed.post".into()],
        zstd_dict: None,
        channel_capacity: 1024,
        max_lag_events: 10_000,
    };

    let state = MyState {};

    let handler = Arc::new(|event: JetstreamEvent, _state: MyState| {
        Box::pin(async move {
            println!("got event from {}", event.did);
            Ok(())
        }) as futures::future::BoxFuture<'static, anyhow::Result<()>>
    });

    let _handle = spawn_consumer(&config, state, handler).await?;

    // Consumer runs in the background until the task is aborted
    tokio::signal::ctrl_c().await?;
    Ok(())
}
```

## Design notes

This crate is deliberately **independent of `atrg-core`** to avoid cyclic dependencies. It defines its own `StreamConfig` that `atrg-core` maps from its `JetstreamConfig` before calling `spawn_consumer`. Events are delivered in arrival order per consumer connection.

When used via the `atrg-core` `AtrgApp` builder, all of this wiring is automatic — just add `[jetstream]` to your `atrg.toml` and register a handler with `.on_event()`.

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).