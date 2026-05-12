# atrg Migration Guide

This document provides detailed migration instructions for upgrading between major versions of at-rust-go (atrg). It is designed to be consumed by both humans and AI agents (LLMs) assisting with code migration.

For the machine-readable API surface, see also: llms.txt and llms-full.txt

---

## Migrating from v0.1.x to v0.2.0

Release date: 2026-05-12

### Summary of breaking changes

1. AppState has a new required field: `extensions`
2. `run_user_migrations` is deprecated; replaced by `run_isolated_migrations`
3. `AppConfig` has a new field: `admin_dids`
4. `StreamConfig` has a new field: `cursor`
5. `AuthSource` enum has a new variant: `ApiKey`
6. Internal migrations now use `_atrg_migrations` tracking table instead of `_sqlx_migrations`

### Change 1: AppState gains `extensions` field

BEFORE (v0.1.x):
```rust
use atrg_core::AppState;
use std::sync::Arc;

let state = AppState {
    config: Arc::new(config),
    db: pool,
    http: reqwest::Client::new(),
    identity: Arc::new(resolver),
};
```

AFTER (v0.2.0):
```rust
use atrg_core::{AppState, Extensions};
use std::sync::Arc;

let state = AppState {
    config: Arc::new(config),
    db: pool,
    http: reqwest::Client::new(),
    identity: Arc::new(resolver),
    extensions: Arc::new(Extensions::new()),  // ADD THIS LINE
};
```

If you construct `AppState` directly (typically only in tests — production code uses `AtrgApp::run()` which handles this automatically), add `extensions: Arc::new(Extensions::new())` to the struct literal.

Search pattern to find affected code:
```
grep -rn "AppState {" --include="*.rs" | grep -v "extensions"
```

The `Extensions` type is re-exported from `atrg_core::Extensions`.

To store app-specific state (replacing `once_cell` or other global state patterns):
```rust
// In main.rs / startup:
struct MyAppState { db: PgPool, blobs: S3Client }

AtrgApp::new()
    .with_extension(MyAppState { db, blobs })
    .mount(routes())
    .run()
    .await

// In handlers:
async fn my_handler(State(state): State<AppState>) -> impl IntoResponse {
    let app = state.extension::<MyAppState>();
    // use app.db, app.blobs, etc.
}
```

### Change 2: run_user_migrations deprecated

BEFORE (v0.1.x):
```rust
atrg_db::run_user_migrations(&pool, Path::new("./migrations")).await?;
```

AFTER (v0.2.0):
```rust
atrg_db::run_isolated_migrations(&pool, Path::new("./migrations"), "_app_migrations").await?;
```

The third argument is the tracking table name. Framework migrations now use `_atrg_migrations` and app migrations should use a different name (e.g. `_app_migrations`) to avoid conflicts.

If you have multiple binaries sharing a database (e.g. write server + aggregator), use different tracking table names:
```rust
// In write server:
atrg_db::run_isolated_migrations(&pool, Path::new("./server_migrations"), "_server_migrations").await?;

// In aggregator:
atrg_db::run_isolated_migrations(&pool, Path::new("./aggregator_migrations"), "_aggregator_migrations").await?;
```

The old `run_user_migrations` still works but emits a deprecation warning and will be removed in v0.3.0.

NOTE: If you relied on the default `_sqlx_migrations` tracking table for your app's migrations, they will NOT be automatically migrated to the new table name. The old migrations remain applied in the database — the new tracking table starts empty and will re-apply migrations. If your migrations are idempotent (use `CREATE TABLE IF NOT EXISTS`, `INSERT ... ON CONFLICT DO NOTHING`, etc.), this is safe. If not, you need to manually copy rows from `_sqlx_migrations` to your new tracking table before upgrading:
```sql
INSERT INTO _app_migrations (version, description, checksum, applied_at)
SELECT version, description, checksum, installed_on FROM _sqlx_migrations
WHERE version NOT IN (SELECT version FROM _app_migrations);
```

