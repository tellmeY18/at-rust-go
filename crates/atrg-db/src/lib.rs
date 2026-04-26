//! Database layer for at-rust-go: SQLite connection pooling and migrations.
//!
//! This crate provides a thin wrapper around `sqlx` with SQLite, handling
//! connection pool creation with sensible defaults (WAL journal mode, foreign
//! keys enabled) and a two-stage migration system: internal migrations for
//! atrg's own tables (sessions, etc.) and user-supplied migrations for
//! application-specific schema.

#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::str::FromStr;

/// A SQLite connection pool. This is the primary database handle passed
/// throughout the atrg application via `AppState`.
pub type DbConn = sqlx::SqlitePool;

/// Connect to a SQLite database and return a connection pool.
///
/// The pool is configured with:
/// - `create_if_missing(true)` — the database file is created automatically
/// - WAL journal mode — better concurrent read performance
/// - Foreign keys enabled — referential integrity is enforced
/// - Up to 8 concurrent connections
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> anyhow::Result<()> {
/// let pool = atrg_db::connect("sqlite://atrg.db").await?;
/// # Ok(())
/// # }
/// ```
pub async fn connect(url: &str) -> anyhow::Result<sqlx::SqlitePool> {
    let opts = sqlx::sqlite::SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;

    tracing::info!("connected to SQLite database: {}", url);

    Ok(pool)
}

/// Run atrg's internal migrations (sessions table, etc.).
///
/// These migrations are embedded in the `atrg-db` crate at compile time
/// from the `migrations/` directory next to this crate's `Cargo.toml`.
/// They are idempotent and safe to run on every startup.
pub async fn run_internal_migrations(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
    let migrator = sqlx::migrate!("./migrations");
    let num_migrations = migrator.migrations.len();

    migrator.run(pool).await?;

    tracing::info!(
        count = num_migrations,
        "applied atrg internal migrations (if pending)"
    );

    Ok(())
}

/// Run user-supplied migrations from the given directory.
///
/// If the directory does not exist or contains no `.sql` files, this
/// function returns `Ok(())` silently — it is not an error for a project
/// to have no custom migrations yet.
///
/// Migrations are discovered and applied in filename order using the
/// standard `sqlx` migration conventions.
pub async fn run_user_migrations(
    pool: &sqlx::SqlitePool,
    dir: &std::path::Path,
) -> anyhow::Result<()> {
    if !dir.exists() {
        tracing::debug!(
            path = %dir.display(),
            "user migrations directory does not exist, skipping"
        );
        return Ok(());
    }

    let has_sql_files = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "sql"));

    if !has_sql_files {
        tracing::debug!(
            path = %dir.display(),
            "user migrations directory contains no .sql files, skipping"
        );
        return Ok(());
    }

    let migrator = sqlx::migrate::Migrator::new(dir).await?;
    let num_migrations = migrator.migrations.len();

    migrator.run(pool).await?;

    tracing::info!(
        count = num_migrations,
        path = %dir.display(),
        "applied user migrations (if pending)"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connect_memory() {
        let pool = connect("sqlite::memory:")
            .await
            .expect("should connect to in-memory SQLite");

        let row: (i32,) = sqlx::query_as("SELECT 1")
            .fetch_one(&pool)
            .await
            .expect("should execute SELECT 1");

        assert_eq!(row.0, 1);
    }

    #[tokio::test]
    async fn test_internal_migrations() {
        let pool = connect("sqlite::memory:").await.expect("should connect");

        run_internal_migrations(&pool)
            .await
            .expect("should run internal migrations");

        let row: (String,) = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='atrg_sessions'",
        )
        .fetch_one(&pool)
        .await
        .expect("atrg_sessions table should exist");

        assert_eq!(row.0, "atrg_sessions");
    }

    #[tokio::test]
    async fn test_migrations_idempotent() {
        let pool = connect("sqlite::memory:").await.expect("should connect");

        run_internal_migrations(&pool)
            .await
            .expect("first run should succeed");

        run_internal_migrations(&pool)
            .await
            .expect("second run should also succeed (idempotent)");
    }

    #[tokio::test]
    async fn test_user_migrations_empty_dir() {
        let pool = connect("sqlite::memory:").await.expect("should connect");

        let tmp_dir = std::env::temp_dir().join(format!("atrg_test_empty_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).expect("should create temp dir");

        let result = run_user_migrations(&pool, &tmp_dir).await;

        // Clean up before asserting
        let _ = std::fs::remove_dir_all(&tmp_dir);

        result.expect("empty dir should succeed silently");
    }

    #[tokio::test]
    async fn test_user_migrations_nonexistent_dir() {
        let pool = connect("sqlite::memory:").await.expect("should connect");

        let nonexistent =
            std::path::Path::new("/tmp/atrg_test_nonexistent_dir_that_does_not_exist");

        run_user_migrations(&pool, nonexistent)
            .await
            .expect("nonexistent dir should succeed silently");
    }
}
