#![deny(unsafe_code)]
#![warn(missing_docs)]
//! XRPC route registration helpers and error types for at-rust-go.
//!
//! Provides [`xrpc_router()`] — a pre-configured Axum router for `/xrpc/*`
//! endpoints that automatically wraps errors in the AT Protocol error envelope.

pub mod error;

pub use error::{XrpcError, XrpcErrorName};

use axum::response::IntoResponse;
use axum::routing::any;
use axum::Router;

/// Create a new XRPC router with the standard AT Protocol error fallback.
///
/// Mount your XRPC procedures on this router, then merge it into your app:
///
/// ```rust,ignore
/// let xrpc = atrg_xrpc::xrpc_router()
///     .route("/xrpc/com.example.getPosts", get(get_posts))
///     .route("/xrpc/com.example.createPost", post(create_post));
/// ```
pub fn xrpc_router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().fallback(any(xrpc_fallback))
}

/// Fallback handler for unmatched XRPC methods.
async fn xrpc_fallback() -> impl IntoResponse {
    XrpcError {
        name: XrpcErrorName::MethodNotImplemented,
        message: "XRPC method not implemented".to_string(),
    }
}

/// Convenience constructor: returns an `InvalidRequest` error.
pub fn xrpc_invalid_request(msg: impl Into<String>) -> XrpcError {
    XrpcError {
        name: XrpcErrorName::InvalidRequest,
        message: msg.into(),
    }
}

/// Convenience constructor: returns an `AuthRequired` error.
pub fn xrpc_auth_required(msg: impl Into<String>) -> XrpcError {
    XrpcError {
        name: XrpcErrorName::AuthRequired,
        message: msg.into(),
    }
}

/// Convenience constructor: returns a `Forbidden` error.
pub fn xrpc_forbidden(msg: impl Into<String>) -> XrpcError {
    XrpcError {
        name: XrpcErrorName::Forbidden,
        message: msg.into(),
    }
}

/// Convenience constructor: returns a `NotFound` error.
pub fn xrpc_not_found(msg: impl Into<String>) -> XrpcError {
    XrpcError {
        name: XrpcErrorName::NotFound,
        message: msg.into(),
    }
}

/// Convenience constructor: returns a `RateLimitExceeded` error.
pub fn xrpc_rate_limit(msg: impl Into<String>) -> XrpcError {
    XrpcError {
        name: XrpcErrorName::RateLimitExceeded,
        message: msg.into(),
    }
}
