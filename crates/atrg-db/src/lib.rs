//! Database layer for at-rust-go: SQLite and/or PostgreSQL connection pooling
//! and migrations.
//!
//! This crate provides a thin wrapper around `sqlx` exposing a [`DbPool`] enum
//! that wraps either a SQLite or a PostgreSQL connection pool. The variants
//! are gated behind cargo features:
//!
//! - `sqlite` *(default)* — pulls in the SQLite driver and enables
//!   [`DbPool::Sqlite`].
//! - `postgres` *(optional)* — pulls in the PostgreSQL driver and enables
//!   [`DbPool::Postgres`].
//!
//! At runtime, [`connect`] inspects the URL scheme (`sqlite://`, `sqlite::memory:`
//! → SQLite; `postgres://`, `postgresql://` → PostgreSQL) and returns the
//! appropriate variant. If the matching driver was not compiled in, an error
//! is returned.
//!
//! Internal migrations live under `migrations/sqlite/` and `migrations/postgres/`
//! and are embedded at compile time; only the migrations matching the active
//! pool variant are run.

#![deny(unsafe_code)]
#![warn(missing_docs)]

#[cfg(not(any(feature = "sqlite", feature = "postgres")))]
compile_error!(
    "atrg-db requires at least one of the `sqlite` or `postgres` cargo features"
);

#[cfg(feature = "sqlite")]
use std::str::FromStr;

#[cfg(feature = "sqlite")]
use sqlx::SqlitePool;

#[cfg(feature = "postgres")]
use sqlx::PgPool;

/// A database connection pool — either SQLite or PostgreSQL.
///
/// `DbPool` is the primary database handle threaded through atrg's
/// [`AppState`](../atrg_core/struct.AppState.html). It is cheaply
/// cloneable (the underlying sqlx pools are themselves `Arc`-based).
#[derive(Clone)]
pub enum DbPool {
    /// A SQLite connection pool. Available when the `sqlite` cargo feature
    /// is enabled (the default).
    #[cfg(feature = "sqlite")]
    Sqlite(SqlitePool),
    /// A PostgreSQL connection pool. Available when the `postgres` cargo
    /// feature is enabled.
    #[cfg(feature = "postgres")]
    Postgres(PgPool),
}

impl DbPool {
    /// Borrow the inner SQLite pool, if any.
    #[cfg(feature = "sqlite")]
    pub fn as_sqlite(&self) -> Option<&SqlitePool> {
        match self {
            DbPool::Sqlite(pool) => Some(pool),
            #[cfg(feature = "postgres")]
            DbPool::Postgres(_) => None,
        }
    }

    /// Borrow the inner PostgreSQL pool, if any.
    #[cfg(feature = "postgres")]
    pub fn as_postgres(&self) -> Option<&PgPool> {
        match self {
            DbPool::Postgres(pool) => Some(pool),
            #[cfg(feature = "sqlite")]
            DbPool::Sqlite(_) => None,
        }
    }

