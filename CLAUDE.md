# CLAUDE.md — at-rust-go (atrg): AT Protocol Backend Framework

## Project Overview

**at-rust-go** (CLI: `atrg`) is a batteries-included, opinionated Rust **backend** framework for building AT Protocol social applications. It is an **API-only / headless** framework — it provides everything you need to stand up the server side of a federated social app on the AT Protocol network, and nothing for the frontend.

The goal is dead simple: a developer runs three commands and has a working, federated AT Protocol API server.

```bash
cargo install atrg-cli
atrg new my-app
cd my-app && atrg dev
```

That's it. A running ATProto API server with OAuth login, Jetstream event streaming, XRPC endpoints, and JSON HTTP routes — in under five minutes. Bring your own frontend (web, mobile, native — atrg doesn't care).

at-rust-go is to AT Protocol backends what Django REST Framework is to web APIs — it makes the right choices for you so you focus on your app's business logic, not the protocol plumbing.

---

## Core Philosophy

1. **Headless / API-only.** atrg is a backend framework. It does **not** ship templating, server-rendered HTML, static asset hosting, JS bundlers, or any frontend concerns. Output is JSON (and the OAuth redirect flow). Bring your own client.
2. **Do not reimplement AT Protocol.** Use `atproto-crates` (v0.14.x) as the protocol layer. atrg is glue and convention, not a protocol implementation.
3. **Convention over configuration.** An `atrg.toml` file drives everything. Sane defaults ship out of the box.
4. **Simple and stupid.** If something can be done with less abstraction, do it with less abstraction. Avoid macros unless they eliminate genuine boilerplate.
5. **Axum is the HTTP layer.** `atproto-crates` already uses Axum. So does atrg. No reinventing the router.
6. **SQLite by default.** Ship with `sqlx` + SQLite. No Postgres required to start. Swap when you need to.
7. **Tokio throughout.** All async is `tokio`. No mixing runtimes.
8. **Latest stable Rust.** Require the current stable toolchain via `rust-toolchain.toml`.
9. **Lexicon-agnostic (pure gateway).** atrg ships zero lexicons. It is a transport-layer framework. Developers bring their own `.json` lexicon definitions and build the data models and XRPC methods on top. There are no feature flags for bundled lexicons (`app.bsky.*`, `com.atproto.*`, or otherwise). This keeps the core lean and leaves the choice of application semantics to the developer. atrg provides the roads and the sign-posts (OAuth, XRPC, Jetstream, session management) — never the buildings or street names.
10. **Schema tooling, not schema bundling.** atrg ships a code-generation pipeline (`atrg generate`) that turns *the developer's own* lexicon JSON files into Rust types, validators, and Axum route stubs. The generator runs at build/dev time inside the user's project — it never embeds any lexicon into atrg's published crates. This gives the productivity of "batteries included" without the lock-in.
11. **Protocol correctness over convenience.** Every error response from an `/xrpc/*` route uses the AT Protocol error envelope. Every OAuth token is refreshed transparently when expired. Every DID resolution is cached. These are non-optional defaults — atrg is the gateway, so getting the protocol right is the framework's job, not the developer's.

---

## Repository Layout

```
at-rust-go/
├── CLAUDE.md                  ← this file
├── ROADMAP.md                 ← v0.1.0 implementation plan
├── Cargo.toml                 ← workspace root
├── rust-toolchain.toml        ← pin to latest stable
├── crates/
│   ├── atrg-core/             ← AppState, config, app builder
│   ├── atrg-auth/             ← OAuth wiring on top of atproto-oauth-axum
│   ├── atrg-stream/           ← Jetstream consumer wiring
│   ├── atrg-db/               ← sqlx migrations, session store, record cache
│   ├── atrg-xrpc/             ← XRPC route registration helpers + `XrpcError`
│   ├── atrg-identity/         ← DID/handle resolution + cache (TTL-backed)
│   ├── atrg-codegen/          ← Lexicon-driven codegen (`atrg generate`)
│   ├── atrg-testing/          ← Test utilities: mock client, fake Jetstream, in-mem PDS
│   └── atrg-cli/              ← `atrg` binary (new, dev, migrate, routes, generate)
└── examples/
    ├── minimal/               ← 50-line hello-world AT Protocol API
    └── social/                ← Full social API scaffold (posts, follows, likes)
```

Note: there is **no** `atrg-ui` crate. atrg ships zero frontend code.

---

## Dependency Stack

These are the **only** external crates atrg layers on top of. Keep this list short.

