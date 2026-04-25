//! Session types and database operations.

use rand::Rng;
use sqlx::SqlitePool;

/// The source of authentication credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthSource {
    /// Authenticated via atrg's own session token (cookie or bearer).
    Atrg,
    /// Authenticated via a PDS-issued AT Protocol JWT.
    AtprotoJwt,
}

/// A resolved authentication session, shared across all auth paths.
///
/// Handlers receive this via `AuthUser` or `RequireAuth` extractors.
/// The `source` field indicates whether the credential was an atrg
/// session token or an AT Protocol JWT — but most handlers shouldn't
/// need to check.
#[derive(Debug, Clone)]
pub struct AtrgSession {
    /// The user's DID (e.g. `did:plc:...`).
    pub did: String,
    /// The user's handle (e.g. `alice.bsky.social`).
    pub handle: String,
    /// The access token for outbound AT Protocol calls.
    pub access_token: String,
    /// The refresh token (only present for atrg sessions).
    pub refresh_token: Option<String>,
    /// Unix timestamp when this session expires.
    pub expires_at: i64,
    /// How the user authenticated.
    pub source: AuthSource,
}

/// Generate a cryptographically random session ID (32 bytes, base64url-encoded).
pub fn generate_session_id() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    base64_url_encode(&bytes)
}

/// Base64url-encode without padding.
fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// Look up a session by ID, filtering out expired sessions.
pub async fn find_session(
    pool: &SqlitePool,
    session_id: &str,
) -> anyhow::Result<Option<AtrgSession>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let row = sqlx::query_as::<_, SessionRow>(
        "SELECT id, did, handle, access_token, refresh_token, expires_at
         FROM atrg_sessions
         WHERE id = ? AND expires_at > ?",
    )
    .bind(session_id)
    .bind(now)
    .fetch_optional(pool)
    .await?;

    // Update last_used_at on access
    if row.is_some() {
        let _ = sqlx::query("UPDATE atrg_sessions SET last_used_at = unixepoch() WHERE id = ?")
            .bind(session_id)
            .execute(pool)
            .await;
    }

    Ok(row.map(|r| AtrgSession {
        did: r.did,
        handle: r.handle,
        access_token: r.access_token,
        refresh_token: r.refresh_token,
        expires_at: r.expires_at,
        source: AuthSource::Atrg,
    }))
}

/// Insert a new session into the database.
pub async fn create_session(
    pool: &SqlitePool,
    session_id: &str,
    did: &str,
    handle: &str,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_at: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO atrg_sessions (id, did, handle, access_token, refresh_token, expires_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(session_id)
    .bind(did)
    .bind(handle)
    .bind(access_token)
    .bind(refresh_token)
    .bind(expires_at)
    .execute(pool)
    .await?;

    tracing::debug!(did = %did, handle = %handle, "session created");
    Ok(())
}

/// Delete a session by ID (logout).
pub async fn delete_session(pool: &SqlitePool, session_id: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM atrg_sessions WHERE id = ?")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete all expired sessions (cleanup).
pub async fn cleanup_expired_sessions(pool: &SqlitePool) -> anyhow::Result<u64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let result = sqlx::query("DELETE FROM atrg_sessions WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;

    let deleted = result.rows_affected();
    if deleted > 0 {
        tracing::info!(count = deleted, "cleaned up expired sessions");
    }
    Ok(deleted)
}

/// Delete expired OAuth states (cleanup).
pub async fn cleanup_expired_oauth_states(pool: &SqlitePool) -> anyhow::Result<u64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let result = sqlx::query("DELETE FROM atrg_oauth_states WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    #[allow(dead_code)]
    id: String,
    did: String,
    handle: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = atrg_db::connect("sqlite::memory:").await.unwrap();
        atrg_db::run_internal_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn generate_session_id_is_unique() {
        let a = generate_session_id();
        let b = generate_session_id();
        assert_ne!(a, b);
        assert!(a.len() >= 40, "session id should be ~43 chars base64url");
    }

    #[tokio::test]
    async fn create_and_find_session() {
        let pool = test_pool().await;
        let sid = generate_session_id();
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 86400;

        create_session(
            &pool,
            &sid,
            "did:plc:test123",
            "alice.test",
            "tok_abc",
            Some("ref_xyz"),
            expires,
        )
        .await
        .unwrap();

        let session = find_session(&pool, &sid)
            .await
            .unwrap()
            .expect("session should exist");
        assert_eq!(session.did, "did:plc:test123");
        assert_eq!(session.handle, "alice.test");
        assert_eq!(session.access_token, "tok_abc");
        assert_eq!(session.refresh_token.as_deref(), Some("ref_xyz"));
        assert_eq!(session.source, AuthSource::Atrg);
    }

    #[tokio::test]
    async fn expired_session_not_found() {
        let pool = test_pool().await;
        let sid = generate_session_id();
        // Expired 1 hour ago
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 3600;

        create_session(
            &pool,
            &sid,
            "did:plc:expired",
            "old.test",
            "tok",
            None,
            expires,
        )
        .await
        .unwrap();

        let session = find_session(&pool, &sid).await.unwrap();
        assert!(session.is_none(), "expired session should not be returned");
    }

    #[tokio::test]
    async fn delete_session_works() {
        let pool = test_pool().await;
        let sid = generate_session_id();
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 86400;

        create_session(&pool, &sid, "did:plc:del", "del.test", "tok", None, expires)
            .await
            .unwrap();

        delete_session(&pool, &sid).await.unwrap();
        let session = find_session(&pool, &sid).await.unwrap();
        assert!(session.is_none());
    }

    #[tokio::test]
    async fn cleanup_expired_sessions_works() {
        let pool = test_pool().await;
        let expired = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 3600;
        let valid = expired + 7200;

        create_session(&pool, "expired1", "did:plc:e1", "e1", "tok", None, expired)
            .await
            .unwrap();
        create_session(&pool, "valid1", "did:plc:v1", "v1", "tok", None, valid)
            .await
            .unwrap();

        let deleted = cleanup_expired_sessions(&pool).await.unwrap();
        assert_eq!(deleted, 1);

        assert!(find_session(&pool, "valid1").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn missing_session_returns_none() {
        let pool = test_pool().await;
        let session = find_session(&pool, "nonexistent").await.unwrap();
        assert!(session.is_none());
    }
}
