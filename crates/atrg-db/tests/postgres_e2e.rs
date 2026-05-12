//! End-to-end tests for atrg against a real PostgreSQL instance.
//!
//! These tests are gated behind the `postgres` feature flag and require
//! a running PostgreSQL server. Set `TEST_DATABASE_URL` to a valid Postgres
//! connection string.
//!
//! Run locally:
//!   TEST_DATABASE_URL="postgres://atrg_test@localhost/atrg_test" \
//!     cargo test --test postgres_e2e --features postgres -- --test-threads=1
//!
//! In CI, a Postgres service container is spun up automatically.

#![cfg(feature = "postgres")]

use std::sync::Arc;

// ─── Helpers ──────────────────────────────────────────────────────────

/// Returns the Postgres connection URL, or `None` if `TEST_DATABASE_URL` is
/// not set — in which case every test should silently return `Ok(())`.
fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

/// Macro that skips (returns Ok) when Postgres is unavailable.
macro_rules! require_pg {
    () => {
        if test_db_url().is_none() {
            eprintln!("  skipping (TEST_DATABASE_URL not set)");
            return;
        }
    };
}

/// Create a unique test database by appending a random suffix.
/// Returns `(connection_url_for_new_db, db_name)`.
async fn create_test_db() -> (String, String) {
    let base_url = test_db_url().expect("TEST_DATABASE_URL required — call require_pg!() first");
    let db_name = format!("atrg_e2e_{}", rand::random::<u32>());

    // Connect to the base URL (usually pointing to 'postgres' or 'atrg_test' db)
    let pool = sqlx::PgPool::connect(&base_url)
        .await
        .expect("connect to base Postgres");

    // Create a fresh database for this test run
    sqlx::query(&format!("CREATE DATABASE \"{}\"", db_name))
        .execute(&pool)
        .await
        .expect("create test database");

    pool.close().await;

    // Build the URL for the new database by replacing the last path segment.
    let test_url = if base_url.ends_with('/') {
        format!("{}{}", base_url, db_name)
    } else if base_url.contains('/') {
        let (prefix, _) = base_url.rsplit_once('/').unwrap();
        format!("{}/{}", prefix, db_name)
    } else {
        format!("{}/{}", base_url, db_name)
    };

    (test_url, db_name)
}

/// Drop a test database after the test completes.
async fn drop_test_db(db_name: &str) {
    let Some(base_url) = test_db_url() else {
        return;
    };
    if let Ok(pool) = sqlx::PgPool::connect(&base_url).await {
        // Force disconnect other clients
        let _ = sqlx::query(&format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{}' AND pid <> pg_backend_pid()",
            db_name
        ))
        .execute(&pool)
        .await;

        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{}\"", db_name))
            .execute(&pool)
            .await;
        pool.close().await;
    }
}

// ─── §7.1 Migration Namespace Isolation ──────────────────────────────

mod migration_isolation {
    use super::*;

    #[tokio::test]
    async fn internal_migrations_use_atrg_tracking_table() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");

        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("internal migrations");

        let pg = pool.as_postgres().unwrap();

