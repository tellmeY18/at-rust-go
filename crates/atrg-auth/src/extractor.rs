//! Axum extractors for authentication.
//!
//! - [`AuthUser`] — optional; returns `None` if not authenticated.
//! - [`RequireAuth`] — strict; rejects with 401 if not authenticated.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::header;
use axum::http::request::Parts;

use atrg_core::error::AtrgError;
use atrg_core::state::AppState;

use crate::jwt;
use crate::session::{self, AtrgSession, AuthSource};

/// Optional authentication extractor.
///
/// Reads the `atrg_session` cookie or `Authorization: Bearer` header
/// and resolves the session. Returns `AuthUser(None)` if no valid
/// credential is found — does NOT reject the request.
///
/// ```rust,ignore
/// async fn handler(AuthUser(user): AuthUser) -> impl IntoResponse {
///     match user {
///         Some(session) => Json(json!({"did": session.did})),
///         None => Json(json!({"authenticated": false})),
///     }
/// }
/// ```
pub struct AuthUser(pub Option<AtrgSession>);

/// Strict authentication extractor.
///
/// Same logic as `AuthUser`, but rejects with `401 Unauthorized` JSON
/// if no valid session is found.
///
/// ```rust,ignore
/// async fn handler(RequireAuth(session): RequireAuth) -> impl IntoResponse {
///     Json(json!({"did": session.did}))
/// }
/// ```
pub struct RequireAuth(pub AtrgSession);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let session = resolve_session(parts, &app_state).await;
        Ok(AuthUser(session))
    }
}

impl<S> FromRequestParts<S> for RequireAuth
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = AtrgError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let session = resolve_session(parts, &app_state).await;
        match session {
            Some(s) => Ok(RequireAuth(s)),
            None => Err(AtrgError::Auth("unauthenticated".to_string())),
        }
    }
}

/// Core session resolution logic shared by both extractors.
///
/// Priority:
/// 1. `Authorization: Bearer <token>` header
///    a. If token looks like a JWT → try AT Protocol JWT verification
///    b. Otherwise → look up as atrg session ID
/// 2. `atrg_session=<id>` cookie → look up as atrg session ID
async fn resolve_session(parts: &Parts, state: &AppState) -> Option<AtrgSession> {
    // Try Authorization header first
    if let Some(auth_header) = parts.headers.get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                let token = token.trim();
                if !token.is_empty() {
                    return resolve_bearer_token(token, state).await;
                }
            }
        }
    }

    // Fall back to cookie
    if let Some(cookie_header) = parts.headers.get(header::COOKIE) {
        if let Ok(cookies) = cookie_header.to_str() {
            if let Some(session_id) = extract_cookie_value(cookies, "atrg_session") {
                return resolve_atrg_session(session_id, state).await;
            }
        }
    }

    None
}

/// Resolve a bearer token — JWT, API key, or atrg session.
async fn resolve_bearer_token(token: &str, state: &AppState) -> Option<AtrgSession> {
    // If it looks like a JWT, try parsing it as an AT Protocol JWT
    if jwt::looks_like_jwt(token) {
        if let Ok(claims) = jwt::decode_claims_unverified(token) {
            if jwt::verify_expiration(&claims).is_ok() {
                let host = &state.config.app.host;
                if jwt::verify_audience(&claims, host).is_ok() || claims.aud.is_none() {
                    tracing::debug!(
                        iss = %claims.iss,
                        sub = %claims.sub,
                        "accepted AT Protocol JWT"
                    );
                    return Some(AtrgSession {
                        did: claims.sub,
                        handle: String::new(),
                        access_token: token.to_string(),
                        refresh_token: None,
                        expires_at: claims.exp.unwrap_or(0) as i64,
                        source: AuthSource::AtprotoJwt,
                    });
                }
            }
        }
        // If JWT parsing failed, fall through
    }

    // Try as API key (contains underscore prefix pattern like "atrg_" or "chg_")
    if token.contains('_') {
        if let Ok(Some(api_key)) = crate::api_keys::find_by_key(&state.db, token).await {
            tracing::debug!(
                did = %api_key.did,
                prefix = %api_key.key_prefix,
                "authenticated via API key"
            );
            return Some(AtrgSession {
                did: api_key.did,
                handle: String::new(),
                access_token: token.to_string(),
                refresh_token: None,
                expires_at: api_key.expires_at.unwrap_or(i64::MAX),
                source: AuthSource::ApiKey,
            });
        }
    }

    // Try as atrg session token
    resolve_atrg_session(token, state).await
}

/// Look up an atrg session by ID from the database.
async fn resolve_atrg_session(session_id: &str, state: &AppState) -> Option<AtrgSession> {
    match session::find_session(&state.db, session_id).await {
        Ok(session) => session,
        Err(e) => {
            tracing::warn!(error = %e, "failed to look up session");
            None
        }
    }
}

/// Parse a cookie header string and extract the value for a given name.
pub(crate) fn extract_cookie_value<'a>(cookies: &'a str, name: &str) -> Option<&'a str> {
    cookies.split(';').map(|s| s.trim()).find_map(|cookie| {
        let (key, value) = cookie.split_once('=')?;
        if key.trim() == name {
            Some(value.trim())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_cookie_value_present() {
        let cookies = "foo=bar; atrg_session=abc123; other=val";
        assert_eq!(
            extract_cookie_value(cookies, "atrg_session"),
            Some("abc123")
        );
    }

    #[test]
    fn extract_cookie_value_missing() {
        let cookies = "foo=bar; other=val";
        assert_eq!(extract_cookie_value(cookies, "atrg_session"), None);
    }

    #[test]
    fn extract_cookie_value_empty() {
        assert_eq!(extract_cookie_value("", "atrg_session"), None);
    }

    #[test]
    fn extract_cookie_value_single() {
        assert_eq!(
            extract_cookie_value("atrg_session=xyz", "atrg_session"),
            Some("xyz")
        );
    }
}
