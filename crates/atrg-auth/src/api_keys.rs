//! Native API key authentication for at-rust-go.
//!
//! Provides key generation, storage, lookup, and revocation — eliminating the
//! middleware hacks (synthetic session injection) previously needed for
//! programmatic / MCP access.
//!
//! # Design
//!
//! - Keys are prefixed (e.g. `atrg_`, `chg_`) and contain 32 random bytes
//!   hex-encoded for a total of 64 hex characters after the prefix.
//! - Only the SHA-256 hash of the key is persisted; the full key is shown
//!   once at creation time and cannot be recovered.
//! - The first 8 characters after the prefix are stored as `key_prefix` for
//!   identification in list/revoke operations (similar to GitHub token design).
//! - Scopes are stored as a comma-separated string in the database and
//!   exposed as `Vec<String>` in the Rust API.

use atrg_db::DbPool;
use rand::Rng;
use sha2::{Digest, Sha256};

// ─── SQL Table Definitions ──────────────────────────────────────────────────

/// SQLite DDL for the `api_keys` table.
pub const CREATE_API_KEYS_TABLE_SQLITE: &str = "\
CREATE TABLE IF NOT EXISTS api_keys (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    key_hash     TEXT NOT NULL UNIQUE,
    key_prefix   TEXT NOT NULL,
    did          TEXT NOT NULL,
    name         TEXT NOT NULL,
    scopes       TEXT NOT NULL DEFAULT '',
    expires_at   INTEGER,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    last_used_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_api_keys_did ON api_keys(did);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_prefix ON api_keys(key_prefix);
";

/// PostgreSQL DDL for the `api_keys` table.
pub const CREATE_API_KEYS_TABLE_POSTGRES: &str = "\
CREATE TABLE IF NOT EXISTS api_keys (
    id           BIGSERIAL PRIMARY KEY,
    key_hash     TEXT NOT NULL UNIQUE,
    key_prefix   TEXT NOT NULL,
    did          TEXT NOT NULL,
    name         TEXT NOT NULL,
    scopes       TEXT NOT NULL DEFAULT '',
    expires_at   BIGINT,
    created_at   BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
    last_used_at BIGINT
);
CREATE INDEX IF NOT EXISTS idx_api_keys_did ON api_keys(did);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_prefix ON api_keys(key_prefix);
";

// ─── Types ──────────────────────────────────────────────────────────────────

/// A stored API key record.
///
/// The actual secret is never stored — only its SHA-256 hash. The
/// `key_prefix` field contains the first 8 hex characters after the
/// application prefix, useful for display and revocation.
#[derive(Debug, Clone)]
pub struct ApiKey {
    /// Auto-increment row ID.
    pub id: i64,
    /// First 8 hex chars of the random portion (for display/revocation).
    pub key_prefix: String,
    /// The DID this key authenticates as.
    pub did: String,
    /// Human-friendly name (e.g. "CI bot", "MCP server").
    pub name: String,
    /// Permission scopes granted to this key.
    pub scopes: Vec<String>,
    /// Optional expiry as a Unix timestamp. `None` means the key never expires.
    pub expires_at: Option<i64>,
    /// Unix timestamp of creation.
    pub created_at: i64,
    /// Last time this key was used for authentication.
    pub last_used_at: Option<i64>,
}

/// Internal row representation matching the DB schema.
#[derive(Debug, sqlx::FromRow)]
struct ApiKeyRow {
    id: i64,
    #[allow(dead_code)]
    key_hash: String,
    key_prefix: String,
    did: String,
    name: String,
    scopes: String,
    expires_at: Option<i64>,
    created_at: i64,
    last_used_at: Option<i64>,
}

impl ApiKeyRow {
    fn into_api_key(self) -> ApiKey {
        let scopes = if self.scopes.is_empty() {
            Vec::new()
        } else {
            self.scopes
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect()
        };
        ApiKey {
            id: self.id,
            key_prefix: self.key_prefix,
            did: self.did,
            name: self.name,
            scopes,
            expires_at: self.expires_at,
            created_at: self.created_at,
            last_used_at: self.last_used_at,
        }
    }
}

// ─── Key Generation ─────────────────────────────────────────────────────────

/// Generate a new API key with the given prefix (e.g. `"atrg_"` or `"chg_"`).
///
/// Returns `(full_key, key_hash)` — the full key is shown to the user once at
/// creation time, and the SHA-256 hex hash is stored in the database.
///
/// The key format is: `{prefix}{64 hex chars}` (32 random bytes hex-encoded).
pub fn generate_key(prefix: &str) -> (String, String) {
    let mut random_bytes = [0u8; 32];
    rand::thread_rng().fill(&mut random_bytes);

    let hex_part = hex::encode(random_bytes);
    let full_key = format!("{prefix}{hex_part}");
    let key_hash = hash_key(&full_key);

    (full_key, key_hash)
}