```toml
# Cargo.toml (workspace)
[workspace.dependencies]

# AT Protocol — DO NOT reimplement anything these provide
atproto-identity   = "0.14.2"
atproto-oauth      = "0.14.2"
atproto-oauth-aip  = "0.14.2"
atproto-oauth-axum = "0.14.2"
atproto-client     = "0.14.2"
atproto-xrpcs      = "0.14.2"
atproto-jetstream  = "0.14.2"
atproto-record     = "0.14.2"
atproto-extras     = "0.14.2"
# Lexicon validation + codegen input (used by developers at build time;
# atrg ships no built-in lexicons, only tooling that consumes the user's lexicons)
atproto-lexicon    = "0.14.2"

# HTTP
axum               = { version = "0.8", features = ["macros"] }
tower              = "0.5"
tower-http         = { version = "0.6", features = ["trace", "cors"] }

# Async
tokio              = { version = "1", features = ["full"] }

# Database
sqlx               = { version = "0.8", features = ["sqlite", "runtime-tokio", "migrate"] }

# Serialization
serde              = { version = "1", features = ["derive"] }
serde_json         = "1"

# Config
toml               = "0.8"

# Errors & Logging
anyhow             = "1"
thiserror          = "1"
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# Caching (DID/handle resolution cache)
moka               = { version = "0.12", features = ["future"] }

# JWT (verifying AT Protocol JWTs from PDSs)
jsonwebtoken       = "9"

# Codegen (atrg-codegen only)
quote              = "1"
proc-macro2        = "1"
prettyplease       = "0.2"
syn                = { version = "2", features = ["full"] }

# CLI (atrg-cli only)
clap               = { version = "4", features = ["derive"] }
```

Note the absence of `minijinja`, `tera`, `askama`, `tower-http`'s `fs` feature, or any frontend tooling. atrg is an API server.

```toml
# rust-toolchain.toml
[toolchain]
channel = "stable"
```

---

## atrg-core — AppState and Builder

This crate is the spine. Everything else attaches to `AppState`.

### `AppState`

```rust
// crates/atrg-core/src/lib.rs
use std::sync::Arc;
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: SqlitePool,
    pub http: reqwest::Client,
    /// DID/handle resolver with TTL-backed in-memory cache.
    pub identity: Arc<atrg_identity::IdentityResolver>,
}
```

`AppState` is an `Arc`-wrapped bundle passed into every Axum handler. Keep it flat — no nested `Arc<Arc<...>>`. There is no template engine field; atrg returns JSON.

### `Config`

Loaded from `atrg.toml` in the project root at startup.

```rust
// crates/atrg-core/src/config.rs
#[derive(Debug, serde::Deserialize)]
pub struct Config {
    pub app: AppConfig,
    pub auth: AuthConfig,
    pub database: DatabaseConfig,
    pub jetstream: Option<JetstreamConfig>,
}

#[derive(Debug, serde::Deserialize)]
pub struct AppConfig {
    pub name: String,
    pub host: String,           // e.g. "myapp.example.com"
    pub port: u16,              // default: 3000
    pub secret_key: String,     // for session signing
    pub cors_origins: Vec<String>, // allowed CORS origins for the API
    pub environment: String,       // "development" | "production"; affects cookie Secure flag and security headers
}

#[derive(Debug, serde::Deserialize)]
pub struct AuthConfig {
    pub client_id: String,      // AT Protocol OAuth client ID
    pub redirect_uri: String,   // must match host
    pub scope: String,          // default: "atproto transition:generic"
}

#[derive(Debug, serde::Deserialize)]
pub struct DatabaseConfig {
    pub url: String,            // default: "sqlite://atrg.db"
}

#[derive(Debug, serde::Deserialize)]
pub struct JetstreamConfig {
    pub host: String,            // e.g. "jetstream1.us-east.bsky.network"
    pub collections: Vec<String>, // e.g. ["app.bsky.feed.post"]
    pub zstd_dict: Option<String>, // path to dictionary file; auto-downloaded + cached if URL
    pub channel_capacity: usize, // bounded backpressure channel size; default 1024
    pub max_lag_events: usize,   // shed events / log warn beyond this; default 10_000
}

#[derive(Debug, serde::Deserialize)]
pub struct IdentityConfig {
    pub cache_capacity: u64,    // max DID documents cached; default 10_000
    pub cache_ttl_secs: u64,    // TTL per entry; default 3600 (1 hour)
    pub plc_directory: String,  // default "https://plc.directory"
}
```

