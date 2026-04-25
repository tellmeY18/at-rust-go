//! Security headers middleware for production deployments.
//!
//! In non-development environments, adds standard security headers to all responses.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;

/// Security headers applied in non-development mode.
const SECURITY_HEADERS: &[(&str, &str)] = &[
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "strict-origin-when-cross-origin"),
    (
        "content-security-policy",
        "default-src 'none'; frame-ancestors 'none'",
    ),
];

/// Axum middleware that adds security headers in production.
pub async fn security_headers_middleware(req: Request, next: Next) -> axum::response::Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    for (name, value) in SECURITY_HEADERS {
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.insert(n, v);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_headers_are_valid() {
        for (name, value) in SECURITY_HEADERS {
            assert!(
                HeaderName::from_bytes(name.as_bytes()).is_ok(),
                "invalid header name: {name}"
            );
            assert!(
                HeaderValue::from_str(value).is_ok(),
                "invalid header value: {value}"
            );
        }
    }
}
