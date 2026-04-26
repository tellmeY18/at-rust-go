# atrg-auth

**OAuth authentication wiring and session management for AT Protocol applications.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

- **OAuth route mounting** — `/auth/login`, `/auth/callback`, and `/auth/logout` endpoints wired automatically into your atrg app.
- **Session management** — SQLite-backed sessions with automatic token refresh when AT Protocol access tokens expire.
- **AT Protocol JWT verification** — validates PDS-issued JWTs using DID-resolved signing keys, so your `/xrpc/*` routes accept standard AT Protocol authorization.
- **`AuthUser` extractor** — an Axum extractor that resolves the current session from a cookie or `Authorization: Bearer` header. Returns `None` for unauthenticated requests, letting your handler decide the response.
- **`RequireAuth` extractor** — strict variant that rejects with `401 Unauthorized` if no valid session exists.
- **Dual credential support** — transparently handles both atrg session tokens (issued after OAuth login) and AT Protocol JWTs (issued by the user's PDS), exposing both through the same `AtrgSession` type.

## Key types

| Type | Description |
|------|-------------|
| `AuthUser` | Axum extractor — wraps `Option<AtrgSession>` |
| `RequireAuth` | Axum extractor — wraps `AtrgSession`, rejects if unauthenticated |
| `AtrgSession` | Session data: `did`, `handle`, `access_token`, `refresh_token`, `expires_at`, `source` |
| `AuthSource` | Enum: `Atrg` (session cookie/token) or `AtprotoJwt` (PDS-issued JWT) |

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
atrg-auth = { version = "0.1", path = "../atrg-auth" }
atrg-core = { version = "0.1", path = "../atrg-core" }
axum = "0.8"
serde_json = "1"
```

Use the extractors in any Axum handler:

```rust
use axum::{Json, extract::State};
use atrg_auth::{AuthUser, RequireAuth};
use atrg_core::AppState;
use serde_json::json;

/// Public endpoint — works for both authenticated and anonymous requests.
async fn whoami(
    State(_state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Json<serde_json::Value> {
    match user {
        Some(u) => Json(json!({ "did": u.did, "handle": u.handle })),
        None    => Json(json!({ "authenticated": false })),
    }
}

/// Protected endpoint — returns 401 if not logged in.
async fn my_profile(
    State(_state): State<AppState>,
    RequireAuth(user): RequireAuth,
) -> Json<serde_json::Value> {
    Json(json!({
        "did": user.did,
        "handle": user.handle,
        "source": format!("{:?}", user.source),
    }))
}
```

OAuth routes (`/auth/login`, `/auth/callback`, `/auth/logout`) and the session endpoint (`GET /auth/session`) are mounted automatically by `AtrgApp::run()` — no manual setup required.

## Configuration

Auth settings live in your project's `atrg.toml`:

```toml
[auth]
client_id = "http://localhost:3000/client-metadata.json"
redirect_uri = "http://localhost:3000/auth/callback"
scope = "atproto transition:generic"
```

## Part of the atrg workspace

This crate is not intended to be used standalone. It integrates with:

- **atrg-core** — provides `AppState` and `Config`
- **atrg-db** — backs the `atrg_sessions` table
- **atrg-identity** — resolves DIDs for JWT verification

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).