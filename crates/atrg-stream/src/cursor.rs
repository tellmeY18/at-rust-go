//! Cursor persistence for Jetstream consumer.
//!
//! Stores the last processed event timestamp (`time_us`) so the consumer
//! can resume from where it left off after restart.

use atrg_db::DbPool;

/// SQL for creating the cursor table (SQLite).
pub const CREATE_CURSOR_TABLE_SQLITE: &str = r#"
CREATE TABLE IF NOT EXISTS atrg_jetstream_cursors (
    consumer_id TEXT PRIMARY KEY,
    time_us INTEGER NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
"#;

/// SQL for creating the cursor table (PostgreSQL).
pub const CREATE_CURSOR_TABLE_POSTGRES: &str = r#"
CREATE TABLE IF NOT EXISTS atrg_jetstream_cursors (
    consumer_id TEXT PRIMARY KEY,
    time_us BIGINT NOT NULL,
    updated_at BIGINT NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::bigint
);
"#;

/// Load the last stored cursor for a consumer.
/// Returns `None` if no cursor has been stored yet.
pub async fn load_cursor(pool: &DbPool, consumer_id: &str) -> anyhow::Result<Option<i64>> {
    let result: Option<i64> = match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query_scalar("SELECT time_us FROM atrg_jetstream_cursors WHERE consumer_id = ?1")
                .bind(consumer_id)
                .fetch_optional(p)
                .await?
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query_scalar("SELECT time_us FROM atrg_jetstream_cursors WHERE consumer_id = $1")
                .bind(consumer_id)
                .fetch_optional(p)
                .await?
        }
    };
    Ok(result)
}

/// Save the cursor position for a consumer.
pub async fn save_cursor(pool: &DbPool, consumer_id: &str, time_us: i64) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query(
                "INSERT INTO atrg_jetstream_cursors (consumer_id, time_us, updated_at) \
                 VALUES (?1, ?2, ?3) \
                 ON CONFLICT(consumer_id) DO UPDATE SET time_us = ?2, updated_at = ?3",
            )
            .bind(consumer_id)
            .bind(time_us)
            .bind(now)
            .execute(p)
            .await?;
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query(
                "INSERT INTO atrg_jetstream_cursors (consumer_id, time_us, updated_at) \
                 VALUES ($1, $2, $3) \
                 ON CONFLICT(consumer_id) DO UPDATE SET time_us = $2, updated_at = $3",
            )
            .bind(consumer_id)
            .bind(time_us)
            .bind(now)
            .execute(p)
            .await?;
        }
    }
    Ok(())
}

/// Ensure the cursor table exists for the active backend.
///
/// This should be called once during app startup (e.g. inside
/// `AtrgApp::run()`) before spawning the Jetstream consumer.
pub async fn ensure_cursor_table(pool: &DbPool) -> anyhow::Result<()> {
    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query(CREATE_CURSOR_TABLE_SQLITE).execute(p).await?;
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query(CREATE_CURSOR_TABLE_POSTGRES).execute(p).await?;
        }
    }
    tracing::debug!("ensured atrg_jetstream_cursors table exists");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_cursor_roundtrip() {
        let pool = atrg_db::connect("sqlite::memory:").await.unwrap();
        // Create table
        if let DbPool::Sqlite(p) = &pool {
            sqlx::query(CREATE_CURSOR_TABLE_SQLITE)
                .execute(p)
                .await
                .unwrap();
        }

        // Initially no cursor
        let cursor = load_cursor(&pool, "test-consumer").await.unwrap();
        assert_eq!(cursor, None);

        // Save cursor
        save_cursor(&pool, "test-consumer", 1234567890)
            .await
            .unwrap();

        // Load it back
        let cursor = load_cursor(&pool, "test-consumer").await.unwrap();
        assert_eq!(cursor, Some(1234567890));

        // Update cursor
        save_cursor(&pool, "test-consumer", 9999999999)
            .await
            .unwrap();
        let cursor = load_cursor(&pool, "test-consumer").await.unwrap();
        assert_eq!(cursor, Some(9999999999));
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_multiple_consumers() {
        let pool = atrg_db::connect("sqlite::memory:").await.unwrap();
        if let DbPool::Sqlite(p) = &pool {
            sqlx::query(CREATE_CURSOR_TABLE_SQLITE)
                .execute(p)
                .await
                .unwrap();
        }

        save_cursor(&pool, "consumer-a", 100).await.unwrap();
        save_cursor(&pool, "consumer-b", 200).await.unwrap();

        assert_eq!(load_cursor(&pool, "consumer-a").await.unwrap(), Some(100));
        assert_eq!(load_cursor(&pool, "consumer-b").await.unwrap(), Some(200));
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_ensure_cursor_table_idempotent() {
        let pool = atrg_db::connect("sqlite::memory:").await.unwrap();

        // Call twice — should not error on the second call.
        ensure_cursor_table(&pool).await.unwrap();
        ensure_cursor_table(&pool).await.unwrap();

        // Table should be usable.
        save_cursor(&pool, "idempotent-test", 42).await.unwrap();
        assert_eq!(
            load_cursor(&pool, "idempotent-test").await.unwrap(),
            Some(42)
        );
    }
}