        // _atrg_migrations should exist
        let has_atrg: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                SELECT 1 FROM information_schema.tables \
                WHERE table_name = '_atrg_migrations'\
            )",
        )
        .fetch_one(pg)
        .await
        .unwrap();
        assert!(has_atrg, "_atrg_migrations table should exist");

        // _sqlx_migrations should NOT exist (we use our own tracking)
        let has_sqlx: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                SELECT 1 FROM information_schema.tables \
                WHERE table_name = '_sqlx_migrations'\
            )",
        )
        .fetch_one(pg)
        .await
        .unwrap();
        assert!(
            !has_sqlx,
            "_sqlx_migrations should NOT exist after internal migrations"
        );

        // atrg_sessions table should exist
        let has_sessions: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                SELECT 1 FROM information_schema.tables \
                WHERE table_name = 'atrg_sessions'\
            )",
        )
        .fetch_one(pg)
        .await
        .unwrap();
        assert!(has_sessions, "atrg_sessions table should be created");

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn isolated_migrations_coexist_without_conflict() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");

        // Run framework migrations
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("internal");

        // Create two separate app migration directories
        let dir_a = std::env::temp_dir().join(format!("pg_mig_a_{}", rand::random::<u32>()));
        let dir_b = std::env::temp_dir().join(format!("pg_mig_b_{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();

        std::fs::write(
            dir_a.join("0001_create_posts.sql"),
            "CREATE TABLE IF NOT EXISTS posts (\
                id BIGSERIAL PRIMARY KEY, \
                title TEXT NOT NULL\
            );",
        )
        .unwrap();

        std::fs::write(
            dir_b.join("0001_create_feeds.sql"),
            "CREATE TABLE IF NOT EXISTS feeds (\
                id BIGSERIAL PRIMARY KEY, \
                name TEXT NOT NULL\
            );",
        )
        .unwrap();

        // Run both sets with separate tracking tables
        atrg_db::run_isolated_migrations(&pool, &dir_a, "_app_a_migrations")
            .await
            .expect("app A migrations");
        atrg_db::run_isolated_migrations(&pool, &dir_b, "_app_b_migrations")
            .await
            .expect("app B migrations");

        let pg = pool.as_postgres().unwrap();

        // Both application tables should exist
        let has_posts: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                SELECT 1 FROM information_schema.tables WHERE table_name = 'posts'\
            )",
        )
        .fetch_one(pg)
        .await
        .unwrap();
        assert!(has_posts);

        let has_feeds: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                SELECT 1 FROM information_schema.tables WHERE table_name = 'feeds'\
            )",
        )
        .fetch_one(pg)
        .await
        .unwrap();
        assert!(has_feeds);

        // Both tracking tables should exist independently
        let has_a: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                SELECT 1 FROM information_schema.tables WHERE table_name = '_app_a_migrations'\
            )",
        )
        .fetch_one(pg)
        .await
        .unwrap();
        assert!(has_a);

        let has_b: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                SELECT 1 FROM information_schema.tables WHERE table_name = '_app_b_migrations'\
            )",
        )
        .fetch_one(pg)
        .await
        .unwrap();
        assert!(has_b);

        // Re-running should be idempotent
        atrg_db::run_isolated_migrations(&pool, &dir_a, "_app_a_migrations")
            .await
            .expect("idempotent re-run");

        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
        drop_test_db(&db_name).await;
    }
}

// ─── §7.2 Postgres Connection & Pool ─────────────────────────────────

mod postgres_pool {
    use super::*;

    #[tokio::test]
    async fn connect_and_ping() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect to Postgres");

        assert_eq!(pool.backend(), "postgres");
        pool.ping().await.expect("ping should succeed");
        assert!(pool.as_postgres().is_some());

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn internal_migrations_create_all_tables() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("migrations");

        let pg = pool.as_postgres().unwrap();

        // Verify all expected tables
        for table in &["atrg_sessions", "atrg_oauth_states"] {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(\
                    SELECT 1 FROM information_schema.tables WHERE table_name = $1\
                )",
            )
            .bind(table)
            .fetch_one(pg)
            .await
            .unwrap();
            assert!(exists, "table {} should exist", table);
        }

        pool.close().await;
        drop_test_db(&db_name).await;
    }
}

// ─── §8.1 API Key Auth ──────────────────────────────────────────────

mod api_key_auth {
    use super::*;

    async fn setup_pool() -> (atrg_db::DbPool, String) {
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("migrations");

        // Create api_keys table
        let pg = pool.as_postgres().unwrap();
        sqlx::raw_sql(atrg_auth::api_keys::CREATE_API_KEYS_TABLE_POSTGRES)
            .execute(pg)
            .await
            .expect("create api_keys table");

        (pool, db_name)
    }

    #[tokio::test]
    async fn create_find_revoke_cycle() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        // Create a key
        let (full_key, api_key) = atrg_auth::api_keys::create_api_key(
            &pool,
            "did:plc:testuser",
            "Test Key",
            &["admin:*".to_string()],
            "atrg_",
        )
        .await
        .expect("create key");

