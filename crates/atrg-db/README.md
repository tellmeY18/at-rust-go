# atrg-db

**Database layer for at-rust-go: SQLite and PostgreSQL connection pooling and migrations.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

<!-- Uncomment when published:
[![crates.io](https://img.shields.io/crates/v/atrg-db.svg)](https://crates.io/crates/atrg-db)
[![docs.rs](https://docs.rs/atrg-db/badge.svg)](https://docs.rs/atrg-db)
-->

## What this crate provides

- **`DbPool`** — enum wrapping either `sqlx::SqlitePool` or `sqlx::PgPool`, the primary database handle used throughout atrg
- **`connect(url)`** — create a connection pool, dispatching by URL scheme (`sqlite://`, `sqlite::memory:`, `postgres://`, `postgresql://`)
- **`run_internal_migrations(pool)`** — apply atrg's own embedded migrations (sessions table, etc.) using the dialect matching the pool variant; idempotent on every startup
- **`run_user_migrations(pool, dir)`** — discover and apply application-specific `.sql` migrations from a directory, silently skipping if the directory is missing or empty

The crate does **not** provide an ORM. Write SQL with `sqlx`.

## Cargo features

| Feature    | Default | Effect                                                                 |
|------------|:-------:|------------------------------------------------------------------------|
| `sqlite`   | ✅       | Pulls in the `sqlx/sqlite` driver and enables `DbPool::Sqlite`.        |
| `postgres` | ❌       | Pulls in the `sqlx/postgres` driver and enables `DbPool::Postgres`.    |

At least one of `sqlite` or `postgres` must be enabled (a `compile_error!` fires otherwise). Enabling both is supported; `connect()` picks the backend at runtime from the URL scheme.

## Usage

```toml
[dependencies]
# SQLite only (default)
atrg-db = "0.1"

# PostgreSQL only
atrg-db = { version = "0.1", default-features = false, features = ["postgres"] }

# Both backends — let `connect()` choose at runtime
atrg-db = { version = "0.1", features = ["postgres"] }
```

```rust
use atrg_db::DbPool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Pick the backend from the URL scheme
    let pool: DbPool = atrg_db::connect("sqlite://atrg.db").await?;
    // or: let pool = atrg_db::connect("postgres://user:pass@host/db").await?;

    // Run atrg internal migrations (sessions, OAuth state, etc.)
    atrg_db::run_internal_migrations(&pool).await?;

    // Run your app's migrations from the migrations/ directory
    atrg_db::run_user_migrations(&pool, std::path::Path::new("./migrations")).await?;

    // Borrow the underlying sqlx pool to run queries
    if let Some(sqlite) = pool.as_sqlite() {
        let row: (i32,) = sqlx::query_as("SELECT 1").fetch_one(sqlite).await?;
        assert_eq!(row.0, 1);
    }

    Ok(())
}
```

## Connection defaults

### SQLite

| Setting              | Value  |
|----------------------|--------|
| Journal mode         | WAL    |
| Foreign keys         | ON     |
| Create if missing    | Yes    |
| Max connections      | 8      |

### PostgreSQL

| Setting              | Value  |
|----------------------|--------|
| Max connections      | 8      |

## Migration conventions

atrg's internal migrations live in this crate under `migrations/sqlite/` and `migrations/postgres/`. The set matching the active `DbPool` variant is run automatically. User migrations live in your project's `migrations/` directory and are applied in filename order against the same backend:

```
migrations/
├── 0001_create_posts.sql
└── 0002_create_follows.sql
```

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).