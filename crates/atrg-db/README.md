# atrg-db

**Database layer for at-rust-go: SQLite connection pooling and migrations.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

<!-- Uncomment when published:
[![crates.io](https://img.shields.io/crates/v/atrg-db.svg)](https://crates.io/crates/atrg-db)
[![docs.rs](https://docs.rs/atrg-db/badge.svg)](https://docs.rs/atrg-db)
-->

## What this crate provides

- **`DbConn`** — type alias for `sqlx::SqlitePool`, the primary database handle used throughout atrg
- **`connect(url)`** — create a connection pool with sensible defaults (WAL journal mode, foreign keys enabled, auto-create database file)
- **`run_internal_migrations(pool)`** — apply atrg's own embedded migrations (sessions table, etc.), idempotent on every startup
- **`run_user_migrations(pool, dir)`** — discover and apply application-specific `.sql` migrations from a directory, silently skipping if the directory is missing or empty

The crate does **not** provide an ORM. Write SQL with `sqlx::query!()` for compile-time checked queries.

## Usage

```toml
[dependencies]
atrg-db = "0.1"
```

```rust
use atrg_db::DbConn;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Connect to SQLite (file is created if missing)
    let pool: DbConn = atrg_db::connect("sqlite://atrg.db").await?;

    // Run atrg internal migrations (sessions, etc.)
    atrg_db::run_internal_migrations(&pool).await?;

    // Run your app's migrations from the migrations/ directory
    atrg_db::run_user_migrations(&pool, std::path::Path::new("./migrations")).await?;

    // Use the pool in your handlers
    let row: (i32,) = sqlx::query_as("SELECT 1")
        .fetch_one(&pool)
        .await?;
    assert_eq!(row.0, 1);

    Ok(())
}
```

## Connection defaults

| Setting              | Value  |
|----------------------|--------|
| Journal mode         | WAL    |
| Foreign keys         | ON     |
| Create if missing    | Yes    |
| Max connections      | 8      |

## Migration conventions

atrg's internal migrations run first (prefixed `atrg_`). User migrations live in your project's `migrations/` directory and are applied in filename order:

```
migrations/
├── 0001_create_posts.sql
└── 0002_create_follows.sql
```

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).