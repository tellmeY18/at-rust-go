//! Role-Based Access Control and ban system for at-rust-go.
//!
//! Provides role management, scoped role checking, ban enforcement,
//! and admin DID bootstrapping.

use atrg_db::DbPool;
use serde::{Deserialize, Serialize};

/// A role assignment for a DID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    /// The DID this role is assigned to.
    pub did: String,
    /// The role name (e.g. "admin", "classRep", "student").
    pub role: String,
    /// Optional scope type for resource-level roles (e.g. "course").
    pub scope_type: Option<String>,
    /// Optional scope identifier (e.g. a course AT-URI).
    pub scope_id: Option<String>,
    /// DID of the user who granted this role, or "system:bootstrap".
    pub granted_by: Option<String>,
    /// ISO timestamp or unix seconds string of when the role was granted.
    pub granted_at: String,
}

/// A ban record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ban {
    /// The banned DID.
    pub did: String,
    /// Human-readable reason for the ban.
    pub reason: Option<String>,
    /// Unix timestamp when the ban expires, or None for permanent.
    pub expires_at: Option<i64>,
    /// DID of the admin who created the ban.
    pub created_by: String,
    /// Unix timestamp when the ban was created.
    pub created_at: i64,
}

/// SQL for creating the roles table (SQLite).
pub const CREATE_ROLES_TABLE_SQLITE: &str = r#"
CREATE TABLE IF NOT EXISTS atrg_roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    did TEXT NOT NULL,
    role TEXT NOT NULL,
    scope_type TEXT,
    scope_id TEXT,
    granted_by TEXT,
    granted_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(did, role, scope_type, scope_id)
);
CREATE INDEX IF NOT EXISTS idx_atrg_roles_did ON atrg_roles(did);
"#;

/// SQL for creating the roles table (PostgreSQL).
pub const CREATE_ROLES_TABLE_POSTGRES: &str = r#"
CREATE TABLE IF NOT EXISTS atrg_roles (
    id BIGSERIAL PRIMARY KEY,
    did TEXT NOT NULL,
    role TEXT NOT NULL,
    scope_type TEXT,
    scope_id TEXT,
    granted_by TEXT,
    granted_at TEXT NOT NULL DEFAULT NOW()::text,
    UNIQUE(did, role, scope_type, scope_id)
);
CREATE INDEX IF NOT EXISTS idx_atrg_roles_did ON atrg_roles(did);
"#;

/// SQL for creating the bans table (SQLite).
pub const CREATE_BANS_TABLE_SQLITE: &str = r#"
CREATE TABLE IF NOT EXISTS atrg_bans (
    did TEXT PRIMARY KEY,
    reason TEXT,
    expires_at INTEGER,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
"#;

/// SQL for creating the bans table (PostgreSQL).
pub const CREATE_BANS_TABLE_POSTGRES: &str = r#"
CREATE TABLE IF NOT EXISTS atrg_bans (
    did TEXT PRIMARY KEY,
    reason TEXT,
    expires_at BIGINT,
    created_by TEXT NOT NULL,
    created_at BIGINT NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::bigint
);
"#;

/// Check if a DID has a specific role (optionally scoped to a resource).
///
/// When `scope_id` is provided, matches roles that are either scoped to that
/// specific resource OR have no scope (instance-wide grant). This means an
/// instance-wide "admin" role will match any scope check.
pub async fn has_role(
    pool: &DbPool,
    did: &str,
    role: &str,
    scope_id: Option<&str>,
) -> anyhow::Result<bool> {
    let count: i64 = match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            if let Some(sid) = scope_id {
                sqlx::query_scalar(
                    "SELECT COUNT(*) FROM atrg_roles WHERE did = ?1 AND role = ?2 AND (scope_id = ?3 OR scope_id IS NULL)",
                )
                .bind(did)
                .bind(role)
                .bind(sid)
                .fetch_one(p)
                .await?
            } else {
                sqlx::query_scalar("SELECT COUNT(*) FROM atrg_roles WHERE did = ?1 AND role = ?2")
                    .bind(did)
                    .bind(role)
                    .fetch_one(p)
                    .await?
            }
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            if let Some(sid) = scope_id {
                sqlx::query_scalar(
                    "SELECT COUNT(*) FROM atrg_roles WHERE did = $1 AND role = $2 AND (scope_id = $3 OR scope_id IS NULL)",
                )
                .bind(did)
                .bind(role)
                .bind(sid)
                .fetch_one(p)
                .await?
            } else {
                sqlx::query_scalar("SELECT COUNT(*) FROM atrg_roles WHERE did = $1 AND role = $2")
                    .bind(did)
                    .bind(role)
                    .fetch_one(p)
                    .await?
            }
        }
    };
    Ok(count > 0)
}

/// Check if a DID is currently banned.
///
/// Returns `true` if there is an active ban (either permanent or with an
/// `expires_at` in the future). Expired bans are not considered active.
pub async fn is_banned(pool: &DbPool, did: &str) -> anyhow::Result<bool> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let count: i64 = match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM atrg_bans WHERE did = ?1 AND (expires_at IS NULL OR expires_at > ?2)",
            )
            .bind(did)
            .bind(now)
            .fetch_one(p)
            .await?
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM atrg_bans WHERE did = $1 AND (expires_at IS NULL OR expires_at > $2)",
            )
            .bind(did)
            .bind(now)
            .fetch_one(p)
            .await?
        }
    };
    Ok(count > 0)
}

