//! Route wiring for the social API example.

use atrg_core::AppState;
use axum::{routing::get, Router};

use crate::handlers;

pub fn api() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::index))
        .route("/api/timeline", get(handlers::timeline::timeline))
        .route("/api/profile/{handle}", get(handlers::profile::profile))
}
