//! Profile handler — returns user profile with counts.

use atrg_core::error::AtrgError;
use atrg_core::state::AppState;
use axum::extract::{Path, State};
use axum::Json;

/// `GET /api/profile/{handle}`
///
/// Returns profile information including post, follower, and following counts.
pub async fn profile(
    State(state): State<AppState>,
    Path(handle): Path<String>,
) -> Result<Json<serde_json::Value>, AtrgError> {
    // Resolve handle to DID
    let identity = state
        .identity
        .resolve(&handle)
        .await
        .map_err(|e| AtrgError::BadRequest(format!("could not resolve handle: {e}")))?;

    let did = &identity.did;

    let db = state
        .db
        .as_sqlite()
        .ok_or_else(|| AtrgError::Internal(anyhow::anyhow!("social-example requires a SQLite pool")))?;

    // Count posts
    let (post_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM posts WHERE did = ?")
        .bind(did)
        .fetch_one(db)
        .await
        .unwrap_or((0,));

    // Count followers
    let (follower_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM follows WHERE target_did = ?")
            .bind(did)
            .fetch_one(db)
            .await
            .unwrap_or((0,));

    // Count following
    let (following_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM follows WHERE subject_did = ?")
            .bind(did)
            .fetch_one(db)
            .await
            .unwrap_or((0,));

    Ok(Json(serde_json::json!({
        "did": did,
        "handle": identity.handle,
        "postCount": post_count,
        "followerCount": follower_count,
        "followingCount": following_count,
    })))
}
