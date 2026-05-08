//! Application state shared across all Axum handlers.

use std::sync::Arc;

use atrg_db::DbPool;

use crate::config::Config;
use atrg_identity::IdentityResolver;

/// Shared application state passed to every Axum handler.
///
/// This is the central state object that every route handler receives via
/// `axum::extract::State<AppState>`. It holds the parsed configuration,
/// database connection pool, and a shared HTTP client for outbound requests.
///
/// `AppState` is cheaply cloneable — all inner fields are either `Arc`-wrapped
/// or already use internal reference counting (e.g. sqlx pools, `reqwest::Client`).
#[derive(Clone)]
pub struct AppState {
    /// Parsed configuration from `atrg.toml`.
    pub config: Arc<Config>,
    /// Database connection pool. May be SQLite or PostgreSQL depending on
    /// the `[database] url` scheme in `atrg.toml` (and which features are
    /// compiled in to `atrg-db`).
    pub db: DbPool,
    /// Shared HTTP client for outbound requests.
    pub http: reqwest::Client,
    /// DID/handle resolver with TTL-backed in-memory cache.
    pub identity: Arc<IdentityResolver>,
}

// ---------------------------------------------------------------------------
// FromRef implementations — allow Axum sub-extractors to pull individual
// fields out of AppState without the handler needing to destructure manually.
// ---------------------------------------------------------------------------

impl axum::extract::FromRef<AppState> for DbPool {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<Config> {
    fn from_ref(state: &AppState) -> Self {
        state.config.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<IdentityResolver> {
    fn from_ref(state: &AppState) -> Self {
        state.identity.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time assertion helper.
    fn _assert_send_sync_clone<T: Send + Sync + Clone>() {}

    #[test]
    fn app_state_is_send_sync_clone() {
        _assert_send_sync_clone::<AppState>();
    }
}