/// Compute the SHA-256 hex digest of a full API key.
fn hash_key(full_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(full_key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Extract the display prefix from a full key.
///
/// Given `"atrg_abcdef0123456789..."`, returns `"atrg_abcdef01"` — the
/// application prefix plus the first 8 hex characters.
fn extract_display_prefix(full_key: &str, app_prefix: &str) -> String {
    let after_prefix = &full_key[app_prefix.len()..];
    let visible_chars: String = after_prefix.chars().take(8).collect();
    format!("{app_prefix}{visible_chars}")
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn scopes_to_csv(scopes: &[String]) -> String {
    scopes.join(",")
}

// ─── CRUD Operations ────────────────────────────────────────────────────────

/// Create a new API key for the given DID.
///
/// Returns the full key (to show the user once) and the stored [`ApiKey`]
/// metadata. The full key **cannot** be recovered after this call.
pub async fn create_api_key(
    pool: &DbPool,
    did: &str,
    name: &str,
    scopes: &[String],
    prefix: &str,
) -> anyhow::Result<(String, ApiKey)> {
    let (full_key, key_hash) = generate_key(prefix);
    let key_prefix = extract_display_prefix(&full_key, prefix);
    let scopes_csv = scopes_to_csv(scopes);
    let created_at = now_unix();

    let id: i64 = match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            let result = sqlx::query(
                "INSERT INTO api_keys (key_hash, key_prefix, did, name, scopes, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&key_hash)
            .bind(&key_prefix)
            .bind(did)
            .bind(name)
            .bind(&scopes_csv)
            .bind(created_at)
            .execute(p)
            .await?;
            result.last_insert_rowid()
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            let row: (i64,) = sqlx::query_as(
                "INSERT INTO api_keys (key_hash, key_prefix, did, name, scopes, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 RETURNING id",
            )
            .bind(&key_hash)
            .bind(&key_prefix)
            .bind(did)
            .bind(name)
            .bind(&scopes_csv)
            .bind(created_at)
            .fetch_one(p)
            .await?;
            row.0
        }
    };

    let api_key = ApiKey {
        id,
        key_prefix,
        did: did.to_owned(),
        name: name.to_owned(),
        scopes: scopes.to_vec(),
        expires_at: None,
        created_at,
        last_used_at: None,
    };

    tracing::info!(did = %did, name = %name, prefix = %api_key.key_prefix, "API key created");
    Ok((full_key, api_key))
}

/// Look up an API key by its full secret value.
///
/// Hashes the provided key and searches by hash. Returns `None` if not found
/// or if the key has expired. Updates `last_used_at` on successful lookup.
pub async fn find_by_key(pool: &DbPool, full_key: &str) -> anyhow::Result<Option<ApiKey>> {
    let key_hash = hash_key(full_key);
    let now = now_unix();

    let row: Option<ApiKeyRow> = match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query_as::<_, ApiKeyRow>(
                "SELECT id, key_hash, key_prefix, did, name, scopes, expires_at, created_at, last_used_at
                 FROM api_keys
                 WHERE key_hash = ? AND (expires_at IS NULL OR expires_at > ?)",
            )
            .bind(&key_hash)
            .bind(now)
            .fetch_optional(p)
            .await?
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query_as::<_, ApiKeyRow>(
                "SELECT id, key_hash, key_prefix, did, name, scopes, expires_at, created_at, last_used_at
                 FROM api_keys
                 WHERE key_hash = $1 AND (expires_at IS NULL OR expires_at > $2)",
            )
            .bind(&key_hash)
            .bind(now)
            .fetch_optional(p)
            .await?
        }
    };

    // Update last_used_at on successful lookup.
    if row.is_some() {
        let touched = now_unix();
        match pool {
            #[cfg(feature = "sqlite")]
            DbPool::Sqlite(p) => {
                let _ = sqlx::query("UPDATE api_keys SET last_used_at = ? WHERE key_hash = ?")
                    .bind(touched)
                    .bind(&key_hash)
                    .execute(p)
                    .await;
            }
            #[cfg(feature = "postgres")]
            DbPool::Postgres(p) => {
                let _ = sqlx::query("UPDATE api_keys SET last_used_at = $1 WHERE key_hash = $2")
                    .bind(touched)
                    .bind(&key_hash)
                    .execute(p)
                    .await;
            }
        }
    }

    Ok(row.map(ApiKeyRow::into_api_key))
}

