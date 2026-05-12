//! Database layer for at-rust-go: SQLite and/or PostgreSQL connection pooling
//! and migrations.
//!
//! This crate provides a thin wrapper around `sqlx` exposing a [`DbPool`] enum
//! that wraps either a SQLite or a PostgreSQL connection pool. The variants
//! are gated behind cargo features:
//!
//! - `sqlite` *(default)* — pulls in the SQLite driver and enables
//!   `DbPool::Sqlite`.
//! - `postgres` *(optional)* — pulls in the PostgreSQL driver and enables
//!   `DbPool::Postgres`.
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
compile_error!("atrg-db requires at least one of the `sqlite` or `postgres` cargo features");

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
/// - `sqlite://path` or `sqlite::memory:` → returns `DbPool::Sqlite`
///   (requires the `sqlite` feature).
/// - `postgres://...` or `postgresql://...` → returns `DbPool::Postgres`
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
///
/// Uses the `_atrg_migrations` tracking table so that framework migrations
/// never conflict with application migrations or other binaries sharing
/// the same database.
pub async fn run_internal_migrations(pool: &DbPool) -> anyhow::Result<()> {
    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(_) => {
            let migrator = sqlx::migrate!("./migrations/sqlite");
            let n = migrator.migrations.len();
            run_migrator_with_table(pool, &migrator, "_atrg_migrations").await?;
            tracing::info!(
                count = n,
                backend = "sqlite",
                table = "_atrg_migrations",
                "applied atrg internal migrations"
            );
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(_) => {
            let migrator = sqlx::migrate!("./migrations/postgres");
            let n = migrator.migrations.len();
            run_migrator_with_table(pool, &migrator, "_atrg_migrations").await?;
            tracing::info!(
                count = n,
                backend = "postgres",
                table = "_atrg_migrations",
                "applied atrg internal migrations"
            );
        }
    }
    Ok(())
}

/// Run user-supplied migrations from `dir` against the active backend.
///
/// If the directory does not exist or contains no `.sql` files, this function
/// returns `Ok(())` silently.
///
/// # Deprecation Notice
///
/// This function uses the default `_sqlx_migrations` tracking table which
/// can conflict with other migration sets in the same database. Prefer
/// [`run_isolated_migrations`] which uses a custom tracking table to
/// prevent conflicts between framework migrations, app migrations, and
/// multiple binaries sharing the same database.
#[deprecated(
    since = "0.2.0",
    note = "Use `run_isolated_migrations` with a custom tracking table to avoid migration conflicts"
)]
pub async fn run_user_migrations(pool: &DbPool, dir: &std::path::Path) -> anyhow::Result<()> {
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

/// Run migrations from a directory using an isolated tracking table.
///
/// This prevents conflicts between framework migrations and app migrations,
/// or between multiple app binaries sharing the same database. Each caller
/// specifies its own `tracking_table` name (e.g. `"_myapp_migrations"`,
/// `"_aggregator_migrations"`) and only migrations tracked in that table
/// affect the outcome.
///
/// The function is idempotent: re-running with the same migrations and
/// tracking table is a no-op for already-applied migrations.
///
/// # Arguments
///
/// * `pool` — the database connection pool (SQLite or PostgreSQL).
/// * `dir` — path to the directory containing numbered `.sql` migration files.
/// * `tracking_table` — the name of the table used to record which migrations
///   have been applied. Must be a valid SQL identifier (letters, digits,
///   underscores). The table is created automatically if it does not exist.
///
/// # Errors
///
/// Returns an error if:
/// - The directory does not exist or cannot be read.
/// - A migration file contains invalid SQL.
/// - The database rejects a migration statement.
/// - The `tracking_table` name contains invalid characters.
///
/// # Examples
///
/// ```no_run
/// # async fn example(pool: &atrg_db::DbPool) -> anyhow::Result<()> {
/// use std::path::Path;
///
/// // App migrations — isolated from atrg internals
/// atrg_db::run_isolated_migrations(
///     pool,
///     Path::new("./migrations"),
///     "_myapp_migrations",
/// ).await?;
///
/// // A second binary's migrations in the same DB
/// atrg_db::run_isolated_migrations(
///     pool,
///     Path::new("./aggregator_migrations"),
///     "_aggregator_migrations",
/// ).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run_isolated_migrations(
    pool: &DbPool,
    dir: &std::path::Path,
    tracking_table: &str,
) -> anyhow::Result<()> {
    // Validate the tracking table name to prevent SQL injection.
    if tracking_table.is_empty()
        || !tracking_table
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        anyhow::bail!(
            "invalid tracking table name `{}`; must contain only ASCII alphanumerics and underscores",
            tracking_table
        );
    }

    if !dir.exists() {
        anyhow::bail!("migrations directory does not exist: {}", dir.display());
    }

    let migrator = sqlx::migrate::Migrator::new(dir).await?;
    run_migrator_with_table(pool, &migrator, tracking_table).await?;

    tracing::info!(
        count = migrator.migrations.len(),
        path = %dir.display(),
        table = tracking_table,
        backend = pool.backend(),
        "applied isolated migrations (if pending)"
    );

    Ok(())
}