/// Ban a DID. `ttl_secs` of `None` means permanent.
///
/// If the DID is already banned, the existing ban is replaced with the new
/// parameters (upsert semantics).
pub async fn ban_did(
    pool: &DbPool,
    did: &str,
    reason: Option<&str>,
    ttl_secs: Option<i64>,
    created_by: &str,
) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let expires_at = ttl_secs.map(|ttl| now + ttl);

    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query(
                "INSERT OR REPLACE INTO atrg_bans (did, reason, expires_at, created_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(did)
            .bind(reason)
            .bind(expires_at)
            .bind(created_by)
            .bind(now)
            .execute(p)
            .await?;
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query(
                "INSERT INTO atrg_bans (did, reason, expires_at, created_by, created_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (did) DO UPDATE SET reason = $2, expires_at = $3, created_by = $4, created_at = $5",
            )
            .bind(did)
            .bind(reason)
            .bind(expires_at)
            .bind(created_by)
            .bind(now)
            .execute(p)
            .await?;
        }
    }
    Ok(())
}

/// Lift a ban on a DID.
///
/// Returns `true` if a ban was actually removed, `false` if the DID was not
/// banned.
pub async fn lift_ban(pool: &DbPool, did: &str) -> anyhow::Result<bool> {
    let rows = match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => sqlx::query("DELETE FROM atrg_bans WHERE did = ?1")
            .bind(did)
            .execute(p)
            .await?
            .rows_affected(),
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => sqlx::query("DELETE FROM atrg_bans WHERE did = $1")
            .bind(did)
            .execute(p)
            .await?
            .rows_affected(),
    };
    Ok(rows > 0)
}

/// Grant a role to a DID (with optional resource scoping).
///
/// If the exact same role assignment already exists (same DID, role,
/// scope_type, scope_id), this is a no-op.
pub async fn grant_role(
    pool: &DbPool,
    did: &str,
    role: &str,
    scope_type: Option<&str>,
    scope_id: Option<&str>,
    granted_by: Option<&str>,
) -> anyhow::Result<()> {
    let now = chrono_now();
    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query(
                "INSERT OR IGNORE INTO atrg_roles (did, role, scope_type, scope_id, granted_by, granted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(did)
            .bind(role)
            .bind(scope_type)
            .bind(scope_id)
            .bind(granted_by)
            .bind(&now)
            .execute(p)
            .await?;
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query(
                "INSERT INTO atrg_roles (did, role, scope_type, scope_id, granted_by, granted_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
            )
            .bind(did)
            .bind(role)
            .bind(scope_type)
            .bind(scope_id)
            .bind(granted_by)
            .bind(&now)
            .execute(p)
            .await?;
        }
    }
    Ok(())
}

/// Revoke a role from a DID.
///
/// When `scope_id` is provided, only the scoped assignment is removed.
/// When `scope_id` is `None`, all assignments of that role for the DID are
/// removed (both scoped and unscoped).
///
/// Returns `true` if at least one assignment was removed.
pub async fn revoke_role(
    pool: &DbPool,
    did: &str,
    role: &str,
    scope_id: Option<&str>,
) -> anyhow::Result<bool> {
    let rows = match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            if let Some(sid) = scope_id {
                sqlx::query("DELETE FROM atrg_roles WHERE did = ?1 AND role = ?2 AND scope_id = ?3")
                    .bind(did)
                    .bind(role)
                    .bind(sid)
                    .execute(p)
                    .await?
                    .rows_affected()
            } else {
                sqlx::query("DELETE FROM atrg_roles WHERE did = ?1 AND role = ?2")
                    .bind(did)
                    .bind(role)
                    .execute(p)
                    .await?
                    .rows_affected()
            }
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            if let Some(sid) = scope_id {
                sqlx::query("DELETE FROM atrg_roles WHERE did = $1 AND role = $2 AND scope_id = $3")
                    .bind(did)
                    .bind(role)
                    .bind(sid)
                    .execute(p)
                    .await?
                    .rows_affected()
            } else {
                sqlx::query("DELETE FROM atrg_roles WHERE did = $1 AND role = $2")
                    .bind(did)
                    .bind(role)
                    .execute(p)
                    .await?
                    .rows_affected()
            }
        }
    };
    Ok(rows > 0)
}

/// Bootstrap admin DIDs — ensures each DID in the list has the "admin" role.
///
/// Called on startup from `AtrgApp::run()`. Uses upsert semantics so it is
/// safe to call repeatedly (idempotent). The `granted_by` field is set to
/// `"system:bootstrap"` to distinguish auto-provisioned admins from manually
/// granted ones.
pub async fn bootstrap_admins(pool: &DbPool, admin_dids: &[String]) -> anyhow::Result<()> {
    for did in admin_dids {
        grant_role(pool, did, "admin", None, None, Some("system:bootstrap")).await?;
        tracing::info!(did = %did, "auto-provisioned admin DID");
    }
    Ok(())
}

/// Produce a UTC timestamp string (unix seconds) without pulling in chrono.
fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}