/// List API keys, optionally filtered by DID.
///
/// Does **not** return the key hash (it's useless to the caller). Returns
/// metadata only.
pub async fn list_api_keys(pool: &DbPool, did: Option<&str>) -> anyhow::Result<Vec<ApiKey>> {
    let rows: Vec<ApiKeyRow> = match (pool, did) {
        #[cfg(feature = "sqlite")]
        (DbPool::Sqlite(p), Some(did)) => {
            sqlx::query_as::<_, ApiKeyRow>(
                "SELECT id, key_hash, key_prefix, did, name, scopes, expires_at, created_at, last_used_at
                 FROM api_keys WHERE did = ? ORDER BY created_at DESC",
            )
            .bind(did)
            .fetch_all(p)
            .await?
        }
        #[cfg(feature = "sqlite")]
        (DbPool::Sqlite(p), None) => {
            sqlx::query_as::<_, ApiKeyRow>(
                "SELECT id, key_hash, key_prefix, did, name, scopes, expires_at, created_at, last_used_at
                 FROM api_keys ORDER BY created_at DESC",
            )
            .fetch_all(p)
            .await?
        }
        #[cfg(feature = "postgres")]
        (DbPool::Postgres(p), Some(did)) => {
            sqlx::query_as::<_, ApiKeyRow>(
                "SELECT id, key_hash, key_prefix, did, name, scopes, expires_at, created_at, last_used_at
                 FROM api_keys WHERE did = $1 ORDER BY created_at DESC",
            )
            .bind(did)
            .fetch_all(p)
            .await?
        }
        #[cfg(feature = "postgres")]
        (DbPool::Postgres(p), None) => {
            sqlx::query_as::<_, ApiKeyRow>(
                "SELECT id, key_hash, key_prefix, did, name, scopes, expires_at, created_at, last_used_at
                 FROM api_keys ORDER BY created_at DESC",
            )
            .fetch_all(p)
            .await?
        }
    };

    Ok(rows.into_iter().map(ApiKeyRow::into_api_key).collect())
}

