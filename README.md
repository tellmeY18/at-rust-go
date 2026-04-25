# at-rust-go (atrg)

> A batteries-included, opinionated Rust **backend** framework for building [AT Protocol](https://atproto.com/) applications.

**atrg is headless / API-only.** It provides everything you need to stand up a federated AT Protocol API server — OAuth, Jetstream event streaming, XRPC endpoints, and JSON HTTP routes. Bring your own frontend (web, mobile, native — atrg doesn't care).

---

## Getting Started

### Install the CLI

```bash
cargo install atrg-cli
```

### Create a new project

```bash
atrg new my-app
cd my-app
```

This scaffolds a complete AT Protocol API project:

```
my-app/
├── Cargo.toml           # Dependencies pre-configured
├── rust-toolchain.toml  # Pinned to stable Rust
├── atrg.toml            # Framework configuration
├── src/
│   ├── main.rs          # 5-line entry point
│   └── routes.rs        # Your JSON API handlers
└── migrations/
    └── .gitkeep         # Add your SQL migrations here
```

### Run the dev server

```bash
atrg dev
```

Your API is now running at `http://localhost:3000/`. Every response is JSON.

```bash
# Health check
curl http://localhost:3000/
# → {"name":"my-app","status":"ok"}

# Readiness probe
curl http://localhost:3000/readyz
# → {"ok":true,"database":"connected",...}
```

---

## Configuration

All configuration lives in `atrg.toml`:

```toml
[app]
name = "my-app"
host = "127.0.0.1"
port = 3000
secret_key = "your-secret-key-at-least-32-chars"
cors_origins = ["http://localhost:5173"]
environment = "development"  # or "production"

[auth]
client_id = "http://localhost:3000/client-metadata.json"
redirect_uri = "http://localhost:3000/auth/callback"
scope = "atproto transition:generic"

[database]
url = "sqlite://atrg.db"

# Uncomment to enable real-time event streaming
# [jetstream]
# host = "jetstream1.us-east.bsky.network"
# collections = ["app.bsky.feed.post"]
```

---

## Features

### OAuth Authentication

atrg handles the full AT Protocol OAuth flow automatically:

```bash
# Initiate login (redirects to user's PDS)
curl http://localhost:3000/auth/login?handle=alice.example.com

# Check session
curl -H "Cookie: atrg_session=..." http://localhost:3000/auth/session
# → {"did":"did:plc:...","handle":"alice.example.com","expires_at":...}

# Logout
curl -X POST -H "Cookie: atrg_session=..." http://localhost:3000/auth/logout
# → 204 No Content
```

Use the `AuthUser` or `RequireAuth` extractors in your handlers:

```rust
use atrg_core::{AppState, AuthUser, RequireAuth};
use axum::{Json, extract::State};

// Optional auth — returns None if not logged in
async fn me(AuthUser(user): AuthUser) -> Json<serde_json::Value> {
    match user {
        Some(u) => Json(serde_json::json!({"did": u.did, "handle": u.handle})),
        None    => Json(serde_json::json!({"authenticated": false})),
    }
}

// Required auth — rejects with 401 if not logged in
async fn create_post(
    RequireAuth(user): RequireAuth,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({"created_by": user.did}))
}
```

atrg transparently accepts both **atrg session tokens** (cookie or bearer) and **AT Protocol JWTs** (issued by the user's PDS). Handlers don't need to know which one the client used.

### Jetstream Event Streaming

Enable `[jetstream]` in `atrg.toml` and register an event handler:

```rust
use atrg_core::AtrgApp;
use atrg_stream::JetstreamEvent;

AtrgApp::new()
    .on_event(|event: JetstreamEvent, state| Box::pin(async move {
        if let Some(commit) = &event.commit {
            tracing::info!(
                did = %event.did,
                collection = %commit.collection,
                "received event"
            );
        }
        Ok(())
    }))
    .run()
    .await?;
```

Production-hardened out of the box:
- **Bounded backpressure** — configurable channel capacity, no unbounded buffers
- **Lag detection** — warns and drops events when the handler falls behind
- **Exponential backoff** — 1s → 60s cap on reconnection attempts
- **ZSTD dictionary** — auto-downloaded and cached for compressed streams
- **Metrics** — `events_received`, `events_dropped`, `errors`, `reconnects`, `queue_depth`

### XRPC Procedures

Register AT Protocol XRPC procedures with proper error envelopes:

```rust
use atrg_xrpc::{xrpc_router, XrpcError, xrpc_invalid_request};
use axum::{routing::get, Json};

let xrpc = xrpc_router()
    .route("/xrpc/com.example.ping", get(ping));

async fn ping() -> Result<Json<serde_json::Value>, XrpcError> {
    Ok(Json(serde_json::json!({"pong": true})))
}
```

Unmatched XRPC methods automatically return the AT Protocol error envelope:
```json
{"error": "MethodNotImplemented", "message": "XRPC method not implemented"}
```

### Lexicon Code Generation

Generate Rust types and route stubs from your own AT Protocol lexicon files:

```bash
# Place your lexicon JSON files in ./lexicons/
atrg generate --input lexicons/ --output src/generated/
```

This produces strongly-typed `serde` structs, Axum handler stubs, and a `xrpc_routes()` function — all from **your** lexicons, not any bundled ones.

### DID/Handle Resolution

Every identity lookup goes through a TTL-backed in-memory cache:

```rust
let identity = state.identity.resolve("did:plc:xyz123").await?;
println!("Handle: {}, PDS: {:?}", identity.handle, identity.pds_endpoint);
```

Configurable capacity (default 10,000 entries) and TTL (default 1 hour).

### Testing

Use `atrg-testing` for fast, deterministic handler tests without network access:

```rust
use atrg_testing::{test_state, seed_session};

#[tokio::test]
async fn test_my_handler() {
    let state = test_state().await;
    let session_id = seed_session(&state.db, "did:plc:test", "test.user").await;

    // Use tower::ServiceExt::oneshot to test handlers
    // ...
}
```

---

## Built-in Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/healthz` | GET | Liveness probe — always 200 |
| `/readyz` | GET | Readiness probe — checks DB, shows cache metrics |
| `/auth/login?handle=...` | GET | Initiate OAuth login |
| `/auth/callback` | GET | OAuth callback |
| `/auth/logout` | POST | Clear session (204) |
| `/auth/session` | GET | Current session info or 401 |
| `/client-metadata.json` | GET | OAuth client metadata |
| `/.well-known/oauth-protected-resource` | GET | OAuth resource metadata |

---

## CLI Commands

| Command | Description |
|---------|-------------|
| `atrg new <name>` | Scaffold a new API project |
| `atrg dev` | Start dev server (with cargo-watch if available) |
| `atrg migrate` | Run pending database migrations |
| `atrg routes` | List registered routes |
| `atrg build` | Release build (`cargo build --release`) |
| `atrg generate` | Generate Rust code from lexicon JSON files |

---

## Architecture

```
atrg (workspace)
├── atrg-core       — AppState, config, app builder, error types, pagination
├── atrg-auth       — OAuth flow, session management, JWT verification, extractors
├── atrg-stream     — Jetstream consumer with backpressure and metrics
├── atrg-db         — SQLite connection pool + migration runner
├── atrg-xrpc       — XRPC router factory + AT Protocol error envelope
├── atrg-identity   — DID/handle resolution with TTL cache
├── atrg-codegen    — Lexicon JSON → Rust code generator
├── atrg-testing    — Mock clients, fake Jetstream, test helpers
└── atrg-cli        — The `atrg` binary
```

---

## What atrg Does NOT Do

- **No frontend.** No HTML, no templates, no static files, no JS bundler. atrg returns JSON.
- **No bundled lexicons.** Bring your own `.json` lexicon files. atrg provides the transport layer.
- **No ORM.** Write SQL with `sqlx::query!()`.
- **No PDS implementation.** atrg builds apps *on top of* the AT Protocol network.

---

## Examples

- **[`examples/minimal/`](examples/minimal/)** — Hello-world AT Protocol API (50 lines)
- **[`examples/social/`](examples/social/)** — Full social API scaffold with timeline, profiles, and Jetstream ingestion

---

## License

MIT OR Apache-2.0