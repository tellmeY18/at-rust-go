//! API handlers for the social example.

pub mod profile;
pub mod timeline;

use axum::Json;

/// Health check / index endpoint.
pub async fn index() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "social-example",
        "status": "ok",
        "description": "AT Protocol social API built with at-rust-go"
    }))
}