/// Revoke (delete) an API key by its display prefix (e.g. `"atrg_abcdef01"`).
///
/// Returns `true` if a key was deleted, `false` if no matching key was found.
pub async fn revoke_api_key(pool: &DbPool, key_prefix: &str) -> anyhow::Result<bool> {
    let rows_affected: u64 = match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => sqlx::query("DELETE FROM api_keys WHERE key_prefix = ?")
            .bind(key_prefix)
            .execute(p)
            .await?
            .rows_affected(),
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => sqlx::query("DELETE FROM api_keys WHERE key_prefix = $1")
            .bind(key_prefix)
            .execute(p)
            .await?
            .rows_affected(),
    };

    if rows_affected > 0 {
        tracing::info!(key_prefix = %key_prefix, "API key revoked");
    }
    Ok(rows_affected > 0)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_key_has_correct_format() {
        let (full_key, hash) = generate_key("atrg_");
        // Prefix + 64 hex chars = prefix.len() + 64
        assert!(full_key.starts_with("atrg_"));
        assert_eq!(full_key.len(), 5 + 64); // "atrg_" is 5 chars
                                            // Hash is 64 hex chars (SHA-256)
        assert_eq!(hash.len(), 64);
        // All chars after prefix are hex
        assert!(full_key[5..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_key_custom_prefix() {
        let (full_key, _hash) = generate_key("chg_");
        assert!(full_key.starts_with("chg_"));
        assert_eq!(full_key.len(), 4 + 64); // "chg_" is 4 chars
    }

    #[test]
    fn generate_key_produces_unique_keys() {
        let (k1, _) = generate_key("atrg_");
        let (k2, _) = generate_key("atrg_");
        assert_ne!(k1, k2);
    }

    #[test]
    fn hash_is_deterministic() {
        let key = "atrg_abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567";
        let h1 = hash_key(key);
        let h2 = hash_key(key);
        assert_eq!(h1, h2);
    }

    #[test]
    fn extract_display_prefix_works() {
        let full_key = "atrg_abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567";
        let prefix = extract_display_prefix(full_key, "atrg_");
        assert_eq!(prefix, "atrg_abcdef01");
    }

    #[test]
    fn scopes_csv_round_trip() {
        let scopes = vec!["read".to_owned(), "write".to_owned(), "admin".to_owned()];
        let csv = scopes_to_csv(&scopes);
        assert_eq!(csv, "read,write,admin");
        let recovered: Vec<String> = csv.split(',').map(|s| s.trim().to_owned()).collect();
        assert_eq!(recovered, scopes);
    }

    #[test]
    fn empty_scopes_csv() {
        let scopes: Vec<String> = Vec::new();
        let csv = scopes_to_csv(&scopes);
        assert_eq!(csv, "");
    }

    #[cfg(feature = "sqlite")]
    mod sqlite_tests {
        use super::*;

        async fn test_pool() -> DbPool {
            let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
            sqlx::raw_sql(CREATE_API_KEYS_TABLE_SQLITE)
                .execute(&pool)
                .await
                .unwrap();
            DbPool::Sqlite(pool)
        }

        #[tokio::test]
        async fn create_and_find_api_key() {
            let pool = test_pool().await;
            let scopes = vec!["read".to_owned(), "write".to_owned()];
            let (full_key, api_key) =
                create_api_key(&pool, "did:plc:test123", "Test Key", &scopes, "atrg_")
                    .await
                    .unwrap();

            assert!(full_key.starts_with("atrg_"));
            assert_eq!(api_key.did, "did:plc:test123");
            assert_eq!(api_key.name, "Test Key");
            assert_eq!(api_key.scopes, scopes);

            // Look up by full key
            let found = find_by_key(&pool, &full_key).await.unwrap();
            assert!(found.is_some());
            let found = found.unwrap();
            assert_eq!(found.did, "did:plc:test123");
            assert_eq!(found.name, "Test Key");
            assert_eq!(found.scopes, scopes);

            // Verify last_used_at was updated in the DB (the returned struct
            // reflects the row at SELECT time, but a second lookup should show it).
            let found2 = find_by_key(&pool, &full_key).await.unwrap().unwrap();
            assert!(found2.last_used_at.is_some());
        }

        #[tokio::test]
        async fn find_nonexistent_key_returns_none() {
            let pool = test_pool().await;
            let found = find_by_key(&pool, "atrg_nonexistent_key_that_does_not_exist_at_all")
                .await
                .unwrap();
            assert!(found.is_none());
        }

        #[tokio::test]
        async fn list_keys_by_did() {
            let pool = test_pool().await;
            let scopes = vec!["read".to_owned()];

            create_api_key(&pool, "did:plc:alice", "Key 1", &scopes, "atrg_")
                .await
                .unwrap();
            create_api_key(&pool, "did:plc:alice", "Key 2", &scopes, "atrg_")
                .await
                .unwrap();
            create_api_key(&pool, "did:plc:bob", "Bob Key", &scopes, "atrg_")
                .await
                .unwrap();

            let alice_keys = list_api_keys(&pool, Some("did:plc:alice")).await.unwrap();
            assert_eq!(alice_keys.len(), 2);

            let all_keys = list_api_keys(&pool, None).await.unwrap();
            assert_eq!(all_keys.len(), 3);
        }

        #[tokio::test]
        async fn revoke_api_key_works() {
            let pool = test_pool().await;
            let scopes = vec!["admin".to_owned()];
            let (full_key, api_key) =
                create_api_key(&pool, "did:plc:test", "Revokable", &scopes, "atrg_")
                    .await
                    .unwrap();

            // Key exists
            assert!(find_by_key(&pool, &full_key).await.unwrap().is_some());

            // Revoke
            let revoked = revoke_api_key(&pool, &api_key.key_prefix).await.unwrap();
            assert!(revoked);

            // Key no longer exists
            assert!(find_by_key(&pool, &full_key).await.unwrap().is_none());
        }

        #[tokio::test]
        async fn revoke_nonexistent_key_returns_false() {
            let pool = test_pool().await;
            let revoked = revoke_api_key(&pool, "atrg_nonexist").await.unwrap();
            assert!(!revoked);
        }

        #[tokio::test]
        async fn expired_key_not_returned() {
            let pool = test_pool().await;
            let (full_key, key_hash) = generate_key("atrg_");
            let key_prefix = extract_display_prefix(&full_key, "atrg_");
            let past = now_unix() - 3600; // expired 1 hour ago

            // Insert directly with an already-expired timestamp
            match &pool {
                DbPool::Sqlite(p) => {
                    sqlx::query(
                        "INSERT INTO api_keys (key_hash, key_prefix, did, name, scopes, expires_at, created_at)
                         VALUES (?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(&key_hash)
                    .bind(&key_prefix)
                    .bind("did:plc:expired")
                    .bind("Expired Key")
                    .bind("")
                    .bind(past)
                    .bind(now_unix())
                    .execute(p)
                    .await
                    .unwrap();
                }
                #[cfg(feature = "postgres")]
                _ => unreachable!("test only runs with sqlite"),
            }

            let found = find_by_key(&pool, &full_key).await.unwrap();
            assert!(found.is_none());
        }
    }
}