### Change 3: AppConfig gains `admin_dids` field

BEFORE (v0.1.x):
```rust
AppConfig {
    name: "my-app".into(),
    host: "127.0.0.1".into(),
    port: 3000,
    secret_key: "...".into(),
    cors_origins: vec![],
    environment: "development".into(),
}
```

AFTER (v0.2.0):
```rust
AppConfig {
    name: "my-app".into(),
    host: "127.0.0.1".into(),
    port: 3000,
    secret_key: "...".into(),
    cors_origins: vec![],
    environment: "development".into(),
    admin_dids: vec![],  // ADD THIS LINE
}
```

This field defaults to empty via `#[serde(default)]` when loading from atrg.toml, so no config file changes are needed. Only affects code that constructs `AppConfig` struct literals directly (typically test code).

Search pattern:
```
grep -rn "AppConfig {" --include="*.rs" | grep -v "admin_dids"
```

To use admin bootstrapping, add to atrg.toml:
```toml
[app]
admin_dids = ["did:plc:your-admin-did"]
```
Or set the environment variable:
```
ATRG_APP__ADMIN_DIDS=did:plc:abc123,did:plc:def456
```

### Change 4: StreamConfig gains `cursor` field

BEFORE (v0.1.x):
```rust
let stream_config = atrg_stream::StreamConfig {
    host: "jetstream1.us-east.bsky.network".into(),
    collections: vec!["app.bsky.feed.post".into()],
    zstd_dict: None,
    channel_capacity: 1024,
    max_lag_events: 10_000,
};
```

AFTER (v0.2.0):
```rust
let stream_config = atrg_stream::StreamConfig {
    host: "jetstream1.us-east.bsky.network".into(),
    collections: vec!["app.bsky.feed.post".into()],
    zstd_dict: None,
    channel_capacity: 1024,
    max_lag_events: 10_000,
    cursor: None,  // ADD THIS LINE — None means "start from live"
};
```

Only affects code that constructs `StreamConfig` directly. If you use `AtrgApp::on_event()` (the typical path), this is handled automatically.

Cursor options:
- `None` or `Some("live".into())` — start from the current time (v0.1 behavior, default)
- `Some("auto".into())` — resume from the last stored cursor position
- `Some("1234567890".into())` — start from a specific timestamp in microseconds

### Change 5: AuthSource enum gains ApiKey variant

BEFORE (v0.1.x):
```rust
match session.source {
    AuthSource::Atrg => { /* atrg session cookie/bearer */ }
    AuthSource::AtprotoJwt => { /* PDS-issued JWT */ }
}
```

AFTER (v0.2.0):
```rust
match session.source {
    AuthSource::Atrg => { /* atrg session cookie/bearer */ }
    AuthSource::AtprotoJwt => { /* PDS-issued JWT */ }
    AuthSource::ApiKey => { /* API key (programmatic access) */ }  // ADD THIS ARM
}
```

If you match exhaustively on `AuthSource`, add the `ApiKey` variant. If you use a wildcard (`_`) catch-all, no change needed.

API keys are bearer tokens with a configurable prefix (default: `atrg_`). The `RequireAuth` extractor handles them transparently — no handler changes needed unless you inspect `session.source`.

### Change 6: Internal migration tracking table

The framework's internal migrations (atrg_sessions, atrg_oauth_states) previously used the default `_sqlx_migrations` table. They now use `_atrg_migrations`.

Impact: On first startup after upgrading, the framework will re-run its internal migrations against the new tracking table. Since all internal migrations use `CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT EXISTS`, this is safe and idempotent.

The old `_sqlx_migrations` table will remain in your database but is no longer used. You can safely drop it after verifying the upgrade:
```sql
DROP TABLE IF EXISTS _sqlx_migrations;
```

### New features available after upgrading (no migration required)

