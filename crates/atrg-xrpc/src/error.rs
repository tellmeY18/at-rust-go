//! AT Protocol XRPC error envelope.
//!
//! Every `/xrpc/*` failure must use this type to ensure responses
//! conform to the AT Protocol error format.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use atrg_core::error::AtrgError;

/// XRPC error name variants per the AT Protocol spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrpcErrorName {
    /// The request was malformed. HTTP 400.
    InvalidRequest,
    /// Authentication is required. HTTP 401.
    AuthRequired,
    /// The authenticated user is not allowed to perform this action. HTTP 403.
    Forbidden,
    /// The requested resource was not found. HTTP 404.
    NotFound,
    /// Too many requests. HTTP 429.
    RateLimitExceeded,
    /// An unexpected server error occurred. HTTP 500.
    InternalServerError,
    /// The XRPC method is not implemented. HTTP 501.
    MethodNotImplemented,
}

impl XrpcErrorName {
    /// The string representation used in the JSON error envelope.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "InvalidRequest",
            Self::AuthRequired => "AuthRequired",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "NotFound",
            Self::RateLimitExceeded => "RateLimitExceeded",
            Self::InternalServerError => "InternalServerError",
            Self::MethodNotImplemented => "MethodNotImplemented",
        }
    }

    /// The HTTP status code for this error.
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::AuthRequired => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::RateLimitExceeded => StatusCode::TOO_MANY_REQUESTS,
            Self::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MethodNotImplemented => StatusCode::NOT_IMPLEMENTED,
        }
    }
}

/// AT Protocol XRPC error envelope.
///
/// Use this as the error type for all `/xrpc/*` handlers:
///
/// ```rust,ignore
/// async fn get_posts() -> Result<Json<Posts>, XrpcError> {
///     // ...
/// }
/// ```
#[derive(Debug)]
pub struct XrpcError {
    /// The error category.
    pub name: XrpcErrorName,
    /// Human-readable error message.
    pub message: String,
}

impl std::fmt::Display for XrpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name.as_str(), self.message)
    }
}

impl std::error::Error for XrpcError {}

impl IntoResponse for XrpcError {
    fn into_response(self) -> Response {
        let status = self.name.status_code();
        let body = serde_json::json!({
            "error": self.name.as_str(),
            "message": self.message,
        });
        (status, Json(body)).into_response()
    }
}

impl From<AtrgError> for XrpcError {
    fn from(err: AtrgError) -> Self {
        match err {
            AtrgError::NotFound => XrpcError {
                name: XrpcErrorName::NotFound,
                message: "Not found".to_string(),
            },
            AtrgError::Auth(msg) => XrpcError {
                name: XrpcErrorName::AuthRequired,
                message: msg,
            },
            AtrgError::BadRequest(msg) => XrpcError {
                name: XrpcErrorName::InvalidRequest,
                message: msg,
            },
            AtrgError::Database(_) => XrpcError {
                name: XrpcErrorName::InternalServerError,
                message: "Internal server error".to_string(),
            },
            AtrgError::Internal(_) => XrpcError {
                name: XrpcErrorName::InternalServerError,
                message: "Internal server error".to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;

    async fn error_to_parts(err: XrpcError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = Body::new(response.into_body())
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn invalid_request_400() {
        let (status, body) = error_to_parts(XrpcError {
            name: XrpcErrorName::InvalidRequest,
            message: "bad input".into(),
        })
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "InvalidRequest");
        assert_eq!(body["message"], "bad input");
    }

    #[tokio::test]
    async fn auth_required_401() {
        let (status, body) = error_to_parts(XrpcError {
            name: XrpcErrorName::AuthRequired,
            message: "login needed".into(),
        })
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "AuthRequired");
    }

    #[tokio::test]
    async fn forbidden_403() {
        let (status, _) = error_to_parts(XrpcError {
            name: XrpcErrorName::Forbidden,
            message: "nope".into(),
        })
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn not_found_404() {
        let (status, _) = error_to_parts(XrpcError {
            name: XrpcErrorName::NotFound,
            message: "gone".into(),
        })
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rate_limit_429() {
        let (status, _) = error_to_parts(XrpcError {
            name: XrpcErrorName::RateLimitExceeded,
            message: "slow down".into(),
        })
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn internal_500() {
        let (status, _) = error_to_parts(XrpcError {
            name: XrpcErrorName::InternalServerError,
            message: "oops".into(),
        })
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn not_implemented_501() {
        let (status, body) = error_to_parts(XrpcError {
            name: XrpcErrorName::MethodNotImplemented,
            message: "not here".into(),
        })
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["error"], "MethodNotImplemented");
    }

    #[tokio::test]
    async fn from_atrg_error_not_found() {
        let xrpc: XrpcError = AtrgError::NotFound.into();
        assert_eq!(xrpc.name, XrpcErrorName::NotFound);
    }

    #[tokio::test]
    async fn from_atrg_error_auth() {
        let xrpc: XrpcError = AtrgError::Auth("no".into()).into();
        assert_eq!(xrpc.name, XrpcErrorName::AuthRequired);
    }

    #[tokio::test]
    async fn from_atrg_error_bad_request() {
        let xrpc: XrpcError = AtrgError::BadRequest("bad".into()).into();
        assert_eq!(xrpc.name, XrpcErrorName::InvalidRequest);
    }

    #[tokio::test]
    async fn xrpc_router_fallback_returns_501() {
        use atrg_core::config::{AppConfig, AuthConfig, Config, DatabaseConfig};
        use atrg_core::state::AppState;
        use hyper::Request;
        use std::sync::Arc;
        use tower::ServiceExt;

        let db = atrg_db::connect("sqlite::memory:").await.unwrap();
        atrg_db::run_internal_migrations(&db).await.unwrap();
        let state = AppState {
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
                auth: AuthConfig::default(),
                database: DatabaseConfig::default(),
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
        };

        let app = crate::xrpc_router::<AppState>().with_state(state);

        let resp = app
            .oneshot(
                Request::get("/xrpc/com.nonexistent.method")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
        let bytes = Body::new(resp.into_body())
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "MethodNotImplemented");
    }
}