    /// Returns a static string identifying the backend kind: `"sqlite"` or
    /// `"postgres"`. Useful for diagnostics and tests.
    pub fn backend(&self) -> &'static str {
        match self {
            #[cfg(feature = "sqlite")]
            DbPool::Sqlite(_) => "sqlite",
            #[cfg(feature = "postgres")]
            DbPool::Postgres(_) => "postgres",
        }
    }

    /// Close the pool, waiting for in-flight queries to complete.
    pub async fn close(&self) {
        match self {
            #[cfg(feature = "sqlite")]
            DbPool::Sqlite(p) => p.close().await,
            #[cfg(feature = "postgres")]
            DbPool::Postgres(p) => p.close().await,
        }
    }

    /// Whether the underlying pool has been closed.
    pub fn is_closed(&self) -> bool {
        match self {
            #[cfg(feature = "sqlite")]
            DbPool::Sqlite(p) => p.is_closed(),
            #[cfg(feature = "postgres")]
            DbPool::Postgres(p) => p.is_closed(),
        }
    }

    /// Run a trivial `SELECT 1` round-trip against the pool; used by the
    /// `/readyz` health endpoint.
    pub async fn ping(&self) -> anyhow::Result<()> {
        match self {
            #[cfg(feature = "sqlite")]
            DbPool::Sqlite(p) => {
                sqlx::query("SELECT 1").execute(p).await?;
            }
            #[cfg(feature = "postgres")]
            DbPool::Postgres(p) => {
                sqlx::query("SELECT 1").execute(p).await?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Debug for DbPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DbPool").field(&self.backend()).finish()
    }
}

#[cfg(feature = "sqlite")]
impl From<SqlitePool> for DbPool {
    fn from(p: SqlitePool) -> Self {
        DbPool::Sqlite(p)
    }
}

#[cfg(feature = "postgres")]
impl From<PgPool> for DbPool {
    fn from(p: PgPool) -> Self {
        DbPool::Postgres(p)
    }
}

/// Backwards-compatible alias used throughout earlier versions of atrg.
///
/// New code should prefer [`DbPool`] directly.
pub type DbConn = DbPool;

/// Connect to a database, choosing the backend from the URL scheme.
///
/// - `sqlite://path` or `sqlite::memory:` → returns [`DbPool::Sqlite`]
///   (requires the `sqlite` feature).
/// - `postgres://...` or `postgresql://...` → returns [`DbPool::Postgres`]
///   (requires the `postgres` feature).
///
/// SQLite pools are configured with `create_if_missing(true)`, WAL journal
/// mode, and foreign keys enabled. PostgreSQL pools are configured with up
/// to 8 connections.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> anyhow::Result<()> {
/// // SQLite (requires the `sqlite` feature)
/// let pool = atrg_db::connect("sqlite://atrg.db").await?;
/// // PostgreSQL (requires the `postgres` feature)
/// // let pool = atrg_db::connect("postgres://user:pass@host/db").await?;
/// # Ok(())
/// # }
/// ```
pub async fn connect(url: &str) -> anyhow::Result<DbPool> {
    let scheme = url.split(':').next().unwrap_or("").to_ascii_lowercase();
    match scheme.as_str() {
        "sqlite" => {
            #[cfg(feature = "sqlite")]
            {
                let opts = sqlx::sqlite::SqliteConnectOptions::from_str(url)?
                    .create_if_missing(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .foreign_keys(true);

                let pool = sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(8)
                    .connect_with(opts)
                    .await?;

                tracing::info!("connected to SQLite database: {}", url);
                Ok(DbPool::Sqlite(pool))
            }
            #[cfg(not(feature = "sqlite"))]
            {
                anyhow::bail!(
                    "atrg-db was built without the `sqlite` feature; cannot open {}",
                    url
                )
            }
        }
        "postgres" | "postgresql" => {
            #[cfg(feature = "postgres")]
            {
                let pool = sqlx::postgres::PgPoolOptions::new()
                    .max_connections(8)
                    .connect(url)
                    .await?;

                tracing::info!("connected to PostgreSQL database");
                Ok(DbPool::Postgres(pool))
            }
            #[cfg(not(feature = "postgres"))]
            {
                anyhow::bail!(
                    "atrg-db was built without the `postgres` feature; \
                     enable it (e.g. `cargo build --features atrg-db/postgres`) \
                     to use {}",
                    url
                )
            }
        }
        other => anyhow::bail!(
            "unsupported database URL scheme `{}`; expected `sqlite://`, `postgres://`, or `postgresql://`",
            other
        ),
    }
}

/// Run atrg's internal migrations against the active backend.
///
/// The migrations live under `migrations/<backend>/` in this crate and are
/// embedded at compile time. They are idempotent and safe to run on every
/// startup.
pub async fn run_internal_migrations(pool: &DbPool) -> anyhow::Result<()> {
    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            let migrator = sqlx::migrate!("./migrations/sqlite");
            let n = migrator.migrations.len();
            migrator.run(p).await?;
            tracing::info!(count = n, backend = "sqlite", "applied atrg internal migrations");
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            let migrator = sqlx::migrate!("./migrations/postgres");
            let n = migrator.migrations.len();
            migrator.run(p).await?;
            tracing::info!(count = n, backend = "postgres", "applied atrg internal migrations");
        }
    }
    Ok(())
}

/// Run user-supplied migrations from `dir` against the active backend.
///
/// If the directory does not exist or contains no `.sql` files, this function
/// returns `Ok(())` silently.
pub async fn run_user_migrations(
    pool: &DbPool,
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
    let n = migrator.migrations.len();

    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => migrator.run(p).await?,
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => migrator.run(p).await?,
    }

    tracing::info!(
        count = n,
        path = %dir.display(),
        backend = pool.backend(),
        "applied user migrations (if pending)"
    );

    Ok(())
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connect_memory() {
        let pool = connect("sqlite::memory:").await.expect("connect");
        assert_eq!(pool.backend(), "sqlite");
        pool.ping().await.expect("ping");
    }

    #[tokio::test]
    async fn test_internal_migrations() {
        let pool = connect("sqlite::memory:").await.expect("connect");
        run_internal_migrations(&pool)
            .await
            .expect("run internal migrations");

        let sqlite = pool.as_sqlite().expect("sqlite pool");
        let row: (String,) = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='atrg_sessions'",
        )
        .fetch_one(sqlite)
        .await
        .expect("atrg_sessions exists");
        assert_eq!(row.0, "atrg_sessions");
    }

    #[tokio::test]
    async fn test_migrations_idempotent() {
        let pool = connect("sqlite::memory:").await.expect("connect");
        run_internal_migrations(&pool).await.expect("first run");
        run_internal_migrations(&pool).await.expect("second run");
    }

    #[tokio::test]
    async fn test_user_migrations_empty_dir() {
        let pool = connect("sqlite::memory:").await.expect("connect");
        let tmp_dir = std::env::temp_dir().join(format!("atrg_test_empty_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).expect("mkdir");

        let result = run_user_migrations(&pool, &tmp_dir).await;
        let _ = std::fs::remove_dir_all(&tmp_dir);
        result.expect("empty dir succeeds silently");
    }

    #[tokio::test]
    async fn test_user_migrations_nonexistent_dir() {
        let pool = connect("sqlite::memory:").await.expect("connect");
        let nonexistent = std::path::Path::new("/tmp/atrg_test_nonexistent_dir_that_does_not_exist");
        run_user_migrations(&pool, nonexistent)
            .await
            .expect("nonexistent dir succeeds silently");
    }

    #[tokio::test]
    async fn unsupported_scheme_errors() {
        let err = connect("mysql://localhost/db").await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("unsupported database URL scheme"), "got: {msg}");
    }

    #[cfg(not(feature = "postgres"))]
    #[tokio::test]
    async fn postgres_url_without_feature_errors() {
        let err = connect("postgres://user:pass@localhost/db")
            .await
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("postgres") && msg.contains("feature"),
            "got: {msg}"
        );
    }
}
