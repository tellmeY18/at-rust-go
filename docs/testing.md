# Testing with atrg-testing

The `atrg-testing` crate provides utilities for fast, deterministic handler tests.

## Setup

```toml
[dev-dependencies]
atrg-testing = "0.1"
```

## Test State

```rust
use atrg_testing::{test_state, seed_session};

#[tokio::test]
async fn test_my_handler() {
    let state = test_state().await;  // in-memory SQLite, all migrations applied

    // Seed an authenticated session
    let session_id = seed_session(&state.db, "did:plc:test", "test.user").await;

    // Build router and test with oneshot
    let app = Router::new()
        .route("/api/me", get(my_handler))
        .with_state(state);

    let resp = app.oneshot(
        Request::get("/api/me")
            .header("authorization", format!("Bearer {session_id}"))
            .body(Body::empty())
            .unwrap()
    ).await.unwrap();

    assert_eq!(resp.status(), 200);
}
```

## Mock Client

```rust
use atrg_testing::MockAtprotoClient;

let mock = MockAtprotoClient::new();
mock.returns("get_record", "app.bsky.feed.post/abc", json!({"text": "hello"}));

let result = mock.get_record("app.bsky.feed.post", "abc").unwrap();
mock.assert_called("get_record", "app.bsky.feed.post/abc", 1);
```

## Fake Jetstream

```rust
use atrg_testing::FakeJetstream;

let mut fake = FakeJetstream::new(10);
let mut rx = fake.take_receiver().unwrap();
fake.emit_post("did:plc:test", "abc123", "hello world").await.unwrap();
let event = rx.recv().await.unwrap();
```
