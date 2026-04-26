# atrg-xrpc

**XRPC route registration helpers and AT Protocol error types for Rust.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

- **`xrpc_router()`** — a pre-configured Axum router with a fallback that returns spec-compliant AT Protocol error envelopes for unmatched methods.
- **`XrpcError`** — an error type implementing `IntoResponse` that serializes to the AT Protocol error envelope (`{"error": "...", "message": "..."}`), with automatic HTTP status code mapping.
- **`XrpcErrorName`** — enum of standard XRPC error codes: `InvalidRequest`, `AuthRequired`, `Forbidden`, `NotFound`, `RateLimitExceeded`, `InternalServerError`, `MethodNotImplemented`.
- **Convenience constructors** — `xrpc_invalid_request()`, `xrpc_auth_required()`, `xrpc_forbidden()`, `xrpc_not_found()`, `xrpc_rate_limit()` for one-liner error returns.

Every `/xrpc/*` response — success or failure — conforms to the AT Protocol specification. No accidental plain-text 500s.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
atrg-xrpc = { path = "../crates/atrg-xrpc" }  # or version from crates.io
atrg-core = { path = "../crates/atrg-core" }
axum = "0.8"
serde_json = "1"
```

Define XRPC procedures as normal Axum handlers, mount them on an `xrpc_router()`, and merge into your app:

```rust
use axum::{routing::{get, post}, Json, extract::State};
use atrg_core::AppState;
use atrg_xrpc::{xrpc_router, XrpcError, xrpc_not_found, xrpc_invalid_request};
use serde_json::{json, Value};

async fn get_posts(
    State(_state): State<AppState>,
) -> Result<Json<Value>, XrpcError> {
    Ok(Json(json!({ "posts": [] })))
}

async fn create_post(
    State(_state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, XrpcError> {
    let text = body["text"]
        .as_str()
        .ok_or_else(|| xrpc_invalid_request("missing field: text"))?;

    if text.is_empty() {
        return Err(xrpc_invalid_request("text must not be empty"));
    }

    // ... create record ...
    Ok(Json(json!({ "uri": "at://did:plc:example/com.example.post/abc123" })))
}

pub fn routes() -> axum::Router<AppState> {
    xrpc_router()
        .route("/xrpc/com.example.getPosts", get(get_posts))
        .route("/xrpc/com.example.createPost", post(create_post))
}
```

Unmatched `/xrpc/*` paths automatically return:

```json
{
  "error": "MethodNotImplemented",
  "message": "XRPC method not implemented"
}
```

## Error mapping

| `XrpcErrorName`          | HTTP Status |
|--------------------------|-------------|
| `InvalidRequest`         | 400         |
| `AuthRequired`           | 401         |
| `Forbidden`              | 403         |
| `NotFound`               | 404         |
| `RateLimitExceeded`      | 429         |
| `InternalServerError`    | 500         |
| `MethodNotImplemented`   | 501         |

## Related crates

| Crate | Role |
|-------|------|
| `atrg-core` | App builder, config, shared state |
| `atrg-auth` | OAuth login, session extractors |
| `atrg-stream` | Jetstream event consumer |

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).