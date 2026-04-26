# atrg-feed

**Feed generator framework for AT Protocol applications.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

- **`FeedGenerator`** — builder for registering multiple named feeds and producing an Axum router
- **`FeedHandler`** / **`FeedRequest`** — async handler trait and typed request context for feed logic
- **`FeedSkeleton`** / **`SkeletonItem`** — response types conforming to `app.bsky.feed.getFeedSkeleton`
- **`FeedConfig`** / **`FeedDescription`** — feed metadata (display name, description, avatar)
- **`DescribeFeedGeneratorResponse`** — response type for `app.bsky.feed.describeFeedGenerator`
- Automatic XRPC endpoint registration for `describeFeedGenerator` and `getFeedSkeleton`

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
atrg-feed = { version = "0.1", path = "../crates/atrg-feed" }
atrg-core = { version = "0.1", path = "../crates/atrg-core" }
atrg-xrpc = { version = "0.1", path = "../crates/atrg-xrpc" }
```

### Define a feed handler

```rust
use atrg_feed::{FeedGenerator, FeedRequest, FeedSkeleton, SkeletonItem};
use atrg_core::AppState;
use atrg_xrpc::XrpcError;

async fn chronological(req: FeedRequest, state: AppState) -> Result<FeedSkeleton, XrpcError> {
    // Query your database for post URIs
    let items = vec![
        SkeletonItem { post: "at://did:plc:abc/app.bsky.feed.post/tid1".into() },
        SkeletonItem { post: "at://did:plc:abc/app.bsky.feed.post/tid2".into() },
    ];
    Ok(FeedSkeleton { feed: items, cursor: None })
}
```

### Register feeds and mount the router

```rust
use atrg_feed::FeedGenerator;
use atrg_core::AtrgApp;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let feeds = FeedGenerator::new("did:web:feeds.example.com")
        .feed("chronological", "Latest Posts", None, chronological)
        .feed("popular", "Popular Posts", Some("Trending content"), popular_handler)
        .into_router();

    AtrgApp::new()
        .mount(feeds)
        .run()
        .await
}
```

This automatically serves:

- `GET /xrpc/app.bsky.feed.describeFeedGenerator` — lists all registered feeds
- `GET /xrpc/app.bsky.feed.getFeedSkeleton?feed=at://...` — returns the skeleton for a specific feed

### Feed request context

The `FeedRequest` passed to your handler includes:

```rust
pub struct FeedRequest {
    pub feed: String,              // The AT-URI of the requested feed
    pub cursor: Option<String>,    // Pagination cursor from the client
    pub limit: usize,              // Requested page size (default 50, max 100)
    pub requester_did: Option<String>, // DID of the requesting user, if authenticated
}
```

## How it works

`FeedGenerator` collects feed registrations (ID, metadata, handler function) into a map.
When you call `.into_router()`, it produces an Axum `Router<AppState>` with the two
XRPC endpoints pre-wired. Incoming `getFeedSkeleton` requests are dispatched to the
correct handler based on the `feed` query parameter. Unknown feed URIs return an
`XrpcError::NotFound`.

## Integration with atrg-stream

Feed generators typically pair with Jetstream or firehose consumers that index
records into a local database. Your feed handler then queries that database to
build the skeleton. atrg-feed handles the XRPC serving; your indexer and query
logic are your own.

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).