        assert!(full_key.starts_with("atrg_"));
        assert_eq!(api_key.did, "did:plc:testuser");
        assert_eq!(api_key.name, "Test Key");

        // Find by full key
        let found = atrg_auth::api_keys::find_by_key(&pool, &full_key)
            .await
            .expect("find")
            .expect("key should exist");
        assert_eq!(found.did, "did:plc:testuser");

        // List keys
        let keys = atrg_auth::api_keys::list_api_keys(&pool, Some("did:plc:testuser"))
            .await
            .expect("list");
        assert_eq!(keys.len(), 1);

        // Revoke
        let revoked = atrg_auth::api_keys::revoke_api_key(&pool, &api_key.key_prefix)
            .await
            .expect("revoke");
        assert!(revoked);

        // Should no longer be findable
        let gone = atrg_auth::api_keys::find_by_key(&pool, &full_key)
            .await
            .expect("find after revoke");
        assert!(gone.is_none());

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn list_keys_filtered_by_did() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        // Create keys for two different DIDs
        atrg_auth::api_keys::create_api_key(
            &pool,
            "did:plc:alice",
            "Alice Key",
            &["read".to_string()],
            "atrg_",
        )
        .await
        .unwrap();

        atrg_auth::api_keys::create_api_key(
            &pool,
            "did:plc:bob",
            "Bob Key",
            &["write".to_string()],
            "atrg_",
        )
        .await
        .unwrap();

        // List all keys
        let all = atrg_auth::api_keys::list_api_keys(&pool, None)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);

        // List only Alice's keys
        let alice_keys = atrg_auth::api_keys::list_api_keys(&pool, Some("did:plc:alice"))
            .await
            .unwrap();
        assert_eq!(alice_keys.len(), 1);
        assert_eq!(alice_keys[0].did, "did:plc:alice");

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn revoke_nonexistent_key_returns_false() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let result = atrg_auth::api_keys::revoke_api_key(&pool, "atrg_nonexist")
            .await
            .unwrap();
        assert!(!result);

        pool.close().await;
        drop_test_db(&db_name).await;
    }
}

// ─── §8.2 RBAC ───────────────────────────────────────────────────────

mod rbac {
    use super::*;

    async fn setup_pool() -> (atrg_db::DbPool, String) {
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("migrations");

        let pg = pool.as_postgres().unwrap();
        sqlx::raw_sql(atrg_auth::rbac::CREATE_ROLES_TABLE_POSTGRES)
            .execute(pg)
            .await
            .expect("create roles table");
        sqlx::raw_sql(atrg_auth::rbac::CREATE_BANS_TABLE_POSTGRES)
            .execute(pg)
            .await
            .expect("create bans table");

        (pool, db_name)
    }

    #[tokio::test]
    async fn grant_check_revoke_role() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let did = "did:plc:alice";

        // Initially no role
        assert!(!atrg_auth::rbac::has_role(&pool, did, "admin", None)
            .await
            .unwrap());

        // Grant admin role
        atrg_auth::rbac::grant_role(&pool, did, "admin", None, None, Some("system"))
            .await
            .unwrap();
        assert!(atrg_auth::rbac::has_role(&pool, did, "admin", None)
            .await
            .unwrap());

        // Revoke
        assert!(atrg_auth::rbac::revoke_role(&pool, did, "admin", None)
            .await
            .unwrap());
        assert!(!atrg_auth::rbac::has_role(&pool, did, "admin", None)
            .await
            .unwrap());

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn scoped_role_isolation() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let did = "did:plc:bob";

        // Grant classRep for course A only
        atrg_auth::rbac::grant_role(&pool, did, "classRep", Some("course"), Some("cs301"), None)
            .await
            .unwrap();

        // Has role for course A
        assert!(
            atrg_auth::rbac::has_role(&pool, did, "classRep", Some("cs301"))
                .await
                .unwrap()
        );