These are additive features. No code changes needed to use the existing v0.1 functionality — but you can opt in to these new capabilities:

#### API Key Authentication
```rust
// Create an API key
let (full_key, api_key) = atrg_auth::api_keys::create_api_key(
    &pool, "did:plc:user", "My Key", &["admin:*".into()], "atrg_"
).await?;

// Clients authenticate with: Authorization: Bearer atrg_xxxxxxxxxxxx
// RequireAuth extractor handles it automatically — no handler changes needed.
```

#### RBAC (Role-Based Access Control)
```rust
// Grant a role
atrg_auth::rbac::grant_role(&pool, "did:plc:user", "admin", None, None, None).await?;

// Check a role
let is_admin = atrg_auth::rbac::has_role(&pool, "did:plc:user", "admin", None).await?;

// Ban a DID (with optional TTL)
atrg_auth::rbac::ban_did(&pool, "did:plc:bad", Some("spam"), Some(86400), "did:plc:admin").await?;

// NOTE: RBAC tables (atrg_roles, atrg_bans) are NOT auto-created by the framework.
// You must create them yourself using the DDL constants:
//   atrg_auth::rbac::CREATE_ROLES_TABLE_SQLITE (or _POSTGRES)
//   atrg_auth::rbac::CREATE_BANS_TABLE_SQLITE (or _POSTGRES)
```

#### Blob Storage
```toml
# Add to Cargo.toml:
atrg-blob = "0.2"
```
```rust
use atrg_blob::{FileBlobStore, BlobStore};

let store = FileBlobStore::new("./blobs").await?;
let cid = store.put(b"hello world").await?;
let data = store.get(&cid).await?;
```

#### Event Router
```rust
use atrg_stream::EventRouterBuilder;

let router = EventRouterBuilder::new()
    .on_create("app.bsky.feed.post", |event, state| Box::pin(async move {
        println!("new post from {}: {}", event.did, event.rkey);
        Ok(())
    }))
    .build();

AtrgApp::new()
    .on_event(router)
    .run()
    .await
```

#### Cross-Origin Auth (SPA support)
```toml
# atrg.toml
[auth]
post_login_redirect = "https://my-spa.example.com/login"
```
After OAuth, the user is redirected to:
```
https://my-spa.example.com/login?token=SESSION_ID&did=did:plc:xxx&handle=user.bsky.social
```
The SPA stores the token and sends it as `Authorization: Bearer SESSION_ID`.

#### App-Specific Config Sections
```toml
# atrg.toml
[myapp]
database_url = "postgres://..."
admin_email = "admin@example.com"

[myapp.s3]
bucket = "my-blobs"
```
```rust
#[derive(serde::Deserialize)]
struct MyConfig {
    database_url: String,
    admin_email: String,
    s3: S3Config,
}

let config: MyConfig = atrg_core::config::load_app_config("myapp")?;
```

#### Email / OTP
```toml
# Add to Cargo.toml:
atrg-email = "0.2"
```
```rust
// Send OTP (logs to stdout if SMTP not configured)
atrg_email::send_otp(&pool, None, "did:plc:user", "user@example.com").await?;

// Verify OTP
let valid = atrg_email::verify_otp(&pool, "did:plc:user", "user@example.com", "123456").await?;
```

#### Multi-Binary Template
```bash
atrg new my-app --template multi-binary
```
Generates a workspace with:
- `crates/my-app-server/` — write server (OAuth, XRPC, blobs)
- `crates/my-app-aggregator/` — firehose subscriber (feeds, search)
- `crates/my-app-shared/` — shared generated types

### Dependency changes

New workspace crates in v0.2.0:
- `atrg-blob` — content-addressed blob storage (S3 + filesystem)
- `atrg-email` — SMTP email delivery + OTP verification

