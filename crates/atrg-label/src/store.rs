//! SQLite-backed label storage.

use sqlx::SqlitePool;

use crate::types::{Label, SignedLabel};

/// Persistent label store backed by SQLite.
pub struct LabelStore {
    db: SqlitePool,
}

impl LabelStore {
    /// Create a new label store using the given database pool.
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Run the label store migrations (creates the `atrg_labels` table).
    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS atrg_labels (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                src TEXT NOT NULL,
                uri TEXT NOT NULL,
                cid TEXT,
                val TEXT NOT NULL,
                neg INTEGER NOT NULL DEFAULT 0,
                cts TEXT NOT NULL,
                exp TEXT,
                sig TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&self.db)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_atrg_labels_uri ON atrg_labels(uri)")
            .execute(&self.db)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_atrg_labels_src ON atrg_labels(src)")
            .execute(&self.db)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_atrg_labels_val ON atrg_labels(val)")
            .execute(&self.db)
            .await?;

        Ok(())
    }

    /// Insert a signed label into the store.
    ///
    /// Returns the auto-generated row ID of the inserted label.
    pub async fn insert(&self, label: &SignedLabel) -> anyhow::Result<i64> {
        let result = sqlx::query(
            "INSERT INTO atrg_labels (src, uri, cid, val, neg, cts, exp, sig)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&label.label.src)
        .bind(&label.label.uri)
        .bind(&label.label.cid)
        .bind(&label.label.val)
        .bind(label.label.neg as i32)
        .bind(&label.label.cts)
        .bind(&label.label.exp)
        .bind(&label.sig)
        .execute(&self.db)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Query labels for a given subject URI.
    pub async fn query_by_uri(&self, uri: &str) -> anyhow::Result<Vec<SignedLabel>> {
        let rows = sqlx::query_as::<_, LabelRow>(
            "SELECT src, uri, cid, val, neg, cts, exp, sig
             FROM atrg_labels WHERE uri = ? ORDER BY id",
        )
        .bind(uri)
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_signed_label()).collect())
    }

    /// Query labels since a given cursor (row id), for subscription streaming.
    ///
    /// Returns pairs of `(id, SignedLabel)` so callers can use the id as the
    /// next cursor value.
    pub async fn query_since(
        &self,
        cursor: i64,
        limit: i64,
    ) -> anyhow::Result<Vec<(i64, SignedLabel)>> {
        let rows = sqlx::query_as::<_, LabelRowWithId>(
            "SELECT id, src, uri, cid, val, neg, cts, exp, sig
             FROM atrg_labels WHERE id > ? ORDER BY id LIMIT ?",
        )
        .bind(cursor)
        .bind(limit)
        .fetch_all(&self.db)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let id = r.id;
                (id, r.into_signed_label())
            })
            .collect())
    }
}

/// Internal row type for SQLx mapping.
#[derive(sqlx::FromRow)]
struct LabelRow {
    src: String,
    uri: String,
    cid: Option<String>,
    val: String,
    neg: i32,
    cts: String,
    exp: Option<String>,
    sig: String,
}

impl LabelRow {
    fn into_signed_label(self) -> SignedLabel {
        SignedLabel {
            label: Label {
                ver: 1,
                src: self.src,
                uri: self.uri,
                cid: self.cid,
                val: self.val,
                neg: self.neg != 0,
                cts: self.cts,
                exp: self.exp,
            },
            sig: self.sig,
        }
    }
}

/// Internal row type that includes the auto-generated id for cursor support.
#[derive(sqlx::FromRow)]
struct LabelRowWithId {
    id: i64,
    src: String,
    uri: String,
    cid: Option<String>,
    val: String,
    neg: i32,
    cts: String,
    exp: Option<String>,
    sig: String,
}

impl LabelRowWithId {
    fn into_signed_label(self) -> SignedLabel {
        SignedLabel {
            label: Label {
                ver: 1,
                src: self.src,
                uri: self.uri,
                cid: self.cid,
                val: self.val,
                neg: self.neg != 0,
                cts: self.cts,
                exp: self.exp,
            },
            sig: self.sig,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    fn make_signed_label(src: &str, uri: &str, val: &str) -> SignedLabel {
        SignedLabel {
            label: Label {
                ver: 1,
                src: src.to_string(),
                uri: uri.to_string(),
                cid: None,
                val: val.to_string(),
                neg: false,
                cts: "2024-01-01T00:00:00Z".to_string(),
                exp: None,
            },
            sig: "test-sig".to_string(),
        }
    }

    async fn setup_store() -> LabelStore {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let store = LabelStore::new(db);
        store.migrate().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_migrate_creates_table() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let store = LabelStore::new(db.clone());
        store.migrate().await.unwrap();

        // Verify the table exists by querying it.
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM atrg_labels")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(row.0, 0);
    }

    #[tokio::test]
    async fn test_insert_returns_positive_id() {
        let store = setup_store().await;
        let label = make_signed_label("did:plc:labeler", "at://did:plc:user/post/1", "spam");
        let id = store.insert(&label).await.unwrap();
        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_query_by_uri() {
        let store = setup_store().await;

        let uri_a = "at://did:plc:user/post/a";
        let uri_b = "at://did:plc:user/post/b";

        store
            .insert(&make_signed_label("did:plc:l", uri_a, "spam"))
            .await
            .unwrap();
        store
            .insert(&make_signed_label("did:plc:l", uri_a, "porn"))
            .await
            .unwrap();
        store
            .insert(&make_signed_label("did:plc:l", uri_b, "nudity"))
            .await
            .unwrap();

        let results_a = store.query_by_uri(uri_a).await.unwrap();
        assert_eq!(results_a.len(), 2);
        assert_eq!(results_a[0].label.val, "spam");
        assert_eq!(results_a[1].label.val, "porn");

        let results_b = store.query_by_uri(uri_b).await.unwrap();
        assert_eq!(results_b.len(), 1);
        assert_eq!(results_b[0].label.val, "nudity");
    }

    #[tokio::test]
    async fn test_query_by_uri_empty() {
        let store = setup_store().await;
        let results = store.query_by_uri("at://nonexistent").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_query_since_with_cursor() {
        let store = setup_store().await;
        let uri = "at://did:plc:user/post/1";
        for i in 0..5 {
            store
                .insert(&make_signed_label("did:plc:l", uri, &format!("val-{}", i)))
                .await
                .unwrap();
        }

        // First page: 3 results starting from cursor 0.
        let page1 = store.query_since(0, 3).await.unwrap();
        assert_eq!(page1.len(), 3);
        assert_eq!(page1[0].1.label.val, "val-0");
        assert_eq!(page1[2].1.label.val, "val-2");

        // Second page: use last id as cursor.
        let last_cursor = page1.last().unwrap().0;
        let page2 = store.query_since(last_cursor, 3).await.unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].1.label.val, "val-3");
        assert_eq!(page2[1].1.label.val, "val-4");
    }

    #[tokio::test]
    async fn test_query_since_respects_limit() {
        let store = setup_store().await;
        let uri = "at://did:plc:user/post/1";
        for i in 0..10 {
            store
                .insert(&make_signed_label("did:plc:l", uri, &format!("v{}", i)))
                .await
                .unwrap();
        }

        let results = store.query_since(0, 5).await.unwrap();
        assert_eq!(results.len(), 5);
    }
}
