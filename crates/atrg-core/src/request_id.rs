//! Request ID middleware — assigns a unique ID to each request.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;

static REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Middleware that reads or generates a request ID and attaches it to the response.
pub async fn request_id_middleware(mut req: Request, next: Next) -> axum::response::Response {
    // Use existing header or generate a new one
    let request_id = req
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(generate_request_id);

    // Add to request extensions for handler access
    req.extensions_mut().insert(RequestId(request_id.clone()));

    let mut response = next.run(req).await;

    // Echo back in response
    if let Ok(val) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(REQUEST_ID_HEADER.clone(), val);
    }

    response
}

/// A request ID extracted from the middleware.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

fn generate_request_id() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_request_id_is_32_hex_chars() {
        let id = generate_request_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_request_id_is_unique() {
        let a = generate_request_id();
        let b = generate_request_id();
        assert_ne!(a, b);
    }
}