        // Does NOT have role for course B
        assert!(
            !atrg_auth::rbac::has_role(&pool, did, "classRep", Some("cs302"))
                .await
                .unwrap()
        );

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn instance_wide_role_matches_any_scope() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let did = "did:plc:superadmin";

        // Grant instance-wide admin (no scope)
        atrg_auth::rbac::grant_role(&pool, did, "admin", None, None, Some("system:bootstrap"))
            .await
            .unwrap();

        // Instance-wide admin should match when checked with any scope_id
        assert!(
            atrg_auth::rbac::has_role(&pool, did, "admin", Some("cs999"))
                .await
                .unwrap()
        );
        assert!(atrg_auth::rbac::has_role(&pool, did, "admin", None)
            .await
            .unwrap());

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn admin_bootstrap() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let admin_dids = vec!["did:plc:admin1".to_string(), "did:plc:admin2".to_string()];
        atrg_auth::rbac::bootstrap_admins(&pool, &admin_dids)
            .await
            .unwrap();

        assert!(
            atrg_auth::rbac::has_role(&pool, "did:plc:admin1", "admin", None)
                .await
                .unwrap()
        );
        assert!(
            atrg_auth::rbac::has_role(&pool, "did:plc:admin2", "admin", None)
                .await
                .unwrap()
        );

        // Idempotent re-run
        atrg_auth::rbac::bootstrap_admins(&pool, &admin_dids)
            .await
            .unwrap();

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn grant_role_is_idempotent() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let did = "did:plc:idempotent";

        // Grant twice — should not error
        atrg_auth::rbac::grant_role(&pool, did, "student", None, None, None)
            .await
            .unwrap();
        atrg_auth::rbac::grant_role(&pool, did, "student", None, None, None)
            .await
            .unwrap();

        // Only one role assignment should exist
        assert!(atrg_auth::rbac::has_role(&pool, did, "student", None)
            .await
            .unwrap());

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn ban_with_ttl_expires() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let did = "did:plc:badactor";

        // Ban with 1-second TTL
        atrg_auth::rbac::ban_did(&pool, did, Some("spam"), Some(1), "did:plc:admin")
            .await
            .unwrap();
        assert!(atrg_auth::rbac::is_banned(&pool, did).await.unwrap());

        // Wait for TTL to expire
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        assert!(
            !atrg_auth::rbac::is_banned(&pool, did).await.unwrap(),
            "ban should have expired"
        );

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn permanent_ban_persists() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let did = "did:plc:perma";
        atrg_auth::rbac::ban_did(&pool, did, Some("permanent"), None, "did:plc:admin")
            .await
            .unwrap();
        assert!(atrg_auth::rbac::is_banned(&pool, did).await.unwrap());

        // Lift ban
        assert!(atrg_auth::rbac::lift_ban(&pool, did).await.unwrap());
        assert!(!atrg_auth::rbac::is_banned(&pool, did).await.unwrap());

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn ban_upsert_replaces_existing() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let did = "did:plc:upsertban";

        // Ban permanently
        atrg_auth::rbac::ban_did(&pool, did, Some("first offence"), None, "did:plc:mod1")
            .await
            .unwrap();
        assert!(atrg_auth::rbac::is_banned(&pool, did).await.unwrap());

        // Re-ban with a short TTL (upsert should replace)
        atrg_auth::rbac::ban_did(&pool, did, Some("updated"), Some(1), "did:plc:mod2")
            .await
            .unwrap();

        // Wait for TTL to expire — the updated ban should have the short TTL
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        assert!(
            !atrg_auth::rbac::is_banned(&pool, did).await.unwrap(),
            "upserted ban should have expired"
        );

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn lift_nonexistent_ban_returns_false() {
        require_pg!();
        let (pool, db_name) = setup_pool().await;

        let result = atrg_auth::rbac::lift_ban(&pool, "did:plc:notbanned")
            .await
            .unwrap();
        assert!(!result);

        pool.close().await;
        drop_test_db(&db_name).await;
    }
}

