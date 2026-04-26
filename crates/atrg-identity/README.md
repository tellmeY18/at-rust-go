# atrg-identity

**DID and handle resolution with TTL-backed in-memory caching for AT Protocol applications.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

<!-- Uncomment when published:
[![crates.io](https://img.shields.io/crates/v/atrg-identity.svg)](https://crates.io/crates/atrg-identity)
[![docs.rs](https://docs.rs/atrg-identity/badge.svg)](https://docs.rs/atrg-identity)
-->

## What this crate provides

- **`IdentityResolver`** — resolves `did:plc:`, `did:web:`, and handle strings to a unified `ResolvedIdentity`, with results cached in a [`moka`](https://crates.io/crates/moka) TTL-backed in-memory cache.
- **`ResolvedIdentity`** — the resolved payload containing `did`, `handle`, `pds_endpoint`, and the raw DID document.
- **`IdentityConfig`** — configure cache capacity (default 10 000 entries), TTL (default 1 hour), and PLC directory URL.
- **`IdentityMetrics`** — observe cache hits, misses, and current entry count.
- Automatic **dual-key caching** — results are stored under both the DID and the handle so either lookup path is fast.
- Zero dependency on `atrg-core` — usable standalone in any Rust project that needs AT Protocol identity resolution.

## Usage

```toml
[dependencies]
atrg-identity = "0.1"
```

```rust
use atrg_identity::{IdentityConfig, IdentityResolver};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let http = reqwest::Client::new();

    // Use defaults: 10k entries, 1h TTL, https://plc.directory
    let resolver = IdentityResolver::with_defaults(http);

    // Resolve a DID
    let identity = resolver.resolve("did:plc:z72i7hdynmk6r22z27h6tvur").await?;
    println!("{} -> {}", identity.did, identity.handle);

    // Resolve a handle (resolves handle → DID → full document)
    let identity = resolver.resolve("alice.bsky.social").await?;
    println!("PDS: {:?}", identity.pds_endpoint);

    // Check cache performance
    let metrics = resolver.metrics();
    println!("hits={} misses={} entries={}", metrics.hits, metrics.misses, metrics.entry_count);

    // Invalidate a stale entry
    resolver.invalidate("did:plc:z72i7hdynmk6r22z27h6tvur").await;

    Ok(())
}
```

### Custom configuration

```rust
use atrg_identity::{IdentityConfig, IdentityResolver};

let config = IdentityConfig {
    cache_capacity: 50_000,
    cache_ttl_secs: 1800, // 30 minutes
    plc_directory: "https://plc.directory".to_string(),
};

let resolver = IdentityResolver::new(&config, reqwest::Client::new());
```

## How it works

1. On `resolve(subject)`, the cache is checked first.
2. On a miss, the resolver dispatches to DID resolution (PLC directory or `did:web`) or handle resolution (`com.atproto.identity.resolveHandle`).
3. The result is cached under **both** the DID and the handle so subsequent lookups by either key are instant.
4. Entries expire after the configured TTL and are evicted when capacity is exceeded.

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).