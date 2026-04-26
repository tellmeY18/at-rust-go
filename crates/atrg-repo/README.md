# atrg-repo

**Record repository helpers for AT Protocol applications.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

- **`Repo`** — high-level client wrapping `com.atproto.repo.*` XRPC calls (create, get, list, update, delete records)
- **`AtUri`** — parsed AT Protocol URI (`at://did/collection/rkey`) with builder and validation
- **`Tid`** — base32-sortable timestamp identifiers for record keys
- **`BlobRef` / `BlobLink`** — typed references to uploaded blobs
- **`StrongRef`** — typed reference to a specific record version (URI + CID)
- **`Record<T>` / `Page<T>`** — generic wrappers for single records and paginated listings
- **`RepoError`** — structured error type for repository operations
- **`upload_blob` / `upload_blob_from_url`** — blob upload helpers

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
atrg-repo = { version = "0.1", path = "../crates/atrg-repo" }
```

### Creating a Repo client and working with records

```rust
use atrg_repo::Repo;

async fn example(http: &reqwest::Client) -> anyhow::Result<()> {
    // Create from explicit params
    let repo = Repo::new(http, "https://pds.example.com", "access-token", "did:plc:abc123");

    // Or from an authenticated session
    // let repo = Repo::from_session(http, &session, "https://pds.example.com");

    // Create a record
    let record = serde_json::json!({
        "text": "Hello from atrg!",
        "createdAt": "2025-01-01T00:00:00Z",
    });
    let strong_ref = repo.create_record("app.bsky.feed.post", &record).await?;
    println!("Created: {} @ {}", strong_ref.uri, strong_ref.cid);

    // Get a record
    let post: serde_json::Value = repo.get_record("app.bsky.feed.post", "rkey123").await?;

    // List records with pagination
    let page = repo.list_records::<serde_json::Value>("app.bsky.feed.post", 50, None).await?;
    for record in &page.records {
        println!("{}: {:?}", record.uri, record.value);
    }

    Ok(())
}
```

### Working with AT-URIs and TIDs

```rust
use atrg_repo::{AtUri, Tid};

// Parse an AT-URI
let uri = AtUri::new("at://did:plc:abc123/app.bsky.feed.post/3k2la7b");
assert_eq!(uri.did(), "did:plc:abc123");
assert_eq!(uri.collection(), Some("app.bsky.feed.post"));

// Generate a TID for a new record key
let tid = Tid::now();
println!("New rkey: {}", tid);
```

### Uploading blobs

```rust
use atrg_repo::upload_blob;

async fn upload(http: &reqwest::Client) -> anyhow::Result<()> {
    let blob_ref = upload_blob(
        http,
        "https://pds.example.com",
        "access-token",
        b"image data here",
        "image/png",
    ).await?;
    println!("Blob ref: {:?}", blob_ref);
    Ok(())
}
```

## Error handling

All fallible operations return `Result<T, RepoError>`. `RepoError` covers network
failures, XRPC error responses, deserialization issues, and invalid AT-URIs.

```rust
use atrg_repo::{Repo, RepoError};

async fn safe_get(repo: &Repo) {
    match repo.get_record::<serde_json::Value>("app.bsky.feed.post", "missing").await {
        Ok(val) => println!("Got: {:?}", val),
        Err(RepoError::NotFound(_)) => println!("Record does not exist"),
        Err(e) => eprintln!("Repo error: {e}"),
    }
}
```

## Crate feature highlights

- Zero lexicons bundled — works with any AT Protocol collection
- Typed generics over `serde::Deserialize` for record values
- Cursor-based pagination via `Page<T>`
- Integrates with `atrg-auth::AtrgSession` for ergonomic `Repo::from_session`

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).