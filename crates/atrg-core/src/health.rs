//! Built-in health and readiness endpoints.
//!
//! - `GET /healthz` — always returns 200 `{"ok": true}`
//! - `GET /readyz` — returns 200 if DB is reachable, 503 otherwise

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use crate::state::AppState;

/// `GET /healthz` — liveness probe, always 200.
pub async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

/// `GET /readyz` — readiness probe, checks DB connectivity.
pub async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.ping().await {
        Ok(()) => {
            let metrics = state.identity.metrics();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "database": "connected",
                    "database_backend": state.db.backend(),
                    "identity_cache": {
                        "hits": metrics.hits,
                        "misses": metrics.misses,
                        "entries": metrics.entry_count,
                    },
                })),
            )
        }
        Err(e) => {
            tracing::error!(error = %e, "readiness check failed: database unreachable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "ok": false,
                    "database": "unreachable",
                    "error": "database connectivity check failed",
                })),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, AuthConfig, Config, DatabaseConfig};
    use axum::body::Body;
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use hyper::Request;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn test_state() -> AppState {
        let db = atrg_db::connect("sqlite::memory:").await.unwrap();
        atrg_db::run_internal_migrations(&db).await.unwrap();

        let config = Config {
            app: AppConfig {
                name: "test-app".into(),
                host: "127.0.0.1".into(),
                port: 3000,
                secret_key: "test-secret-key-for-health-tests".into(),
                cors_origins: vec![],
                environment: "development".into(),
                admin_dids: vec![],
            },
            auth: AuthConfig {
                client_id: "http://localhost:3000/client-metadata.json".into(),
                redirect_uri: "http://localhost:3000/auth/callback".into(),
                scope: "atproto transition:generic".into(),
                post_login_redirect: "/".into(),
            },
            database: DatabaseConfig {
                url: "sqlite::memory:".into(),
            },
            jetstream: None,
            firehose: None,
            feed_generator: None,
            labeler: None,
            rate_limit: None,
        };

        AppState {
            config: Arc::new(config),
            db,
            http: reqwest::Client::new(),
            identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
                reqwest::Client::new(),
            )),
            extensions: Arc::new(crate::state::Extensions::new()),
        }
    }

    #[tokio::test]
    async fn healthz_returns_200_ok() {
        let app: Router = Router::new().route("/healthz", get(healthz));

        let req = Request::builder()
            .uri("/healthz")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["ok"], true);
    }

    #[tokio::test]
    async fn readyz_returns_200_when_db_connected() {
        let state = test_state().await;
        let app: Router = Router::new()
            .route("/readyz", get(readyz))
            .with_state(state);

        let req = Request::builder()
            .uri("/readyz")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["ok"], true);
        assert_eq!(body["database"], "connected");
        assert!(body["identity_cache"].is_object());
    }
}