/// Validate a tracking table name. Returns an error if the name is empty or
/// contains characters outside `[a-zA-Z0-9_]`.
fn validate_table_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        anyhow::bail!(
            "invalid tracking table name `{}`; must contain only ASCII alphanumerics and underscores",
            name
        );
    }
    Ok(())
}

/// Internal: run a `Migrator` using a custom tracking table instead of the
/// default `_sqlx_migrations`. This is the core engine powering both
/// [`run_internal_migrations`] and [`run_isolated_migrations`].
async fn run_migrator_with_table(
    pool: &DbPool,
    migrator: &sqlx::migrate::Migrator,
    tracking_table: &str,
) -> anyhow::Result<()> {
    validate_table_name(tracking_table)?;

    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => run_migrator_sqlite(p, migrator, tracking_table).await,
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => run_migrator_postgres(p, migrator, tracking_table).await,
    }
}

#[cfg(feature = "sqlite")]
async fn run_migrator_sqlite(
    pool: &SqlitePool,
    migrator: &sqlx::migrate::Migrator,
    tracking_table: &str,
) -> anyhow::Result<()> {
    // Create the tracking table if it does not exist.
    let create_sql = format!(
        "CREATE TABLE IF NOT EXISTS \"{}\" (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            checksum BLOB NOT NULL,
            applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        tracking_table
    );
    sqlx::query(&create_sql).execute(pool).await?;

    // Fetch already-applied versions.
    let applied_sql = format!("SELECT version FROM \"{}\"", tracking_table);
    let applied_rows: Vec<i64> = sqlx::query_scalar(&applied_sql).fetch_all(pool).await?;
    let applied: std::collections::HashSet<i64> = applied_rows.into_iter().collect();

    // Apply each pending migration in order.
    for migration in migrator.migrations.iter() {
        let version = migration.version;
        if applied.contains(&version) {
            continue;
        }

        tracing::debug!(
            version = version,
            description = %migration.description,
            table = tracking_table,
            "applying migration (sqlite)"
        );

        // Execute the migration SQL.
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(pool)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to apply migration {}: {} — {}",
                    version,
                    migration.description,
                    e
                )
            })?;

        // Record it in the tracking table.
        let insert_sql = format!(
            "INSERT INTO \"{}\" (version, description, checksum) VALUES (?, ?, ?)",
            tracking_table
        );
        sqlx::query(&insert_sql)
            .bind(version)
            .bind(migration.description.as_ref())
            .bind(migration.checksum.as_ref())
            .execute(pool)
            .await?;
    }

    Ok(())
}

