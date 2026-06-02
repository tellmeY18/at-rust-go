//! OAuth and session HTTP routes.
//!
//! These routes are mounted automatically by `AtrgApp::run()`:
//!
//! - `GET /auth/login?handle=<handle>` — initiate OAuth PKCE + DPoP flow
//! - `GET /auth/callback` — OAuth callback (exchange code for tokens)
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
use crate::oauth;
use crate::session;

/// Extract the origin (scheme + host + port) from a URL.
/// "https://example.com/client-metadata.json" → "https://example.com"
/// "http://localhost:3000/client-metadata.json" → "http://localhost:3000"
fn origin_of(url: &str) -> String {
    match url.find("://") {
        Some(scheme_end) => {
            let after_scheme = &url[scheme_end + 3..];
            match after_scheme.find('/') {
                Some(path_start) => url[..scheme_end + 3 + path_start].to_string(),
                None => url.to_string(),
            }
        }
        None => url.to_string(),
    }
}

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
    let client_uri = origin_of(&config.client_id);
    Json(serde_json::json!({
        "client_id": config.client_id,
        "client_name": state.config.app.name,
        "client_uri": client_uri,
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
    let base_url = origin_of(&state.config.auth.client_id);
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
    /// URL to redirect the browser to after login (overrides config default).
    redirect_after: Option<String>,
}

/// `GET /auth/login?handle=<handle>`
///
/// Initiates the AT Protocol OAuth PKCE + DPoP flow:
/// 1. Resolves the handle to a DID via the identity resolver
/// 2. Discovers the PDS's OAuth authorization server metadata
/// 3. Generates PKCE code verifier + challenge
/// 4. Generates an ephemeral ES256 DPoP keypair
/// 5. Stores the OAuth state in the database
/// 6. Redirects the browser to the PDS authorization endpoint
async fn login(
    State(state): State<AppState>,
    Query(params): Query<LoginQuery>,
) -> Result<Response, AtrgError> {
    let handle = params
        .handle
        .filter(|h| !h.trim().is_empty())
        .ok_or_else(|| AtrgError::BadRequest("missing 'handle' query parameter".to_string()))?;

    let redirect_after = params
        .redirect_after
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| state.config.auth.post_login_redirect.clone());

    tracing::info!(handle = %handle, "OAuth login initiated");

    // 1. Resolve handle → DID + PDS endpoint
    let identity = state.identity.resolve(&handle).await.map_err(|e| {
        AtrgError::BadRequest(format!("failed to resolve handle '{}': {}", handle, e))
    })?;

    let did = &identity.did;
    let pds_endpoint = identity.pds_endpoint.as_deref().ok_or_else(|| {
        AtrgError::BadRequest(format!(
            "no PDS endpoint found for handle '{}' (DID: {})",
            handle, did
        ))
    })?;

    tracing::debug!(did = %did, pds = %pds_endpoint, "resolved handle");

    // 2. Discover PDS OAuth metadata (protected-resource → authorization-server)
    let metadata = oauth::discover_pds_oauth_metadata(&state.http, pds_endpoint)
        .await
        .map_err(|e| {
            // Surface a clean, user-facing 4xx rather than a bare 500: a
            // discovery failure means we could not reach or parse the PDS's
            // OAuth metadata, which the user can act on (wrong handle, PDS
            // down) and which is not an internal fault of this server.
            tracing::warn!(
                handle = %handle,
                pds = %pds_endpoint,
                error = %e,
                "OAuth metadata discovery failed"
            );
            AtrgError::BadRequest(format!(
                "could not discover OAuth configuration for PDS '{}': {}",
                pds_endpoint, e
            ))
        })?;

    // 3. Generate PKCE code verifier + challenge
    let code_verifier = oauth::generate_code_verifier();
    let code_challenge = oauth::compute_code_challenge(&code_verifier);

    // 4. Generate DPoP keypair
    let dpop = oauth::generate_dpop_keypair().map_err(|e| {
        AtrgError::Internal(anyhow::anyhow!("failed to generate DPoP keypair: {}", e))
    })?;

    // 5. Generate random state + nonce
    let oauth_state_id = session::generate_session_id();
    let nonce = session::generate_session_id();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // 6. Store OAuth state in DB (10-minute expiry)
    let oauth_state = session::OAuthState {
        state: oauth_state_id.clone(),
        pkce_verifier: code_verifier,
        dpop_private_key: dpop.private_key_jwk,
        token_endpoint: metadata.token_endpoint.clone(),
        did: did.clone(),
        handle: handle.clone(),
        nonce: nonce.clone(),
        redirect_after,
        expires_at: now + 600,
    };

    session::save_oauth_state(&state.db, &oauth_state)
        .await
        .map_err(|e| AtrgError::Internal(anyhow::anyhow!("failed to save OAuth state: {}", e)))?;

    // 7. Build authorization URL
    let mut auth_url = url::Url::parse(&metadata.authorization_endpoint).map_err(|e| {
        AtrgError::Internal(anyhow::anyhow!("invalid authorization endpoint URL: {}", e))
    })?;

    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &state.config.auth.client_id)
        .append_pair("redirect_uri", &state.config.auth.redirect_uri)
        .append_pair("scope", &state.config.auth.scope)
        .append_pair("state", &oauth_state_id)
        .append_pair("code_challenge", &code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("login_hint", &handle);

    tracing::info!(
        did = %did,
        handle = %handle,
        authorization_endpoint = %metadata.authorization_endpoint,
        "redirecting to PDS authorization endpoint"
    );

    // 8. Redirect
    Ok(axum::response::Redirect::temporary(auth_url.as_str()).into_response())
}

/// Callback query parameters from the PDS authorization server.
#[derive(serde::Deserialize)]
pub struct CallbackQuery {
    /// The authorization code from the PDS.
    code: Option<String>,
    /// The state parameter (must match what we stored).
    state: Option<String>,
    /// OAuth error code (if the user denied or something went wrong).
    error: Option<String>,
    /// Human-readable error description.
    error_description: Option<String>,
}

/// `GET /auth/callback?code=...&state=...`
///
/// Handles the OAuth callback from the PDS:
/// 1. Validates the state parameter against the database
/// 2. Exchanges the authorization code for tokens via DPoP
/// 3. Verifies the returned DID matches the expected one
/// 4. Creates an atrg session in the database
/// 5. Sets the session cookie and redirects to the frontend
async fn callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackQuery>,
) -> Result<Response, AtrgError> {
    // Check for OAuth errors first
    if let Some(error) = &params.error {
        let description = params
            .error_description
            .as_deref()
            .unwrap_or("unknown error");
        tracing::warn!(error = %error, description = %description, "OAuth callback received error");
        return Err(AtrgError::Auth(format!(
            "OAuth authorization failed: {} — {}",
            error, description
        )));
    }

    let code = params
        .code
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| AtrgError::BadRequest("missing 'code' parameter in callback".to_string()))?;

    let state_param = params
        .state
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            AtrgError::BadRequest("missing 'state' parameter in callback".to_string())
        })?;

    // 1. Look up the OAuth state from DB
    let oauth_state = session::find_oauth_state(&state.db, &state_param)
        .await
        .map_err(|e| AtrgError::Internal(anyhow::anyhow!("failed to look up OAuth state: {}", e)))?
        .ok_or_else(|| {
            AtrgError::BadRequest(
                "invalid or expired OAuth state — the login may have timed out, please try again"
                    .to_string(),
            )
        })?;

    // 2. Delete the state immediately (one-time use)
    session::delete_oauth_state(&state.db, &state_param)
        .await
        .map_err(|e| AtrgError::Internal(anyhow::anyhow!("failed to delete OAuth state: {}", e)))?;

    tracing::debug!(
        did = %oauth_state.did,
        handle = %oauth_state.handle,
        "OAuth callback processing"
    );

    // 3. Exchange the authorization code for tokens
    let token_response = oauth::exchange_code_for_tokens(
        &state.http,
        &oauth_state.token_endpoint,
        &code,
        &oauth_state.pkce_verifier,
        &state.config.auth.redirect_uri,
        &state.config.auth.client_id,
        &oauth_state.dpop_private_key,
    )
    .await
    .map_err(|e| AtrgError::Internal(anyhow::anyhow!("token exchange failed: {}", e)))?;

    // 4. Verify the returned DID matches what we expected
    if token_response.sub != oauth_state.did {
        tracing::error!(
            expected = %oauth_state.did,
            got = %token_response.sub,
            "DID mismatch in token response"
        );
        return Err(AtrgError::Auth(format!(
            "DID mismatch: expected {}, got {}",
            oauth_state.did, token_response.sub
        )));
    }

    // 5. Create an atrg session
    let session_id = session::generate_session_id();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let expires_in = token_response.expires_in.unwrap_or(86400);
    let expires_at = now + expires_in as i64;

    session::create_session(
        &state.db,
        &session_id,
        &token_response.sub,
        &oauth_state.handle,
        &token_response.access_token,
        token_response.refresh_token.as_deref(),
        expires_at,
    )
    .await
    .map_err(|e| AtrgError::Internal(anyhow::anyhow!("failed to create session: {}", e)))?;

    tracing::info!(
        did = %token_response.sub,
        handle = %oauth_state.handle,
        expires_in = expires_in,
        "OAuth login successful, session created"
    );

    // 6. Set the session cookie and redirect to the frontend
    let is_secure = state.config.app.environment != "development";
    let cookie_value = format!(
        "atrg_session={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{}",
        session_id,
        expires_in,
        if is_secure { "; Secure" } else { "" }
    );

    let redirect_url = if oauth_state.redirect_after.starts_with("http://")
        || oauth_state.redirect_after.starts_with("https://")
    {
        // Cross-origin redirect — append token params so the SPA can store them.
        // The cookie won't be readable on a different domain.
        let separator = if oauth_state.redirect_after.contains('?') {
            "&"
        } else {
            "?"
        };
        format!(
            "{}{}token={}&did={}&handle={}",
            oauth_state.redirect_after,
            separator,
            session_id,
            token_response.sub,
            oauth_state.handle,
        )
    } else {
        // Same-origin relative redirect — cookie works fine
        oauth_state.redirect_after.clone()
    };

    let mut response = axum::response::Redirect::temporary(&redirect_url).into_response();

    if let Ok(val) = HeaderValue::from_str(&cookie_value) {
        response.headers_mut().insert("set-cookie", val);
    }

    Ok(response)
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
            }),
            db,
            http: reqwest::Client::new(),
            identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
                reqwest::Client::new(),
            )),
            extensions: Arc::new(atrg_core::Extensions::new()),
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

    #[tokio::test]
    async fn callback_without_code_returns_400() {
        let state = test_state().await;
        let app = test_router(state);
        let resp = app
            .oneshot(
                Request::get("/auth/callback?state=abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn callback_without_state_returns_400() {
        let state = test_state().await;
        let app = test_router(state);
        let resp = app
            .oneshot(
                Request::get("/auth/callback?code=abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn callback_with_invalid_state_returns_400() {
        let state = test_state().await;
        let app = test_router(state);
        let resp = app
            .oneshot(
                Request::get("/auth/callback?code=abc&state=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn callback_with_error_returns_401() {
        let state = test_state().await;
        let app = test_router(state);
        let resp = app
            .oneshot(
                Request::get("/auth/callback?error=access_denied&error_description=user+denied")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[test]
    fn origin_of_strips_path() {
        assert_eq!(
            super::origin_of("https://example.com/client-metadata.json"),
            "https://example.com"
        );
        assert_eq!(
            super::origin_of("http://localhost:3000/client-metadata.json"),
            "http://localhost:3000"
        );
        assert_eq!(
            super::origin_of("http://localhost:3000"),
            "http://localhost:3000"
        );
    }
}