New external dependencies:
- `rust-s3` (optional, behind `atrg-blob/s3` feature) — S3 client
- `lettre` (in `atrg-email`) — SMTP client
- `async-trait` (in `atrg-blob`) — async trait support
- `sha2` (in `atrg-blob`, `atrg-auth`) — SHA-256 hashing

No external dependencies were removed or had breaking version bumps.

### Complete upgrade checklist

- [ ] Update all `atrg-*` dependencies in Cargo.toml to `"0.2"` (resolves to latest 0.2.x)
- [ ] Add `extensions: Arc::new(Extensions::new())` to any direct `AppState` construction (usually only in tests)
- [ ] Add `admin_dids: vec![]` to any direct `AppConfig` construction (usually only in tests)
- [ ] Add `cursor: None` to any direct `StreamConfig` construction
- [ ] Replace `run_user_migrations` calls with `run_isolated_migrations` (add tracking table name as third arg)
- [ ] Add `AuthSource::ApiKey` arm if you match exhaustively on `AuthSource`
- [ ] Run `cargo build` — fix any remaining compilation errors (the compiler will tell you exactly what's missing)
- [ ] Run your test suite
- [ ] Optionally: `DROP TABLE IF EXISTS _sqlx_migrations;` from your database after verifying the upgrade

### Errata

#### v0.2.0 → v0.2.1

`atrg-email`, `atrg-auth`, `atrg-stream`, and `atrg-core` had non-exhaustive `match pool { }` blocks when only one database feature (`sqlite` or `postgres`) was active while `DbPool` still contained both variants. This caused `error[E0004]` at compile time.

Fixed in v0.2.1 by adding `#[allow(unreachable_patterns)] _ => ...` wildcard arms to all 23 match sites across 6 files. If you hit this on v0.2.0, upgrade to `"0.2.1"` or later.

### Known integration patterns (not framework bugs)

These are patterns discovered during changala.app's migration to atrg 0.2.x. They are expected behaviors, not bugs.

#### Domain-specific RBAC stays in the app

`atrg_auth::rbac` provides generic building blocks: `has_role`, `grant_role`, `revoke_role`, `ban_did`, `lift_ban`. But domain-specific authorization logic that combines role checks with business queries (e.g. "is this user the class rep for *this specific course*" or "is this user enrolled in *this course*") must remain in the application. The framework's RBAC is the foundation — the app composes it.

Pattern:
```rust
// App-level helper that uses framework RBAC as a building block
async fn require_class_rep_or_admin(db: &PgPool, did: &str, course_id: i64) -> Result<(), XrpcError> {
    // Check framework-level admin role first
    if atrg_auth::rbac::has_role(pool, did, "admin", None).await? {
        return Ok(());
    }
    // Fall back to domain-specific check
    let is_rep = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM courses WHERE id = $1 AND class_rep_did = $2"
    ).bind(course_id).bind(did).fetch_one(db).await?;
    if is_rep > 0 { Ok(()) } else { Err(XrpcError::forbidden("not authorized")) }
}
```

#### API key table schema differences

If your app already has an `api_keys` table with a different schema (e.g. TEXT timestamps instead of BIGINT, base64-encoded keys instead of hex), you have two options:

1. **Keep your custom implementation** — the framework's `RequireAuth` extractor detects API keys by prefix pattern (`contains('_')`) and delegates to `atrg_auth::api_keys::find_by_key`. If your table schema differs, keep your own `find_api_key` and middleware.

2. **Migrate your table** — alter columns to match atrg's schema (BIGINT timestamps, hex-encoded keys with `sha256-` prefix hashes), then switch to `atrg_auth::api_keys`.

atrg's API key schema uses:
- `key_hash`: `sha256-{hex}` (SHA-256 of the full key, hex-encoded with prefix)
- `expires_at`, `created_at`, `last_used_at`: Unix epoch BIGINT (not RFC3339 TEXT)
- `scopes`: comma-separated TEXT (not JSON array)
- Key format: `{prefix}{64 hex chars}` (not base64url)