// ─── §8.1+§8.2 Auth Session Integration ──────────────────────────────

mod session_integration {
    use super::*;

    #[tokio::test]
    async fn create_find_delete_session_on_postgres() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("migrations");

        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600;

        // Create session
        atrg_auth::session::create_session(
            &pool,
            "sess_pg_001",
            "did:plc:pguser",
            "pguser.bsky.social",
            "access_token_here",
            Some("refresh_token_here"),
            expires,
        )
        .await
        .expect("create session");

        // Find session
        let found = atrg_auth::session::find_session(&pool, "sess_pg_001")
            .await
            .expect("find")
            .expect("session should exist");
        assert_eq!(found.did, "did:plc:pguser");
        assert_eq!(found.handle, "pguser.bsky.social");

        // Delete session
        atrg_auth::session::delete_session(&pool, "sess_pg_001")
            .await
            .expect("delete");

        let gone = atrg_auth::session::find_session(&pool, "sess_pg_001")
            .await
            .expect("find after delete");
        assert!(gone.is_none());

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn expired_session_not_returned() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("migrations");

        // Create a session that expired 1 hour ago
        let expired_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 3600;

        atrg_auth::session::create_session(
            &pool,
            "sess_pg_expired",
            "did:plc:expired",
            "expired.bsky.social",
            "old_token",
            None,
            expired_at,
        )
        .await
        .expect("create expired session");

        // Should not be returned
        let result = atrg_auth::session::find_session(&pool, "sess_pg_expired")
            .await
            .expect("find");
        assert!(result.is_none(), "expired session should not be returned");

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn session_without_refresh_token() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("migrations");

        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600;

        // Create session without refresh token
        atrg_auth::session::create_session(
            &pool,
            "sess_pg_norefresh",
            "did:plc:norefresh",
            "norefresh.bsky.social",
            "access_only",
            None,
            expires,
        )
        .await
        .expect("create session without refresh");

        let found = atrg_auth::session::find_session(&pool, "sess_pg_norefresh")
            .await
            .expect("find")
            .expect("should exist");
        assert_eq!(found.did, "did:plc:norefresh");
        assert!(found.refresh_token.is_none());

        pool.close().await;
        drop_test_db(&db_name).await;
    }
}

// ─── §9.3 Cursor Persistence ─────────────────────────────────────────

mod cursor_persistence {
    use super::*;

    #[tokio::test]
    async fn cursor_roundtrip_on_postgres() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");

        // Ensure table exists
        atrg_stream::cursor::ensure_cursor_table(&pool)
            .await
            .expect("ensure table");

        // No cursor initially
        let cursor = atrg_stream::cursor::load_cursor(&pool, "test-consumer")
            .await
            .expect("load");
        assert_eq!(cursor, None);

        // Save and load
        atrg_stream::cursor::save_cursor(&pool, "test-consumer", 1700000000000000)
            .await
            .expect("save");

        let cursor = atrg_stream::cursor::load_cursor(&pool, "test-consumer")
            .await
            .expect("load");
        assert_eq!(cursor, Some(1700000000000000));

        // Update
        atrg_stream::cursor::save_cursor(&pool, "test-consumer", 1700000099999999)
            .await
            .expect("update");
        let cursor = atrg_stream::cursor::load_cursor(&pool, "test-consumer")
            .await
            .expect("load");
        assert_eq!(cursor, Some(1700000099999999));

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn multiple_consumers_independent() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");

        atrg_stream::cursor::ensure_cursor_table(&pool)
            .await
            .expect("ensure table");

        atrg_stream::cursor::save_cursor(&pool, "consumer-a", 100)
            .await
            .unwrap();
        atrg_stream::cursor::save_cursor(&pool, "consumer-b", 200)
            .await
            .unwrap();

        assert_eq!(
            atrg_stream::cursor::load_cursor(&pool, "consumer-a")
                .await
                .unwrap(),
            Some(100)
        );
        assert_eq!(
            atrg_stream::cursor::load_cursor(&pool, "consumer-b")
                .await
                .unwrap(),
            Some(200)
        );

