//! Framework error types that serialize to JSON responses.
//!
//! Every handler in an atrg application returns `Result<impl IntoResponse, AtrgError>`.
//! Errors are automatically converted to JSON with the shape:
//! ```json
//! { "error": "<code>", "message": "<human readable>" }
//! ```

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// Convenience alias used throughout the framework.
pub type AtrgResult<T> = Result<T, AtrgError>;

/// Unified error type for atrg handlers.
///
/// Each variant maps to a specific HTTP status code and a JSON error envelope.
#[derive(Debug, thiserror::Error)]
pub enum AtrgError {
    /// A database operation failed. Maps to `500 Internal Server Error`.
    /// The underlying error is logged but NOT exposed to clients.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// The request requires authentication but none was provided or the
    /// credentials were invalid. Maps to `401 Unauthorized`.
    #[error("unauthorized: {0}")]
    Auth(String),

    /// The requested resource was not found. Maps to `404 Not Found`.
    #[error("not found")]
    NotFound,

    /// The request was malformed or contained invalid parameters.
    /// Maps to `400 Bad Request`.
    #[error("bad request: {0}")]
    BadRequest(String),

    /// A catch-all for unexpected internal errors. Maps to `500 Internal Server Error`.
    /// The underlying error is logged but NOT exposed to clients.
    #[error("internal error: {0}")]
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for AtrgError {
    fn from(err: anyhow::Error) -> Self {
        AtrgError::Internal(err)
    }
}

impl IntoResponse for AtrgError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AtrgError::NotFound => (StatusCode::NOT_FOUND, "not_found", "Not found".to_string()),
            AtrgError::Auth(m) => (StatusCode::UNAUTHORIZED, "unauthorized", m.clone()),
            AtrgError::BadRequest(m) => (StatusCode::BAD_REQUEST, "bad_request", m.clone()),
            AtrgError::Database(e) => {
                tracing::error!(error = %e, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Database error".to_string(),
                )
            }
            AtrgError::Internal(e) => {
                tracing::error!(error = %e, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Internal server error".to_string(),
                )
            }
        };

        (
            status,
            Json(serde_json::json!({
                "error": code,
                "message": message,
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;

    async fn error_to_parts(err: AtrgError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let body = response.into_body();
        let bytes = Body::new(body).collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn not_found_returns_404() {
        let (status, body) = error_to_parts(AtrgError::NotFound).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not_found");
        assert_eq!(body["message"], "Not found");
    }

    #[tokio::test]
    async fn auth_returns_401() {
        let (status, body) = error_to_parts(AtrgError::Auth("bad token".into())).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "unauthorized");
        assert_eq!(body["message"], "bad token");
    }

    #[tokio::test]
    async fn bad_request_returns_400() {
        let (status, body) = error_to_parts(AtrgError::BadRequest("missing field".into())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "bad_request");
        assert_eq!(body["message"], "missing field");
    }

    #[tokio::test]
    async fn database_error_returns_500() {
        let err = AtrgError::Database(sqlx::Error::RowNotFound);
        let (status, body) = error_to_parts(err).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "database_error");
        assert_eq!(body["message"], "Database error");
    }

    #[tokio::test]
    async fn internal_error_returns_500() {
        let err = AtrgError::Internal(anyhow::anyhow!("something broke"));
        let (status, body) = error_to_parts(err).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "internal_error");
        assert_eq!(body["message"], "Internal server error");
    }

    #[tokio::test]
    async fn response_content_type_is_json() {
        let response = AtrgError::NotFound.into_response();
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            content_type.contains("application/json"),
            "expected application/json, got: {content_type}"
        );
    }

    #[test]
    fn from_sqlx_error() {
        let err: AtrgError = sqlx::Error::RowNotFound.into();
        assert!(matches!(err, AtrgError::Database(_)));
    }

    #[test]
    fn from_anyhow_error() {
        let err: AtrgError = anyhow::anyhow!("boom").into();
        assert!(matches!(err, AtrgError::Internal(_)));
    }

    #[tokio::test]
    async fn response_body_has_exactly_two_keys() {
        let (_, body) = error_to_parts(AtrgError::NotFound).await;
        let obj = body.as_object().unwrap();
        assert_eq!(obj.len(), 2, "expected exactly 'error' and 'message' keys");
        assert!(obj.contains_key("error"));
        assert!(obj.contains_key("message"));
    }
}
