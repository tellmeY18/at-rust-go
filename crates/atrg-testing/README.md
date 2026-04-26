# atrg-testing

**Test utilities for at-rust-go: mock clients, fake Jetstream, and in-memory test harness.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

- **`test_state()`** — builds an `AppState` backed by in-memory SQLite with migrations pre-applied and no outbound network calls.
- **`test_app()`** — returns a ready-to-use `(AtrgApp, AppState)` pair for `tower::ServiceExt::oneshot` testing.
- **`seed_session()`** — inserts a fake authenticated session into the test database so handlers that require `AuthUser` / `RequireAuth` work without an OAuth round-trip.
- **`MockAtprotoClient`** — programmable stub for AT Protocol XRPC calls (`get_record`, `put_record`, etc.). Records every call for assertions.
- **`FakeJetstream`** — feeds deterministic `JetstreamEvent`s into your `on_event` handler without touching the network.

## Usage

Add `atrg-testing` as a dev dependency:

```toml
[dev-dependencies]
atrg-testing = { version = "0.1" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

### Testing a JSON handler

```rust
use axum::{routing::get, Json, Router};
use atrg_core::AppState;
use atrg_testing::test_state;
use tower::ServiceExt;

async fn hello() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[tokio::test]
async fn test_hello_handler() {
    let state = test_state().await;

    let app = Router::new()
        .route("/", get(hello))
        .with_state(state);

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}
```

### Testing an authenticated handler

```rust
use atrg_testing::{test_state, seed_session};

#[tokio::test]
async fn test_authenticated_route() {
    let state = test_state().await;

    // Seed a session so RequireAuth succeeds
    let session_id = seed_session(
        &state.db,
        "did:plc:testuser",
        "test.handle",
    )
    .await
    .unwrap();

    // Use session_id as a Bearer token or cookie in your request
}
```

### Using FakeJetstream

```rust
use atrg_testing::FakeJetstream;

#[tokio::test]
async fn test_event_handler() {
    let state = test_state().await;
    let mut fake = FakeJetstream::new();

    fake.push_commit("did:plc:abc", "app.bsky.feed.post", "rkey123", serde_json::json!({
        "text": "hello from test",
        "createdAt": "2025-01-01T00:00:00Z",
    }));

    // Feed events into your handler
    for event in fake.drain() {
        my_app::handle_event(event, state.clone()).await.unwrap();
    }
}
```

## Design philosophy

- **Zero network access.** Every component is backed by in-memory stores and stubs.
- **Deterministic.** No randomness, no real clocks unless you opt in.
- **Fast.** In-memory SQLite + `oneshot` means sub-millisecond test execution.

## Related crates

| Crate | Role |
|---|---|
| `atrg-core` | `AppState` and app builder under test |
| `atrg-auth` | Session types used by `seed_session` |
| `atrg-db` | In-memory SQLite pool and migrations |
| `atrg-stream` | Real Jetstream consumer (replaced by `FakeJetstream` in tests) |

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).