#[cfg(feature = "postgres")]
async fn run_migrator_postgres(
    pool: &PgPool,
    migrator: &sqlx::migrate::Migrator,
    tracking_table: &str,
) -> anyhow::Result<()> {
    // Create the tracking table if it does not exist.
    let create_sql = format!(
        "CREATE TABLE IF NOT EXISTS \"{}\" (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            checksum BYTEA NOT NULL,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
        tracking_table
    );
    sqlx::query(&create_sql).execute(pool).await?;

    // Fetch already-applied versions.
    let applied_sql = format!("SELECT version FROM \"{}\"", tracking_table);
    let applied_rows: Vec<i64> = sqlx::query_scalar(&applied_sql).fetch_all(pool).await?;
    let applied: std::collections::HashSet<i64> = applied_rows.into_iter().collect();

    // Apply each pending migration in order.
    for migration in migrator.migrations.iter() {
        let version = migration.version;
        if applied.contains(&version) {
            continue;
        }

        tracing::debug!(
            version = version,
            description = %migration.description,
            table = tracking_table,
            "applying migration (postgres)"
        );

        // Execute the migration SQL.
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(pool)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to apply migration {}: {} — {}",
                    version,
                    migration.description,
                    e
                )
            })?;

        // Record it in the tracking table.
        let insert_sql = format!(
            "INSERT INTO \"{}\" (version, description, checksum) VALUES ($1, $2, $3)",
            tracking_table
        );
        sqlx::query(&insert_sql)
            .bind(version)
            .bind(migration.description.as_ref())
            .bind(migration.checksum.as_ref())
            .execute(pool)
            .await?;
    }

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
    #[allow(deprecated)]
    async fn test_user_migrations_empty_dir() {
        let pool = connect("sqlite::memory:").await.expect("connect");
        let tmp_dir = std::env::temp_dir().join(format!("atrg_test_empty_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).expect("mkdir");

        let result = run_user_migrations(&pool, &tmp_dir).await;
        let _ = std::fs::remove_dir_all(&tmp_dir);
        result.expect("empty dir succeeds silently");
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn test_user_migrations_nonexistent_dir() {
        let pool = connect("sqlite::memory:").await.expect("connect");
        let nonexistent =
            std::path::Path::new("/tmp/atrg_test_nonexistent_dir_that_does_not_exist");
        run_user_migrations(&pool, nonexistent)
            .await
            .expect("nonexistent dir succeeds silently");
    }

    #[tokio::test]
    async fn unsupported_scheme_errors() {
        let err = connect("mysql://localhost/db").await.unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("unsupported database URL scheme"),
            "got: {msg}"
        );
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

    // ── Isolated migration tests ──────────────────────────────────────────

    /// Helper: create a temp directory with a numbered SQL migration file.
    fn write_migration(dir: &std::path::Path, filename: &str, sql: &str) {
        std::fs::create_dir_all(dir).expect("create migration dir");
        std::fs::write(dir.join(filename), sql).expect("write migration file");
    }

    #[tokio::test]
    async fn test_isolated_migrations_two_sets_coexist() {
        // Two different migration sets with different tracking tables
        // can coexist in the same database without conflict.
        let pool = connect("sqlite::memory:").await.expect("connect");
        let sqlite = pool.as_sqlite().expect("sqlite pool");

        let base =
            std::env::temp_dir().join(format!("atrg_test_isolated_coexist_{}", std::process::id()));
        let dir_a = base.join("migrations_a");
        let dir_b = base.join("migrations_b");

        // Migration set A: creates a "posts" table
        write_migration(
            &dir_a,
            "20230101000000_create_posts.sql",
            "CREATE TABLE posts (id INTEGER PRIMARY KEY, body TEXT NOT NULL);",
        );

        // Migration set B: creates a "follows" table
        write_migration(
            &dir_b,
            "20230101000000_create_follows.sql",
            "CREATE TABLE follows (id INTEGER PRIMARY KEY, follower TEXT NOT NULL, followee TEXT NOT NULL);",
        );

        // Run both sets with different tracking tables.
        run_isolated_migrations(&pool, &dir_a, "_app_ring_migrations")
            .await
            .expect("ring migrations");
        run_isolated_migrations(&pool, &dir_b, "_app_aggregator_migrations")
            .await
            .expect("aggregator migrations");

        // Both tables should exist.
        let posts: (String,) =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table' AND name='posts'")
                .fetch_one(sqlite)
                .await
                .expect("posts table exists");
        assert_eq!(posts.0, "posts");

        let follows: (String,) =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table' AND name='follows'")
                .fetch_one(sqlite)
                .await
                .expect("follows table exists");
        assert_eq!(follows.0, "follows");

        // Both tracking tables should exist and contain exactly one entry each.
        let ring_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _app_ring_migrations")
            .fetch_one(sqlite)
            .await
            .expect("ring tracking table");
        assert_eq!(ring_count.0, 1);

        let agg_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _app_aggregator_migrations")
            .fetch_one(sqlite)
            .await
            .expect("aggregator tracking table");
        assert_eq!(agg_count.0, 1);

        // No `_sqlx_migrations` table should exist — we bypassed sqlx's default.
        let sqlx_table: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations'",
        )
        .fetch_optional(sqlite)
        .await
        .expect("query");
        assert!(
            sqlx_table.is_none(),
            "_sqlx_migrations should NOT exist when using isolated migrations"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn test_isolated_migrations_idempotent() {
        // Re-running the same migrations with the same tracking table is a no-op.
        let pool = connect("sqlite::memory:").await.expect("connect");
        let sqlite = pool.as_sqlite().expect("sqlite pool");

        let dir = std::env::temp_dir().join(format!(
            "atrg_test_isolated_idempotent_{}",
            std::process::id()
        ));
        write_migration(
            &dir,
            "20230601000000_create_items.sql",
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
        );

        // First run.
        run_isolated_migrations(&pool, &dir, "_test_idempotent")
            .await
            .expect("first run");

        // Second run — should not fail (CREATE TABLE would fail if re-executed).
        run_isolated_migrations(&pool, &dir, "_test_idempotent")
            .await
            .expect("second run (idempotent)");

        // Only one tracking row.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _test_idempotent")
            .fetch_one(sqlite)
            .await
            .expect("count");
        assert_eq!(count.0, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_isolated_migrations_multiple_files_ordered() {
        // Multiple migration files are applied in version order.
        let pool = connect("sqlite::memory:").await.expect("connect");
        let sqlite = pool.as_sqlite().expect("sqlite pool");

        let dir =
            std::env::temp_dir().join(format!("atrg_test_isolated_ordered_{}", std::process::id()));
        write_migration(
            &dir,
            "20230101000000_create_alpha.sql",
            "CREATE TABLE alpha (id INTEGER PRIMARY KEY);",
        );
        write_migration(
            &dir,
            "20230102000000_create_beta.sql",
            "CREATE TABLE beta (id INTEGER PRIMARY KEY, alpha_id INTEGER REFERENCES alpha(id));",
        );

        run_isolated_migrations(&pool, &dir, "_test_ordered")
            .await
            .expect("ordered migrations");

        // Both tables created.
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('alpha', 'beta')",
        )
        .fetch_one(sqlite)
        .await
        .expect("count tables");
        assert_eq!(count.0, 2);

        // Tracking table has 2 rows.
        let track_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _test_ordered")
            .fetch_one(sqlite)
            .await
            .expect("tracking count");
        assert_eq!(track_count.0, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_isolated_migrations_does_not_conflict_with_internal() {
        // Internal (atrg) migrations and isolated app migrations coexist.
        let pool = connect("sqlite::memory:").await.expect("connect");
        let sqlite = pool.as_sqlite().expect("sqlite pool");

        // Run atrg's internal migrations first.
        run_internal_migrations(&pool)
            .await
            .expect("internal migrations");

        // Now run app migrations with a separate tracking table.
        let dir =
            std::env::temp_dir().join(format!("atrg_test_no_conflict_{}", std::process::id()));
        write_migration(
            &dir,
            "20230101000000_create_widgets.sql",
            "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
        );

        run_isolated_migrations(&pool, &dir, "_myapp_migrations")
            .await
            .expect("app migrations");

        // atrg_sessions (from internal) and widgets (from app) both exist.
        let sessions: (String,) = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='atrg_sessions'",
        )
        .fetch_one(sqlite)
        .await
        .expect("atrg_sessions");
        assert_eq!(sessions.0, "atrg_sessions");

        let widgets: (String,) =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table' AND name='widgets'")
                .fetch_one(sqlite)
                .await
                .expect("widgets");
        assert_eq!(widgets.0, "widgets");

        // Tracking tables are separate.
        let atrg_tracking: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _atrg_migrations")
            .fetch_one(sqlite)
            .await
            .expect("atrg tracking");
        assert!(atrg_tracking.0 >= 1);

        let app_tracking: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _myapp_migrations")
            .fetch_one(sqlite)
            .await
            .expect("app tracking");
        assert_eq!(app_tracking.0, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_isolated_migrations_invalid_table_name() {
        let pool = connect("sqlite::memory:").await.expect("connect");

        let dir =
            std::env::temp_dir().join(format!("atrg_test_invalid_name_{}", std::process::id()));
        write_migration(&dir, "20230101000000_noop.sql", "SELECT 1;");

        // Empty name.
        let err = run_isolated_migrations(&pool, &dir, "").await.unwrap_err();
        assert!(
            format!("{err}").contains("invalid tracking table name"),
            "got: {err}"
        );

        // Name with SQL injection attempt.
        let err = run_isolated_migrations(&pool, &dir, "foo; DROP TABLE--")
            .await
            .unwrap_err();
        assert!(
            format!("{err}").contains("invalid tracking table name"),
            "got: {err}"
        );

        // Name with spaces.
        let err = run_isolated_migrations(&pool, &dir, "has spaces")
            .await
            .unwrap_err();
        assert!(
            format!("{err}").contains("invalid tracking table name"),
            "got: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_isolated_migrations_nonexistent_dir_errors() {
        let pool = connect("sqlite::memory:").await.expect("connect");
        let nonexistent = std::path::Path::new("/tmp/atrg_test_isolated_nonexistent_dir_xyzzy");
        let err = run_isolated_migrations(&pool, nonexistent, "_test")
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("does not exist"), "got: {err}");
    }

    #[tokio::test]
    async fn test_internal_migrations_use_atrg_tracking_table() {
        // Verify that run_internal_migrations uses `_atrg_migrations`
        // (not `_sqlx_migrations`).
        let pool = connect("sqlite::memory:").await.expect("connect");
        let sqlite = pool.as_sqlite().expect("sqlite pool");

        run_internal_migrations(&pool)
            .await
            .expect("internal migrations");

        // _atrg_migrations should exist.
        let tracking: (String,) = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='_atrg_migrations'",
        )
        .fetch_one(sqlite)
        .await
        .expect("_atrg_migrations exists");
        assert_eq!(tracking.0, "_atrg_migrations");

        // _sqlx_migrations should NOT exist.
        let sqlx_table: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations'",
        )
        .fetch_optional(sqlite)
        .await
        .expect("query");
        assert!(
            sqlx_table.is_none(),
            "_sqlx_migrations should NOT exist; internal migrations must use _atrg_migrations"
        );
    }
}