        // Updating one doesn't affect the other
        atrg_stream::cursor::save_cursor(&pool, "consumer-a", 999)
            .await
            .unwrap();
        assert_eq!(
            atrg_stream::cursor::load_cursor(&pool, "consumer-b")
                .await
                .unwrap(),
            Some(200)
        );

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn ensure_cursor_table_idempotent() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");

        // Call twice — should not error on the second call
        atrg_stream::cursor::ensure_cursor_table(&pool)
            .await
            .unwrap();
        atrg_stream::cursor::ensure_cursor_table(&pool)
            .await
            .unwrap();

        // Table should be usable
        atrg_stream::cursor::save_cursor(&pool, "idempotent-test", 42)
            .await
            .unwrap();
        assert_eq!(
            atrg_stream::cursor::load_cursor(&pool, "idempotent-test")
                .await
                .unwrap(),
            Some(42)
        );

        pool.close().await;
        drop_test_db(&db_name).await;
    }
}

// ─── §11.2 Email/OTP on Postgres ─────────────────────────────────────

mod email_otp {
    use super::*;

    #[tokio::test]
    async fn otp_send_verify_cycle_on_postgres() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");

        let pg = pool.as_postgres().unwrap();
        sqlx::raw_sql(atrg_email::CREATE_OTP_TABLE_POSTGRES)
            .execute(pg)
            .await
            .expect("create otp table");

        // Send OTP in dev mode (no SMTP — logs to stdout)
        atrg_email::send_otp(&pool, None, "did:plc:student", "student@uni.edu")
            .await
            .expect("send otp");

        // Insert a known OTP for verification testing
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 600;

        sqlx::query(
            "INSERT INTO atrg_otp_codes (did, email, code, expires_at) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind("did:plc:verify")
        .bind("verify@uni.edu")
        .bind("654321")
        .bind(expires)
        .execute(pg)
        .await
        .expect("insert known OTP");

        // Verify correct code
        assert!(
            atrg_email::verify_otp(&pool, "did:plc:verify", "verify@uni.edu", "654321")
                .await
                .unwrap()
        );

        // Used codes can't be verified again
        assert!(
            !atrg_email::verify_otp(&pool, "did:plc:verify", "verify@uni.edu", "654321")
                .await
                .unwrap()
        );

        // Wrong code fails
        assert!(
            !atrg_email::verify_otp(&pool, "did:plc:verify", "verify@uni.edu", "000000")
                .await
                .unwrap()
        );

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn expired_otp_not_verifiable() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");

        let pg = pool.as_postgres().unwrap();
        sqlx::raw_sql(atrg_email::CREATE_OTP_TABLE_POSTGRES)
            .execute(pg)
            .await
            .expect("create otp table");

        // Insert an already-expired OTP
        let expired_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 60;

        sqlx::query(
            "INSERT INTO atrg_otp_codes (did, email, code, expires_at) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind("did:plc:expiredotp")
        .bind("expired@uni.edu")
        .bind("111111")
        .bind(expired_at)
        .execute(pg)
        .await
        .unwrap();

        assert!(
            !atrg_email::verify_otp(&pool, "did:plc:expiredotp", "expired@uni.edu", "111111")
                .await
                .unwrap(),
            "expired OTP should not be verifiable"
        );

        pool.close().await;
        drop_test_db(&db_name).await;
    }
}

// ─── §7.3 AppState Extensions ────────────────────────────────────────

mod app_state_extensions {
    use super::*;
    use atrg_core::Extensions;

    struct TestBlobStore {
        bucket: String,
    }

    struct TestSmtpConfig {
        host: String,
    }

    #[tokio::test]
    async fn extensions_work_with_postgres_pool() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("migrations");

        let http = reqwest::Client::new();
        let identity = Arc::new(atrg_identity::IdentityResolver::with_defaults(http.clone()));

