//! CORS layer builder.
//!
//! Constructs a [`tower_http::cors::CorsLayer`] from the allowed origins
//! listed in `atrg.toml` under `[app].cors_origins`.

use axum::http::{self, Method};
use tower_http::cors::CorsLayer;

/// Build a CORS layer from the configured allowed origins.
///
/// - Empty list → same-origin only (no `Access-Control-Allow-Origin` header).
/// - `["*"]` → fully permissive (all origins, all methods).
/// - Specific origins → allowlist with credentials support.
pub fn build_cors_layer(origins: &[String]) -> CorsLayer {
    if origins.is_empty() {
        // Same-origin only — no CORS headers emitted.
        CorsLayer::new()
    } else if origins.len() == 1 && origins[0] == "*" {
        tracing::warn!("CORS configured with wildcard '*' — all origins allowed");
        CorsLayer::permissive()
    } else {
        let parsed_origins: Vec<http::HeaderValue> = origins
            .iter()
            .filter_map(|o| match o.parse::<http::HeaderValue>() {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!(origin = %o, error = %e, "Skipping invalid CORS origin");
                    None
                }
            })
            .collect();

        CorsLayer::new()
            .allow_origin(parsed_origins)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::PATCH,
                Method::OPTIONS,
            ])
            .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION])
            .allow_credentials(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_origins_returns_layer() {
        // Should not panic — produces a restrictive (same-origin) layer.
        let _layer = build_cors_layer(&[]);
    }

    #[test]
    fn wildcard_returns_permissive_layer() {
        let _layer = build_cors_layer(&["*".to_string()]);
    }

    #[test]
    fn specific_origins_returns_layer() {
        let origins = vec![
            "http://localhost:5173".to_string(),
            "https://myapp.example.com".to_string(),
        ];
        let _layer = build_cors_layer(&origins);
    }

    #[test]
    fn invalid_origin_is_skipped() {
        // A header value with invalid bytes should be filtered out without panic.
        let origins = vec![
            "http://localhost:5173".to_string(),
            "not a valid \x00 header".to_string(),
        ];
        let _layer = build_cors_layer(&origins);
    }
}
