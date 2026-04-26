# atrg-label

**Labeler framework for at-rust-go: create, sign, store, and stream AT Protocol labels.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

- **`LabelService`** — high-level service for creating, signing, negating, and querying labels backed by SQLite
- **`Label`** / **`SignedLabel`** — typed representations of AT Protocol labels (`com.atproto.label.defs#label`) with cryptographic signatures
- **`LabelValue`** — strongly-typed label values (e.g. `!hide`, `!warn`, `porn`, `nudity`, or custom strings)
- **`LabelSigner`** — signs labels using the labeler's private key so consumers can verify authenticity
- **`LabelerConfig`** — configuration block parsed from `atrg.toml` under `[labeler]`
- **XRPC route helpers** — pre-built Axum routes for `com.atproto.label.queryLabels` and label subscription endpoints
- **Automatic migrations** — the labels table is created on first use via `LabelService::migrate()`

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
atrg-label = { version = "0.1" }
```

### Creating a label service and issuing labels

```rust
use atrg_label::{LabelService, LabelValue};
use atrg_label::signing::LabelSigner;
use sqlx::SqlitePool;

async fn example(db: SqlitePool) -> anyhow::Result<()> {
    let signer = LabelSigner::from_pem("path/to/private-key.pem")?;
    let service = LabelService::new(db, signer, "did:web:labeler.example.com".into());

    // Run migrations (creates the labels table)
    service.migrate().await?;

    // Label a piece of content
    let signed = service
        .create_label(
            "at://did:plc:abc123/app.bsky.feed.post/tid",
            LabelValue::Warn,
            None,
        )
        .await?;

    println!("Created label: {} -> {}", signed.label.uri, signed.label.val);

    // Negate a previously issued label
    service
        .negate_label(
            "at://did:plc:abc123/app.bsky.feed.post/tid",
            LabelValue::Warn,
        )
        .await?;

    // Query labels for a subject
    let labels = service
        .query_labels("at://did:plc:abc123/app.bsky.feed.post/tid", None)
        .await?;

    for label in labels {
        println!("{}: neg={}", label.label.val, label.label.neg);
    }

    Ok(())
}
```

### Mounting label routes in your app

```rust
use std::sync::Arc;
use atrg_core::AtrgApp;
use atrg_label::routes::labeler_routes;
use atrg_label::{LabelService, signing::LabelSigner};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build your LabelService (db, signer, and DID configured elsewhere)
    let signer = LabelSigner::from_pem("path/to/private-key.pem")?;
    let service = Arc::new(LabelService::new(db, signer, "did:web:labeler.example.com".into()));

    AtrgApp::new()
        .mount(labeler_routes(service))
        .run()
        .await
}
```

This serves the `com.atproto.label.queryLabels` XRPC endpoint automatically.

### Configuration

Add a `[labeler]` section to your `atrg.toml`:

```toml
[labeler]
did = "did:web:labeler.example.com"
signing_key = "path/to/private-key.pem"
```

## How it works

1. `LabelService::create_label` builds a `Label` struct per the `com.atproto.label` lexicon
2. The label is signed with the labeler's private key via `LabelSigner`
3. The `SignedLabel` is persisted to SQLite through `LabelStore`
4. Consumers query labels via the mounted XRPC endpoint or directly through the service

All labels conform to the AT Protocol label specification — including version, source DID, subject URI, optional CID, value, negation flag, creation timestamp, and optional expiry.

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).