        let mut extensions = Extensions::new();
        extensions.insert(TestBlobStore {
            bucket: "test-bucket".into(),
        });
        extensions.insert(TestSmtpConfig {
            host: "smtp.test.com".into(),
        });

        let config = Arc::new(atrg_core::config::Config {
            app: atrg_core::config::AppConfig {
                name: "e2e-test".into(),
                host: "127.0.0.1".into(),
                port: 3000,
                secret_key: "test-secret-key-at-least-32-chars!".into(),
                cors_origins: vec![],
                environment: "development".into(),
                admin_dids: vec![],
            },
            auth: Default::default(),
            database: atrg_core::config::DatabaseConfig { url: url.clone() },
            jetstream: None,
            firehose: None,
            feed_generator: None,
            labeler: None,
            rate_limit: None,
        });

        let state = atrg_core::AppState {
            config,
            db: pool.clone(),
            http,
            identity,
            extensions: Arc::new(extensions),
        };

        // Access extensions
        let blobs = state.extension::<TestBlobStore>();
        assert_eq!(blobs.bucket, "test-bucket");

        let smtp = state.extension::<TestSmtpConfig>();
        assert_eq!(smtp.host, "smtp.test.com");

        // try_extension for missing type
        assert!(state.try_extension::<String>().is_none());

        pool.close().await;
        drop_test_db(&db_name).await;
    }
}

// ─── Full Stack Integration ──────────────────────────────────────────

mod full_stack {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn full_postgres_server_roundtrip() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("migrations");

        // Create RBAC tables
        let pg = pool.as_postgres().unwrap();
        sqlx::raw_sql(atrg_auth::rbac::CREATE_ROLES_TABLE_POSTGRES)
            .execute(pg)
            .await
            .unwrap();
        sqlx::raw_sql(atrg_auth::rbac::CREATE_BANS_TABLE_POSTGRES)
            .execute(pg)
            .await
            .unwrap();
        sqlx::raw_sql(atrg_auth::api_keys::CREATE_API_KEYS_TABLE_POSTGRES)
            .execute(pg)
            .await
            .unwrap();

        // Bootstrap admin
        atrg_auth::rbac::bootstrap_admins(&pool, &["did:plc:superadmin".to_string()])
            .await
            .unwrap();

        // Create API key
        let (full_key, _) = atrg_auth::api_keys::create_api_key(
            &pool,
            "did:plc:superadmin",
            "Admin Key",
            &["admin:*".to_string()],
            "atrg_",
        )
        .await
        .unwrap();

        // Verify admin role
        assert!(
            atrg_auth::rbac::has_role(&pool, "did:plc:superadmin", "admin", None)
                .await
                .unwrap()
        );
        assert!(!atrg_auth::rbac::is_banned(&pool, "did:plc:superadmin")
            .await
            .unwrap());

