//! OAuth and session HTTP routes.
//!
//! These routes are mounted automatically by `AtrgApp::run()`:
//!
//! - `GET /auth/login?handle=<handle>` — initiate OAuth
//! - `GET /auth/callback` — OAuth callback
//! - `POST /auth/logout` — clear session
//! - `GET /auth/session` — current session info (JSON)
//! - `GET /client-metadata.json` — OAuth client metadata
//! - `GET /.well-known/oauth-protected-resource` — OAuth protected resource metadata

use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use atrg_core::error::AtrgError;
use atrg_core::state::AppState;

use crate::extractor::RequireAuth;
use crate::session;

/// Build the auth router with all authentication routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
        .route("/auth/session", get(get_session))
}

/// Returns a single router containing all atrg built-in auth routes:
/// `/auth/*`, `/client-metadata.json`, and `/.well-known/oauth-protected-resource`.
///
/// This is the recommended way to wire auth into [`atrg_core::AtrgApp`]:
///
/// ```rust,no_run
/// use atrg_core::AtrgApp;
///
/// AtrgApp::new()
///     .with_auth_routes(atrg_auth::routes::auth_router())
///     .with_cleanup_task(atrg_auth::routes::spawn_cleanup_task)
///     .run();
/// ```
pub fn auth_router() -> Router<AppState> {
    routes()
        .route("/client-metadata.json", get(client_metadata))
        .route("/.well-known/oauth-protected-resource", get(well_known))
}

/// OAuth client metadata endpoint.
///
/// Returns the JSON document required by the AT Protocol OAuth spec.
pub async fn client_metadata(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = &state.config.auth;
    Json(serde_json::json!({
        "client_id": config.client_id,
        "client_name": state.config.app.name,
        "client_uri": format!("http://{}:{}", state.config.app.host, state.config.app.port),
        "redirect_uris": [config.redirect_uri],
        "scope": config.scope,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "application_type": "web",
        "token_endpoint_auth_method": "none",
        "dpop_bound_access_tokens": true,
    }))
}

/// OAuth protected resource metadata endpoint.
pub async fn well_known(State(state): State<AppState>) -> Json<serde_json::Value> {
    let base_url = format!("http://{}:{}", state.config.app.host, state.config.app.port);
    Json(serde_json::json!({
        "resource": base_url,
        "authorization_servers": [],
        "scopes_supported": [state.config.auth.scope],
        "bearer_methods_supported": ["header"],
    }))
}

/// Login query parameters.
#[derive(serde::Deserialize)]
pub struct LoginQuery {
    /// The user's AT Protocol handle.
    handle: Option<String>,
}

/// `GET /auth/login?handle=<handle>`
///
/// In a full implementation, this initiates the OAuth PKCE flow with the
/// user's PDS. For now, this is a stub that validates the handle parameter
/// and returns an error explaining OAuth is not yet wired to a real PDS.
async fn login(
    State(_state): State<AppState>,
    Query(params): Query<LoginQuery>,
) -> Result<Response, AtrgError> {
    let handle = params
        .handle
        .filter(|h| !h.trim().is_empty())
        .ok_or_else(|| AtrgError::BadRequest("missing 'handle' query parameter".to_string()))?;

    tracing::info!(handle = %handle, "OAuth login initiated");

    // TODO(phase2-full): Wire up atproto-oauth-axum for real OAuth flow.
    // For now, return a JSON response explaining the flow.
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "oauth_not_yet_wired",
            "message": "OAuth PKCE flow will be wired via atproto-oauth-axum in the next iteration. For now, use the session injection API for testing.",
            "handle": handle,
        })),
    )
        .into_response())
}

/// `GET /auth/callback`
///
/// OAuth callback handler. Stub for now.
async fn callback(State(_state): State<AppState>) -> Result<Response, AtrgError> {
    // TODO(phase2-full): Process OAuth callback, exchange code for tokens,
    // create session, set cookie, redirect.
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "callback_stub",
            "message": "OAuth callback will be implemented with atproto-oauth-axum.",
        })),
    )
        .into_response())
}

/// `POST /auth/logout`
///
/// Clears the session cookie and deletes the session from the database.
async fn logout(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, AtrgError> {
    // Try to find session ID from cookie or bearer
    let session_id = extract_session_id(&headers);

    if let Some(sid) = session_id {
        session::delete_session(&state.db, sid)
            .await
            .map_err(AtrgError::Internal)?;
        tracing::info!("session deleted via logout");
    }

    // Build response with cookie clearing
    let mut response = StatusCode::NO_CONTENT.into_response();

    let is_secure = state.config.app.environment != "development";
    let cookie_value = format!(
        "atrg_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax{}",
        if is_secure { "; Secure" } else { "" }
    );

    if let Ok(val) = HeaderValue::from_str(&cookie_value) {
        response.headers_mut().insert("set-cookie", val);
    }

    Ok(response)
}

/// `GET /auth/session`
///
/// Returns the current session info or 401.
async fn get_session(RequireAuth(user): RequireAuth) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "did": user.did,
        "handle": user.handle,
        "expires_at": user.expires_at,
    }))
}

