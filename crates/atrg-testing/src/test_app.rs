//! Test application builder — in-memory everything, no network.

use atrg_auth::session;
use atrg_core::config::{AppConfig, AuthConfig, Config, DatabaseConfig};
use atrg_core::state::AppState;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Build a test [`AppState`] backed by in-memory SQLite.
///
/// Migrations are applied automatically. No outbound network calls are made.
pub async fn test_state() -> AppState {
    let db = atrg_db::connect("sqlite::memory:").await.expect("test db");
    atrg_db::run_internal_migrations(&db)
        .await
        .expect("migrations");

    let config = Config {
        app: AppConfig {
            name: "test-app".into(),
            host: "127.0.0.1".into(),
            port: 3000,
            secret_key: "test-secret-key-at-least-32-chars!!".into(),
            cors_origins: vec![],
            environment: "development".into(),
        },
        auth: AuthConfig {
            client_id: "http://localhost:3000/client-metadata.json".into(),
            redirect_uri: "http://localhost:3000/auth/callback".into(),
            scope: "atproto transition:generic".into(),
        },
        database: DatabaseConfig {
            url: "sqlite::memory:".into(),
        },
        jetstream: None,
    };

    AppState {
        config: Arc::new(config),
        db,
        http: reqwest::Client::new(),
        identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
            reqwest::Client::new(),
        )),
    }
}

/// Build a test [`AppState`] and an [`AtrgApp`] ready for `oneshot` testing.
pub async fn test_app() -> (atrg_core::AtrgApp, AppState) {
    let state = test_state().await;
    let app = atrg_core::AtrgApp::new();
    (app, state)
}

/// Seed a session into the test database for a given user.
///
/// Returns the session ID that can be used in `Authorization: Bearer <id>`
/// or `Cookie: atrg_session=<id>` headers.
pub async fn seed_session(pool: &SqlitePool, did: &str, handle: &str) -> String {
    let session_id = session::generate_session_id();
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 86400; // 24 hours

    session::create_session(
        pool,
        &session_id,
        did,
        handle,
        "test_access_token",
        Some("test_refresh_token"),
        expires,
    )
    .await
    .expect("seed session");

    session_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_state_works() {
        let state = test_state().await;
        assert_eq!(state.config.app.name, "test-app");
    }

    #[tokio::test]
    async fn test_app_works() {
        let (_app, state) = test_app().await;
        assert_eq!(state.config.app.name, "test-app");
    }

    #[tokio::test]
    async fn seed_and_find_session() {
        let state = test_state().await;
        let sid = seed_session(&state.db, "did:plc:test", "test.user").await;
        assert!(!sid.is_empty());

        let found = session::find_session(&state.db, &sid).await.unwrap();
        assert!(found.is_some());
        let s = found.unwrap();
        assert_eq!(s.did, "did:plc:test");
        assert_eq!(s.handle, "test.user");
    }
}