        // Verify API key is findable
        let found = atrg_auth::api_keys::find_by_key(&pool, &full_key)
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().did, "did:plc:superadmin");

        // Build an Axum app and test handlers
        let http = reqwest::Client::new();
        let identity = Arc::new(atrg_identity::IdentityResolver::with_defaults(http.clone()));

        let config = Arc::new(atrg_core::config::Config {
            app: atrg_core::config::AppConfig {
                name: "e2e-full".into(),
                host: "127.0.0.1".into(),
                port: 3000,
                secret_key: "test-secret-key-at-least-32-chars!".into(),
                cors_origins: vec![],
                environment: "development".into(),
                admin_dids: vec![],
            },
            auth: Default::default(),
            database: atrg_core::config::DatabaseConfig { url: url.clone() },
            jetstream: None,
            firehose: None,
            feed_generator: None,
            labeler: None,
            rate_limit: None,
        });

        let state = atrg_core::AppState {
            config,
            db: pool.clone(),
            http,
            identity,
            extensions: Arc::new(atrg_core::Extensions::new()),
        };

        let app = Router::new()
            .route("/healthz", get(atrg_core::health::healthz))
            .route("/readyz", get(atrg_core::health::readyz))
            .with_state(state);

        // Test healthz — always 200
        let req = axum::http::Request::builder()
            .uri("/healthz")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], true);

        // Test readyz — checks DB connectivity and reports backend
        let req = axum::http::Request::builder()
            .uri("/readyz")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["database"], "connected");
        assert_eq!(json["database_backend"], "postgres");

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn combined_rbac_and_session_lifecycle() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("migrations");

        let pg = pool.as_postgres().unwrap();
        sqlx::raw_sql(atrg_auth::rbac::CREATE_ROLES_TABLE_POSTGRES)
            .execute(pg)
            .await
            .unwrap();
        sqlx::raw_sql(atrg_auth::rbac::CREATE_BANS_TABLE_POSTGRES)
            .execute(pg)
            .await
            .unwrap();

        let did = "did:plc:lifecycle";
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600;

        // 1. Create a session (simulating OAuth callback)
        atrg_auth::session::create_session(
            &pool,
            "sess_lifecycle_001",
            did,
            "lifecycle.bsky.social",
            "at_access_token",
            Some("at_refresh_token"),
            expires,
        )
        .await
        .unwrap();

        // 2. Grant role
        atrg_auth::rbac::grant_role(&pool, did, "student", None, None, Some("system:bootstrap"))
            .await
            .unwrap();

        // 3. Verify: session exists, role exists, not banned
        let session = atrg_auth::session::find_session(&pool, "sess_lifecycle_001")
            .await
            .unwrap()
            .expect("session should exist");
        assert_eq!(session.did, did);
        assert!(atrg_auth::rbac::has_role(&pool, did, "student", None)
            .await
            .unwrap());
        assert!(!atrg_auth::rbac::is_banned(&pool, did).await.unwrap());

        // 4. Ban the user
        atrg_auth::rbac::ban_did(&pool, did, Some("policy violation"), None, "did:plc:admin")
            .await
            .unwrap();
        assert!(atrg_auth::rbac::is_banned(&pool, did).await.unwrap());

        // 5. Delete session (simulating forced logout)
        atrg_auth::session::delete_session(&pool, "sess_lifecycle_001")
            .await
            .unwrap();
        assert!(
            atrg_auth::session::find_session(&pool, "sess_lifecycle_001")
                .await
                .unwrap()
                .is_none()
        );

        // 6. Lift ban
        atrg_auth::rbac::lift_ban(&pool, did).await.unwrap();
        assert!(!atrg_auth::rbac::is_banned(&pool, did).await.unwrap());

        // 7. Role should still exist after ban cycle
        assert!(atrg_auth::rbac::has_role(&pool, did, "student", None)
            .await
            .unwrap());

        pool.close().await;
        drop_test_db(&db_name).await;
    }

    #[tokio::test]
    async fn cursor_and_migrations_coexist_with_sessions() {
        require_pg!();
        let (url, db_name) = create_test_db().await;
        let pool = atrg_db::connect(&url).await.expect("connect");

        // Run internal migrations (creates sessions, oauth_states)
        atrg_db::run_internal_migrations(&pool)
            .await
            .expect("internal migrations");

        // Also set up cursor table
        atrg_stream::cursor::ensure_cursor_table(&pool)
            .await
            .expect("ensure cursor table");

        // All three concerns coexist in the same database
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600;

        atrg_auth::session::create_session(
            &pool,
            "sess_coexist_001",
            "did:plc:coexist",
            "coexist.bsky.social",
            "token",
            None,
            expires,
        )
        .await
        .unwrap();

        atrg_stream::cursor::save_cursor(&pool, "my-consumer", 1700000000000000)
            .await
            .unwrap();

        // Verify both work independently
        let session = atrg_auth::session::find_session(&pool, "sess_coexist_001")
            .await
            .unwrap();
        assert!(session.is_some());

        let cursor = atrg_stream::cursor::load_cursor(&pool, "my-consumer")
            .await
            .unwrap();
        assert_eq!(cursor, Some(1700000000000000));

        pool.close().await;
        drop_test_db(&db_name).await;
    }
}
