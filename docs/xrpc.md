# XRPC Procedures

XRPC is the AT Protocol's RPC mechanism. atrg provides helpers for registering typed procedures.

## Router Factory

```rust
use atrg_xrpc::{xrpc_router, XrpcError, xrpc_invalid_request};
use axum::{routing::{get, post}, Json};

pub fn routes() -> Router<AppState> {
    xrpc_router()
        .route("/xrpc/com.example.ping", get(ping))
        .route("/xrpc/com.example.createThing", post(create_thing))
}
```

## Error Envelope

All `/xrpc/*` errors use the AT Protocol error format:

```json
{"error": "InvalidRequest", "message": "missing required field 'text'"}
```

Available error types: `InvalidRequest` (400), `AuthRequired` (401), `Forbidden` (403), `NotFound` (404), `RateLimitExceeded` (429), `InternalServerError` (500), `MethodNotImplemented` (501).

Unmatched XRPC methods automatically return 501.

## Using with Auth

```rust
async fn create_thing(
    RequireAuth(user): RequireAuth,
    Json(body): Json<CreateThingInput>,
) -> Result<Json<CreateThingOutput>, XrpcError> {
    // user.did, user.handle available
    Ok(Json(output))
}
```