/// Extract session ID from Authorization header or cookie.
fn extract_session_id(headers: &axum::http::HeaderMap) -> Option<&str> {
    // Try bearer token
    if let Some(auth) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(s) = auth.to_str() {
            if let Some(token) = s.strip_prefix("Bearer ") {
                return Some(token.trim());
            }
        }
    }

    // Try cookie
    if let Some(cookie) = headers.get(axum::http::header::COOKIE) {
        if let Ok(cookies) = cookie.to_str() {
            return crate::extractor::extract_cookie_value(cookies, "atrg_session");
        }
    }

    None
}

/// Spawn a periodic cleanup task for expired OAuth states and sessions.
pub fn spawn_cleanup_task(pool: atrg_db::DbPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(600)); // every 10 min
        loop {
            interval.tick().await;
            if let Err(e) = session::cleanup_expired_sessions(&pool).await {
                tracing::warn!(error = %e, "session cleanup failed");
            }
            if let Err(e) = session::cleanup_expired_oauth_states(&pool).await {
                tracing::warn!(error = %e, "oauth state cleanup failed");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use atrg_core::config::{AppConfig, AuthConfig, Config, DatabaseConfig};
    use axum::body::Body;
    use http_body_util::BodyExt;
    use hyper::Request;
    use tower::ServiceExt;

    async fn test_state() -> AppState {
        let db = atrg_db::connect("sqlite::memory:").await.unwrap();
        atrg_db::run_internal_migrations(&db).await.unwrap();
        AppState {
            config: Arc::new(Config {
                app: AppConfig {
                    name: "test".into(),
                    host: "127.0.0.1".into(),
                    port: 3000,
                    secret_key: "a]3)FRd9-x4bQ7Y!kN2mW#pL8v$Tz0cS".into(),
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
                firehose: None,
                feed_generator: None,
                labeler: None,
                rate_limit: None,
            }),
            db,
            http: reqwest::Client::new(),
            identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
                reqwest::Client::new(),
            )),
        }
    }

    fn test_router(state: AppState) -> Router {
        Router::new()
            .merge(routes())
            .route("/client-metadata.json", get(client_metadata))
            .route("/.well-known/oauth-protected-resource", get(well_known))
            .with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn client_metadata_has_required_fields() {
        let state = test_state().await;
        let app = test_router(state);
        let resp = app
            .oneshot(
                Request::get("/client-metadata.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert!(body["client_id"].is_string());
        assert!(body["redirect_uris"].is_array());
        assert!(body["scope"].is_string());
        assert!(body["application_type"].is_string());
        assert!(body["grant_types"].is_array());
        assert!(body["response_types"].is_array());
        assert!(body["dpop_bound_access_tokens"].is_boolean());
    }

    #[tokio::test]
    async fn well_known_returns_json() {
        let state = test_state().await;
        let app = test_router(state);
        let resp = app
            .oneshot(
                Request::get("/.well-known/oauth-protected-resource")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert!(body["resource"].is_string());
        assert!(body["scopes_supported"].is_array());
    }

    #[tokio::test]
    async fn login_without_handle_returns_400() {
        let state = test_state().await;
        let app = test_router(state);
        let resp = app
            .oneshot(Request::get("/auth/login").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn session_without_auth_returns_401() {
        let state = test_state().await;
        let app = test_router(state);
        let resp = app
            .oneshot(Request::get("/auth/session").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn session_with_valid_session_returns_200() {
        let state = test_state().await;
        let sid = session::generate_session_id();
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 86400;
        session::create_session(
            &state.db,
            &sid,
            "did:plc:test",
            "alice.test",
            "tok",
            None,
            expires,
        )
        .await
        .unwrap();

        let app = test_router(state);
        let resp = app
            .oneshot(
                Request::get("/auth/session")
                    .header("cookie", format!("atrg_session={sid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert_eq!(body["did"], "did:plc:test");
        assert_eq!(body["handle"], "alice.test");
    }

    #[tokio::test]
    async fn session_with_bearer_token_returns_200() {
        let state = test_state().await;
        let sid = session::generate_session_id();
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 86400;
        session::create_session(
            &state.db,
            &sid,
            "did:plc:bearer",
            "bob.test",
            "tok",
            None,
            expires,
        )
        .await
        .unwrap();

        let app = test_router(state);
        let resp = app
            .oneshot(
                Request::get("/auth/session")
                    .header("authorization", format!("Bearer {sid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert_eq!(body["did"], "did:plc:bearer");
    }

    #[tokio::test]
    async fn logout_clears_session() {
        let state = test_state().await;
        let sid = session::generate_session_id();
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 86400;
        session::create_session(
            &state.db,
            &sid,
            "did:plc:logout",
            "logout.test",
            "tok",
            None,
            expires,
        )
        .await
        .unwrap();

        let app = test_router(state.clone());
        let resp = app
            .oneshot(
                Request::post("/auth/logout")
                    .header("cookie", format!("atrg_session={sid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 204);
        assert!(resp.headers().get("set-cookie").is_some());

        // Session should be gone
        let s = session::find_session(&state.db, &sid).await.unwrap();
        assert!(s.is_none());
    }
}