### `AtrgApp` Builder

```rust
// crates/atrg-core/src/app.rs
pub struct AtrgApp {
    router: axum::Router<AppState>,
}

impl AtrgApp {
    pub fn new() -> Self {
        Self {
            router: axum::Router::new(),
        }
    }

    // Mount additional Axum routers (user routes, XRPC routes, etc.)
    pub fn mount(mut self, router: axum::Router<AppState>) -> Self {
        self.router = self.router.merge(router);
        self
    }

    // Register a Jetstream event handler
    pub fn on_event<F, Fut>(self, handler: F) -> Self
    where
        F: Fn(atrg_stream::JetstreamEvent, AppState) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        // stored internally, spawned after .run()
        self
    }

    // Start the server — this is the ONLY async entry point
    pub async fn run(self) -> anyhow::Result<()> {
        // 1. Load config from atrg.toml
        // 2. Connect to DB, run migrations
        // 3. Build AppState
        // 4. Mount atrg-auth OAuth routes
        // 5. Apply CORS layer
        // 6. Spawn Jetstream consumer task if configured
        // 7. axum::serve(listener, router).await
    }
}
```

A minimal app's `main.rs` looks like this:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    atrg_core::AtrgApp::new()
        .mount(my_app::routes())
        .on_event(my_app::handle_event)
        .run()
        .await
}
```

---

## Authentication Model — Two Token Types

atrg distinguishes two distinct credentials, and the `AuthUser` / `RequireAuth` extractors handle both transparently:

1. **atrg session token** — issued by atrg after a successful OAuth login. Stored in an `HttpOnly` cookie (browser clients) or returned to SPAs in the post-login redirect fragment. Used for atrg's own JSON API (`/api/*`, `/auth/session`).
2. **AT Protocol JWT** — issued by the user's PDS, used for `/xrpc/*` calls per the AT Protocol spec. Verified using the `iss` (PDS DID) and `sub` (user DID) claims, with the signing key resolved via `atproto-identity`.

The extractor logic:

- `Authorization: Bearer <token>` — parse JWT structure first; if it parses as a JWT and `iss`/`sub` resolve, treat as AT-Proto JWT. Otherwise treat as an atrg session token.
- Cookie `atrg_session=<id>` — always treated as an atrg session token.
- Both paths populate the same `AtrgSession` shape so handlers don't care which credential the client used.

Token refresh: when atrg uses the stored access token to call out via `atproto-client` and gets a `401`, it transparently exchanges the refresh token, updates the `atrg_sessions` row, and retries the request once. Developers never write refresh code.

---

## atrg-auth — OAuth on Top of `atproto-oauth-axum`

Do **not** reimplement OAuth. `atproto-oauth-axum` already has production-ready handlers. atrg's job is to wire them in automatically and expose a simple session extractor.

### What atrg provides

- Mounts `/auth/login`, `/auth/callback`, `/auth/logout` routes automatically in `AtrgApp::run()`. These are the only HTML-emitting endpoints atrg provides, because OAuth requires an HTTP redirect dance — they emit minimal `text/plain` or HTTP redirects, not styled pages. Frontends call `/auth/login?handle=...` and follow redirects.
- Exposes a JSON endpoint `GET /auth/session` returning the currently authenticated user (or `401`).
- Stores OAuth state + sessions in the `atrg-db` SQLite session table.
- Provides an Axum extractor `AuthUser` that reads the session cookie or `Authorization: Bearer <atrg_session_token>` header.

```rust
// crates/atrg-auth/src/extractor.rs

/// Axum extractor — use in any handler to get the logged-in user.
/// Returns None if not logged in (don't reject — let the handler decide
/// whether to return 401 or proceed anonymously).
pub struct AuthUser(pub Option<AtrgSession>);

/// Strict extractor — rejects with 401 if not logged in.
pub struct RequireAuth(pub AtrgSession);

pub struct AtrgSession {
    pub did: String,
    pub handle: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    /// "atrg" if issued via OAuth-then-session-cookie, "atproto_jwt" if request used a PDS-issued JWT.
    pub source: AuthSource,
}

pub enum AuthSource {
    Atrg,        // atrg session token (cookie or bearer)
    AtprotoJwt,  // PDS-issued JWT in Authorization header
}

#[axum::async_trait]
impl<S> axum::extract::FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = std::convert::Infallible;
    // reads session cookie OR bearer token → queries atrg_sessions table → returns AtrgSession
}
```

### Session table schema (auto-migrated)

```sql
CREATE TABLE IF NOT EXISTS atrg_sessions (
    id            TEXT PRIMARY KEY,
    did           TEXT NOT NULL,
    handle        TEXT NOT NULL,
    access_token  TEXT NOT NULL,
    refresh_token TEXT,
    expires_at    INTEGER NOT NULL,
    created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    last_used_at  INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_atrg_sessions_did ON atrg_sessions(did);
CREATE INDEX IF NOT EXISTS idx_atrg_sessions_expires_at ON atrg_sessions(expires_at);
```

### Usage in a handler

```rust
async fn me(
    State(_state): State<AppState>,
    RequireAuth(user): RequireAuth,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "did": user.did,
        "handle": user.handle,
    }))
}
```

---

## atrg-identity — DID & Handle Resolution Cache

Wraps `atproto-identity::resolve_subject()` with a `moka` TTL-backed in-memory cache. Every handler that needs to resolve a DID document or handle should go through `state.identity` rather than calling `atproto-identity` directly.

```rust
// crates/atrg-identity/src/lib.rs
pub struct IdentityResolver {
    cache: moka::future::Cache<String, ResolvedIdentity>,
    http: reqwest::Client,
    plc_directory: String,
}

impl IdentityResolver {
    pub async fn resolve(&self, subject: &str) -> anyhow::Result<ResolvedIdentity>;
    pub async fn invalidate(&self, subject: &str);
    pub fn metrics(&self) -> IdentityMetrics; // hits, misses, evictions
}
```

Configuration comes from `[identity]` in `atrg.toml`. Defaults: 10_000 entries, 1-hour TTL, `https://plc.directory`. The cache is populated on first resolve and used by every JWT verification, every Jetstream event handler that needs handle lookups, and every XRPC procedure that resolves the caller's DID.

---

## atrg-stream — Jetstream Consumer

Wraps `atproto-jetstream` with a simple callback-based interface. Spawned as a background `tokio::task` inside `AtrgApp::run()` when `[jetstream]` is present in `atrg.toml`.

### Production hardening (built in, not opt-in)

- **Bounded backpressure**: events flow through a `tokio::sync::mpsc` channel of size `channel_capacity`. When the user handler falls behind, the consumer pauses reads from the WebSocket rather than buffering unbounded.
- **Lag detection**: if the channel reaches `max_lag_events`, atrg logs a `tracing::warn!` and increments a `jetstream_lagged_total` counter. Beyond that threshold, the oldest event is dropped (with a counter increment) so memory stays bounded.
- **Ordering guarantee**: events are delivered in arrival order per consumer connection. Per-account ordering is preserved by Jetstream itself; cross-account ordering is *not* guaranteed and this is documented.
- **ZSTD dictionary**: if `zstd_dict` is a URL, the dictionary is fetched once at startup, cached on disk under `~/.cache/atrg/jetstream-dict-<hash>.bin`, and reused across restarts. If it's a local path, it's loaded directly. Dictionary refresh is a manual operation (delete the cache file).
- **Reconnection metrics**: `JetstreamMetrics { events_received, events_dropped, errors, reconnects, last_event_at, current_backoff_ms }` exposed via `atrg_stream::metrics()` and surfaced through `/readyz`.
- **Cursor persistence (optional, future)**: out of scope for v0.1.0 but documented as the upgrade path.

### What atrg provides

- Automatic reconnection (delegate to `atproto-jetstream`'s reconnection logic).
- Passes events to the `on_event` handler registered via `AtrgApp::on_event`.
- Provides a typed `JetstreamEvent` re-export so users don't need to import `atproto-jetstream` directly.

```rust
// crates/atrg-stream/src/lib.rs
pub use atproto_jetstream::event::JetstreamEvent;

pub async fn spawn_consumer(
    config: &JetstreamConfig,
    state: AppState,
    handler: impl Fn(JetstreamEvent, AppState) -> BoxFuture<'static, anyhow::Result<()>>
        + Send + Sync + 'static,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    // build atproto-jetstream consumer from config
    // tokio::spawn the read loop
    // call handler for each event
}
```

### Example event handler

```rust
async fn handle_event(event: JetstreamEvent, state: AppState) -> anyhow::Result<()> {
    if let Some(commit) = event.commit {
        if commit.collection == "app.bsky.feed.post" {
            sqlx::query!(
                "INSERT OR IGNORE INTO posts (did, rkey, text, created_at) VALUES (?, ?, ?, ?)",
                event.did,
                commit.rkey,
                commit.record["text"].as_str().unwrap_or(""),
                commit.record["createdAt"].as_str().unwrap_or(""),
            )
            .execute(&state.db)
            .await?;
        }
    }
    Ok(())
}
```

---

## atrg-db — Database Layer

Thin wrapper around `sqlx` with SQLite. Provides:

- A `migrate!()` call on startup that runs `migrations/` in order.
- atrg's own internal migrations (sessions, etc.) embedded in the crate.
- A `DbConn` type alias: `pub type DbConn = sqlx::SqlitePool;`

atrg does **not** provide an ORM. Write SQL. `sqlx::query!()` macros give you compile-time checked queries.

### Migration convention

```
my-app/
└── migrations/
    ├── 0001_create_posts.sql
    └── 0002_create_follows.sql
```

atrg auto-discovers migrations in the `migrations/` directory. atrg's own internal migrations run first (prefixed with `atrg_`).

---

## atrg-xrpc — XRPC Route Helpers

Thin convenience layer on top of `atproto-xrpcs`. The underlying crate already provides JWT extractors and DID resolution middleware — atrg just makes registration ergonomic.

```rust
// crates/atrg-xrpc/src/lib.rs

/// Returns a Router pre-configured with atproto-xrpcs middleware.
/// Mount this under /xrpc in your app.
pub fn xrpc_router<S: Clone + Send + Sync + 'static>() -> axum::Router<S> {
    axum::Router::new()
        // atproto-xrpcs JWT authorization extractor is available
        // in any handler added to this router via `.route()`
        .fallback(xrpc_method_not_implemented)
}

/// AT Protocol XRPC error envelope. Use this for every `/xrpc/*` failure.
/// Maps to the right HTTP status code automatically.
pub struct XrpcError {
    pub name: XrpcErrorName,         // InvalidRequest, AuthRequired, Forbidden, NotFound, RateLimitExceeded, InternalServerError, MethodNotImplemented
    pub message: String,
}

impl IntoResponse for XrpcError { /* status + Json({"error": name, "message": msg}) */ }

