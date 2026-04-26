# atrg-core

**The spine of the at-rust-go framework — app builder, configuration, shared state, and error types.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

- **`AtrgApp`** — the main application builder. Chain `.mount()` to add Axum routers, `.on_event()` to register Jetstream handlers, and `.run()` to start the server.
- **`Config`** — strongly-typed configuration loaded from `atrg.toml` at startup (app settings, auth, database, Jetstream, identity).
- **`AppState`** — the shared state bundle (`Arc`-wrapped) passed into every Axum handler. Holds the config, database pool, HTTP client, and identity resolver.
- **`AtrgError` / `AtrgResult`** — framework-wide error type that serializes to JSON automatically (`{ "error": "code", "message": "..." }`).
- **`RateLimiter` / `RateLimitConfig`** — configurable rate limiting for API endpoints.
- **`shutdown_signal()`** — graceful shutdown listener (Ctrl+C / SIGTERM).
- **CORS, security headers, request IDs, health checks, and pagination** — built-in middleware and utilities so you don't wire them yourself.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
atrg-core = "0.1"
```

Minimal application:

```rust
use atrg_core::AtrgApp;
use axum::{Router, routing::get, Json};
use serde_json::json;

fn api() -> Router<atrg_core::AppState> {
    Router::new()
        .route("/", get(|| async { Json(json!({ "status": "ok" })) }))
}

#[tokio::main]
async fn main() -> atrg_core::AtrgResult<()> {
    AtrgApp::new()
        .mount(api())
        .run()
        .await
}
```

This loads `atrg.toml`, connects to SQLite, runs migrations, mounts OAuth routes, applies CORS + security headers, and starts serving — all in one call.

### Error handling in handlers

```rust
use atrg_core::{AppState, AtrgError};
use axum::{extract::State, Json};

async fn get_item(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AtrgError> {
    let row = sqlx::query!("SELECT name FROM items LIMIT 1")
        .fetch_optional(&state.db)
        .await?;

    match row {
        Some(r) => Ok(Json(serde_json::json!({ "name": r.name }))),
        None => Err(AtrgError::NotFound),
    }
}
```

`AtrgError` variants (`NotFound`, `BadRequest`, `Auth`, `Database`, `Internal`) each map to the correct HTTP status code and a JSON error body.

## Modules

| Module | Purpose |
|---|---|
| `app` | `AtrgApp` builder and `run()` implementation |
| `config` | `Config` and sub-structs deserialized from `atrg.toml` |
| `state` | `AppState` — the shared context for all handlers |
| `error` | `AtrgError`, `AtrgResult` |
| `cors` | CORS layer built from `[app].cors_origins` |
| `security` | Security headers middleware (`X-Content-Type-Options`, `X-Frame-Options`, etc.) |
| `health` | `/healthz` and `/readyz` endpoints |
| `pagination` | Cursor-based pagination helpers |
| `request_id` | Per-request ID generation and propagation |
| `rate_limit` | `RateLimiter` and `RateLimitConfig` |
| `shutdown` | `shutdown_signal()` for graceful termination |
| `env_override` | Environment variable overrides for config values |

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).