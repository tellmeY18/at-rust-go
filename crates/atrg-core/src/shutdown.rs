//! Graceful shutdown utilities for the atrg server.
//!
//! Provides a signal-awaiting future suitable for
//! [`axum::serve::Serve::with_graceful_shutdown`] and a post-shutdown cleanup
//! helper that drains the database pool with a configurable timeout.

use std::time::Duration;

use tokio::signal;

/// Default timeout (in seconds) for shutdown cleanup operations.
const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 30;

/// Future that resolves when the process receives a shutdown signal.
///
/// On Unix this listens for both `SIGINT` (Ctrl+C) and `SIGTERM`.
/// On other platforms only `SIGINT` is supported.
///
/// # Usage
///
/// ```rust,ignore
/// axum::serve(listener, router)
///     .with_graceful_shutdown(atrg_core::shutdown_signal())
///     .await?;
/// ```
pub async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("received Ctrl+C, initiating graceful shutdown");
        }
        _ = terminate => {
            tracing::info!("received SIGTERM, initiating graceful shutdown");
        }
    }
}

/// Run cleanup tasks after the server stops accepting connections.
///
/// Currently this closes the SQLite connection pool, waiting up to `timeout`
/// for in-flight queries to complete. If no timeout is provided the default
/// of 30 seconds is used.
pub async fn shutdown_cleanup(db: &sqlx::SqlitePool, timeout: Option<Duration>) {
    let timeout = timeout.unwrap_or(Duration::from_secs(DEFAULT_SHUTDOWN_TIMEOUT_SECS));
    tracing::info!(timeout_secs = timeout.as_secs(), "running shutdown cleanup");

    let cleanup = async {
        tracing::debug!("closing database connection pool");
        db.close().await;
        tracing::debug!("database pool closed");
    };

    match tokio::time::timeout(timeout, cleanup).await {
        Ok(()) => tracing::info!("shutdown cleanup completed"),
        Err(_) => tracing::warn!("shutdown cleanup timed out"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_is_30_seconds() {
        assert_eq!(DEFAULT_SHUTDOWN_TIMEOUT_SECS, 30);
    }

    #[tokio::test]
    async fn shutdown_cleanup_completes_with_fresh_pool() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        shutdown_cleanup(&pool, Some(Duration::from_secs(5))).await;
        assert!(pool.is_closed());
    }

    #[tokio::test]
    async fn shutdown_cleanup_handles_already_closed_pool() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        pool.close().await;
        // Should not panic
        shutdown_cleanup(&pool, Some(Duration::from_secs(1))).await;
    }
}