pub fn xrpc_invalid_request(msg: impl Into<String>) -> XrpcError;
pub fn xrpc_auth_required(msg: impl Into<String>) -> XrpcError;
pub fn xrpc_forbidden(msg: impl Into<String>) -> XrpcError;
pub fn xrpc_not_found(msg: impl Into<String>) -> XrpcError;
```

Users add their XRPC procedures like any Axum route:

```rust
// In user code
pub fn routes() -> Router<AppState> {
    atrg_xrpc::xrpc_router()
        .route("/xrpc/com.example.getPosts", get(get_posts))
        .route("/xrpc/com.example.createPost", post(create_post))
}
```

---

## JSON API Conventions

atrg is an API-first framework. All non-OAuth-redirect routes return JSON.

- **Success**: `200 OK` with a JSON body. Use `axum::Json(T)`.
- **App errors** (non-XRPC): status code + JSON body of shape `{ "error": "code", "message": "human readable" }`.
- **XRPC errors** (`/xrpc/*` routes): always conform to the AT Protocol error envelope `{ "error": "InvalidRequest", "message": "..." }` with status codes per spec (400/401/403/404/500). Atrg's global fallback handler intercepts plain Axum errors on `/xrpc/*` paths and reshapes them into this envelope so users cannot accidentally leak Axum's default 500 plain-text response.
- **Pagination**: cursor-based — accept `?cursor=<opaque>&limit=<n>` and return `{ "items": [...], "cursor": "<next>" }`.
- **CORS**: configured per `atrg.toml` `[app].cors_origins`. By default, only same-origin requests are allowed. Preflight `OPTIONS` requests are handled correctly for both `/api/*` and `/auth/*` so that browser-based SPAs can complete the OAuth flow without manual workarounds.
- **Security headers**: in non-development mode, atrg adds `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: strict-origin-when-cross-origin`, and a conservative `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'` on JSON responses. Users can override per-route.
- **Content-Type**: always `application/json; charset=utf-8` for API responses.

There is no HTML rendering, no template engine, no static file serving. If you need a frontend, build it as a separate project that consumes this API.

---

## atrg-cli — The `atrg` Binary

The CLI is the developer's primary interface. Keep commands minimal.

```
atrg new <name>     Scaffold a new at-rust-go API project
atrg dev            Start dev server with file watching (cargo-watch)
atrg migrate        Run pending database migrations
atrg routes         Print all registered routes
atrg build          cargo build --release
```

### `atrg new <name>` scaffold output

```
my-app/
├── Cargo.toml           (workspace, pulls in atrg crates)
├── rust-toolchain.toml  (stable)
├── atrg.toml            (app config with sensible defaults)
├── src/
│   ├── main.rs          (5 lines — just calls AtrgApp)
│   └── routes.rs        (example JSON handlers)
└── migrations/
    └── .gitkeep
```

There is no `templates/` or `static/` directory. atrg projects are pure backend.

### Scaffolded `atrg.toml`

```toml
[app]
name = "my-app"
host = "localhost"
port = 3000
secret_key = "CHANGE_ME_IN_PRODUCTION"
cors_origins = ["http://localhost:5173"]  # e.g. a Vite dev server

[auth]
client_id = "http://localhost:3000/client-metadata.json"
redirect_uri = "http://localhost:3000/auth/callback"
scope = "atproto transition:generic"

[database]
url = "sqlite://atrg.db"

# Uncomment to enable Jetstream
# [jetstream]
# host = "jetstream1.us-east.bsky.network"
# collections = ["app.bsky.feed.post"]
```

### Scaffolded `src/main.rs`

```rust
use atrg_core::AtrgApp;

mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    AtrgApp::new()
        .mount(routes::api())
        .run()
        .await
}
```

### Scaffolded `src/routes.rs`

```rust
use axum::{Router, routing::get, Json, extract::State};
use atrg_auth::AuthUser;
use atrg_core::AppState;
use serde_json::json;

pub fn api() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/api/me", get(me))
}

async fn index(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "name": "my-app", "status": "ok" }))
}

async fn me(State(_state): State<AppState>, AuthUser(user): AuthUser) -> Json<serde_json::Value> {
    match user {
        Some(u) => Json(json!({ "did": u.did, "handle": u.handle })),
        None    => Json(json!({ "authenticated": false })),
    }
}
```

---

## What `AtrgApp::run()` Does (Implementation Order)

This is the single most important function. Keep it linear and readable.

```rust
pub async fn run(self) -> anyhow::Result<()> {
    // 1. Init tracing
    tracing_subscriber::fmt::init();

    // 2. Load atrg.toml
    let config: Config = toml::from_str(&std::fs::read_to_string("atrg.toml")?)?;
    let config = Arc::new(config);

    // 3. Connect SQLite and run migrations
    let db = SqlitePool::connect(&config.database.url).await?;
    sqlx::migrate!("./migrations").run(&db).await?;
    atrg_db::internal_migrations().run(&db).await?;

    // 4. Build reqwest client (shared across crates)
    let http = reqwest::Client::builder()
        .user_agent("atrg/0.1")
        .build()?;

    // 5. Assemble AppState
    let state = AppState { config: config.clone(), db: db.clone(), http };

    // 6. Build CORS layer from config
    let cors = build_cors_layer(&config.app.cors_origins);

    // 7. Build Axum router
    let router = axum::Router::new()
        // atrg built-ins (OAuth + well-known)
        .merge(atrg_auth::routes(state.clone()))          // /auth/*
        .route("/client-metadata.json", get(atrg_auth::client_metadata))
        .route("/.well-known/oauth-protected-resource", get(atrg_auth::well_known))
        // User routes
        .merge(self.router)
        .with_state(state.clone())
        .layer(cors)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    // 8. Spawn Jetstream consumer if configured
    if let Some(js_config) = &config.jetstream {
        if let Some(handler) = self.event_handler {
            atrg_stream::spawn_consumer(js_config, state.clone(), handler).await?;
        }
    }

    // 9. Serve
    let addr = format!("{}:{}", config.app.host, config.app.port);
    tracing::info!("at-rust-go API serving on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
```

---

## AT Protocol Client Usage

When a handler needs to call the AT Protocol network on behalf of a logged-in user, use `atproto-client` directly. atrg does not wrap this — users import it.

```toml
# User's Cargo.toml
atproto-client = "0.14.2"
```

```rust
use atproto_client::Client;
use atrg_auth::RequireAuth;

async fn create_post(
    State(state): State<AppState>,
    RequireAuth(user): RequireAuth,
    Json(body): Json<CreatePostRequest>,
) -> Result<Json<serde_json::Value>, AtrgError> {
    let client = Client::new_with_bearer(&state.http, &user.access_token);
    let record = serde_json::json!({
        "$type": "app.bsky.feed.post",
        "text": body.text,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });

    let result = client.put_record("app.bsky.feed.post", &record).await?;
    Ok(Json(result))
}
```

---

## Error Handling Convention

Every handler returns `Result<impl IntoResponse, AtrgError>`. Errors serialize to JSON.

```rust
// crates/atrg-core/src/error.rs
#[derive(Debug)]
pub enum AtrgError {
    Database(sqlx::Error),
    Auth(String),
    NotFound,
    BadRequest(String),
    Internal(anyhow::Error),
}

impl IntoResponse for AtrgError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match self {
            Self::NotFound       => (StatusCode::NOT_FOUND, "not_found", "Not found".to_string()),
            Self::Auth(m)        => (StatusCode::UNAUTHORIZED, "unauthorized", m),
            Self::BadRequest(m)  => (StatusCode::BAD_REQUEST, "bad_request", m),
            Self::Database(_)    => (StatusCode::INTERNAL_SERVER_ERROR, "database_error", "Database error".to_string()),
            Self::Internal(_)    => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Internal server error".to_string()),
        };
        (status, Json(serde_json::json!({ "error": code, "message": message }))).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AtrgError {
    fn from(e: E) -> Self { Self::Internal(e.into()) }
}
```

---

## Implementation Roadmap (high level)

See `ROADMAP.md` for the detailed v0.1.0 plan with micro-checklists and end-to-end test gates per milestone. The high-level phases are:

### Phase 1 — Skeleton (`atrg new` + `atrg dev`)
- Workspace, `atrg-cli new`, `atrg-core` config + `AppState`, `atrg-db` migrations, server boots and serves a JSON `200`.

### Phase 2 — Auth (OAuth login / logout / session)
- `atrg-auth` wires `atproto-oauth-axum` handlers, `AuthUser` + `RequireAuth` extractors, `/auth/session` JSON endpoint, OAuth metadata endpoints.

### Phase 3 — Jetstream (real-time events)
- `atrg-stream::spawn_consumer`, `AtrgApp::on_event`, auto-spawn on config presence.

### Phase 4 — XRPC
- `atrg-xrpc::xrpc_router()` factory, scaffold example XRPC route.

### Phase 5 — Social API scaffold (`atrg new my-app --template social`)
- Posts, follows, likes tables; timeline, profile, post-creation **JSON** endpoints; Jetstream pre-wired.

### Phase 6 — Polish
- `atrg routes`, `atrg migrate`, `atrg build`, structured request logging.

---

## What atrg Explicitly Does NOT Do

- **No frontend, period.** No HTML templating engine (no Minijinja / Tera / Askama). No server-rendered pages. No static file serving (`ServeDir` is not used). No JS/CSS bundling. No asset pipeline. atrg returns JSON.
- **No ORM.** Write SQL with `sqlx::query!()`. If you want SeaORM, add it yourself.
- **No WebSocket push to clients.** Jetstream is consumed server-side. Pushing events to your own clients (SSE / WebSockets) is your responsibility — add raw Axum routes for it.
- **No Bluesky-specific lexicons (or any bundled lexicons).** atrg does not ship `app.bsky.*` (or any application) lexicons. There are no feature flags to enable Bluesky's data model. The framework provides transport, auth, streaming, and XRPC plumbing — never business logic. If you want to build a Bluesky client, you write the same code you would for any other AT Protocol app, bringing your own lexicon schemas. A future community-managed crate (e.g. `atrg-bluesky`) could bundle lexicons as an optional convenience, but it will live outside this repository.
- **No PDS implementation.** atrg builds apps *on top of* the network. Running a full PDS is out of scope.
- **No multi-tenant support.** atrg is for single-app deployments. Multi-tenancy is an application concern.

---

## Key `atproto-crates` Reference

| What you need | Crate | Key type / fn |
|---|---|---|
| Resolve a DID or handle | `atproto-identity` | `resolve_subject()` |
| OAuth login flow | `atproto-oauth-axum` | handlers in that crate |
| Call XRPC endpoints | `atproto-client` | `Client` |
| Publish records | `atproto-client` | `Client::put_record()` |
| XRPC service routes | `atproto-xrpcs` | `Authorization` extractor |
| Real-time events | `atproto-jetstream` | consumer builder |
| Parse rich text | `atproto-extras` | `parse_facets()` |
| AT-URIs, TIDs | `atproto-record` | `AtUri`, `Tid` |
| Lexicon validation | `atproto-lexicon` | `DefaultLexiconResolver` |

---

## atrg-codegen — Lexicon-Driven Code Generation

> The framework ships zero lexicons. The *tooling* to consume the developer's lexicons is a first-class part of atrg.

`atrg generate <dir>` walks a directory of `.json` lexicon files (provided by the developer, in any namespace), validates each via `atproto-lexicon`, and emits Rust code at `src/generated/`:

1. **Record types** — strongly-typed `serde`-derived structs for every lexicon `record`, `object`, `query`, and `procedure` definition.
2. **XRPC route stubs** — Axum handler signatures with input/output types matching the lexicon's parameters, body, and output schemas. Each stub returns `Result<Json<Output>, XrpcError>` and is wired into a generated `pub fn xrpc_routes() -> Router<AppState>`.
3. **Validation glue** — request bodies are validated against the lexicon's JSON Schema before the user's handler runs. Validation failures emit `XrpcError::InvalidRequest` automatically.
4. **AT-URI helpers** — `record_collection()`, `record_uri(rkey)`, etc., for each record type.

The generator is `atrg-codegen`, callable both as a library (so `build.rs` integration is possible) and via the `atrg generate` CLI subcommand. The generated code lives in the *user's* project — atrg's published crates contain only the generator, never any specific lexicon's output.

This is the "missing battery": it gives developers the productivity of a typed SDK without atrg ever taking a position on which lexicons are blessed.

---

## atrg-testing — Test Utilities

A separate, dev-dependency crate developers pull in under `[dev-dependencies]`:

- `MockAtprotoClient` — programmable responses for `Client::get_record`, `put_record`, etc. Records each call for assertions.
- `FakeJetstream` — feeds a deterministic stream of `JetstreamEvent`s into the user's `on_event` handler, bypassing the network.
- `InMemoryPds` — minimal stub PDS that handles `/xrpc/com.atproto.server.*` enough to complete an OAuth round-trip in integration tests.
- `test_app()` builder — returns an `AtrgApp` wired against in-memory SQLite, fake identity resolver, and disabled Jetstream so users can `oneshot` their handlers without any network.

Without `atrg-testing`, business-logic tests would have to mock the entire AT Protocol surface themselves. With it, developers can write fast, deterministic tests for every handler they own.

---

## Versioning & Compatibility Policy

- atrg's MSRV tracks current stable Rust minus one minor version.
- atrg pins `atproto-crates` to a `0.14.x` range. Bumping the minor version of any `atproto-*` crate is a deliberate atrg release with migration notes.
- atrg's own SemVer: until 1.0, breaking changes can land in minor bumps but must appear in `CHANGELOG.md` with a migration note.
- Protocol-level evolution (new XRPC parameter shapes, new OAuth grant types) is absorbed inside `atrg-auth` / `atrg-xrpc` *without* breaking user code wherever possible — atrg's whole point is to insulate the developer from protocol churn.
- atrg never breaks an existing `xrpc_router()` signature without a major bump.

---

## Community & Governance

- Issues tracker is the primary venue for feature requests.
- Non-trivial features (anything that touches the public API, adds a crate, or changes auth/XRPC semantics) go through a lightweight `docs/rfcs/NNNN-title.md` proposal before implementation. PRs without an RFC for these categories will be asked to file one.
- Engagement with the AT Protocol developer community (Discord, GitHub) is expected for maintainers, especially around OAuth and Jetstream behavior changes upstream.
- License: LGPL-3.0-only. Contributions are accepted under the same license.

---

## Testing Strategy

- Unit test atrg glue code with `tokio::test`.
- Integration tests spin up an in-memory SQLite pool (`SqlitePool::connect(":memory:")`).
- Use `tower::ServiceExt::oneshot()` to test Axum handlers without binding a port.
- Assert on JSON response bodies, not HTML — atrg has no HTML.
- Do NOT mock `atproto-crates` — they have their own test coverage.

```rust
#[tokio::test]
async fn test_index_returns_json() {
    let state = test_state().await;
    let app = Router::new()
        .route("/", get(index))
        .with_state(state);
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );
}
```

---

## Coding Standards

- `cargo fmt` before every commit.
- `cargo clippy -- -D warnings` must pass.
- No `unwrap()` in library code. Use `?` or return `AtrgError`.
- `unwrap()` in tests is fine.
- All public types get doc comments.
- Keep files under ~300 lines. Split when they grow.
- Prefer `Arc<T>` over `Mutex<T>` for shared read-only state in `AppState`.
- If state needs mutation at runtime, use `tokio::sync::RwLock<T>` inside `Arc`.