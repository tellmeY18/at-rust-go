//! Timeline handler — returns recent posts.

use atrg_core::error::AtrgError;
use atrg_core::pagination::{decode_cursor, encode_cursor, paginated_response, PaginationParams};
use atrg_core::state::AppState;
use axum::extract::{Query, State};
use axum::Json;

/// `GET /api/timeline?cursor=&limit=`
///
/// Returns recent posts from the local database, sorted by indexed_at descending.
pub async fn timeline(
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<serde_json::Value>, AtrgError> {
    let limit = params.effective_limit() as i64;

    let db = state.db.as_sqlite().ok_or_else(|| {
        AtrgError::Internal(anyhow::anyhow!("social-example requires a SQLite pool"))
    })?;

    let rows = if let Some(ref cursor) = params.cursor {
        let (ts, _rkey) = decode_cursor(cursor)?;
        sqlx::query_as::<_, PostRow>(
            "SELECT did, rkey, text, created_at, indexed_at
             FROM posts
             WHERE indexed_at < ?
             ORDER BY indexed_at DESC
             LIMIT ?",
        )
        .bind(ts)
        .bind(limit + 1)
        .fetch_all(db)
        .await
        .map_err(AtrgError::Database)?
    } else {
        sqlx::query_as::<_, PostRow>(
            "SELECT did, rkey, text, created_at, indexed_at
             FROM posts
             ORDER BY indexed_at DESC
             LIMIT ?",
        )
        .bind(limit + 1)
        .fetch_all(db)
        .await
        .map_err(AtrgError::Database)?
    };

    let has_more = rows.len() as i64 > limit;
    let items: Vec<_> = rows.iter().take(limit as usize).collect();
    let cursor = if has_more {
        items.last().map(|p| encode_cursor(p.indexed_at, &p.rkey))
    } else {
        None
    };

    let items_json: Vec<serde_json::Value> = items
        .iter()
        .map(|p| {
            serde_json::json!({
                "did": p.did,
                "rkey": p.rkey,
                "text": p.text,
                "createdAt": p.created_at,
                "indexedAt": p.indexed_at,
            })
        })
        .collect();

    Ok(Json(paginated_response(items_json, cursor)))
}

#[derive(sqlx::FromRow)]
struct PostRow {
    did: String,
    rkey: String,
    text: String,
    created_at: String,
    indexed_at: i64,
}
