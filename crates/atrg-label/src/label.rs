//! Label service: create, store, and manage labels.

use crate::signing::LabelSigner;
use crate::store::LabelStore;
use crate::types::{Label, LabelValue, SignedLabel};
use sqlx::SqlitePool;

/// The main label service that creates, signs, and stores labels.
pub struct LabelService {
    /// Persistent label storage.
    store: LabelStore,
    /// Signer for producing label signatures.
    signer: LabelSigner,
    /// DID of this labeler service.
    labeler_did: String,
}

impl LabelService {
    /// Create a new label service.
    pub fn new(db: SqlitePool, signer: LabelSigner, labeler_did: String) -> Self {
        Self {
            store: LabelStore::new(db),
            signer,
            labeler_did,
        }
    }

    /// Run migrations to create the labels table.
    pub async fn migrate(&self) -> anyhow::Result<()> {
        self.store.migrate().await
    }

    /// Create and store a label for a subject.
    ///
    /// The label is signed and persisted to the store before being returned.
    pub async fn create_label(
        &self,
        subject_uri: &str,
        value: LabelValue,
        subject_cid: Option<&str>,
    ) -> anyhow::Result<SignedLabel> {
        let label = Label {
            ver: 1,
            src: self.labeler_did.clone(),
            uri: subject_uri.to_string(),
            cid: subject_cid.map(|s| s.to_string()),
            val: value.to_string(),
            neg: false,
            cts: now_iso8601(),
            exp: None,
        };

        let sig = self.signer.sign(&label)?;
        let signed = SignedLabel { label, sig };

        self.store.insert(&signed).await?;
        Ok(signed)
    }

    /// Negate (remove) a previously issued label.
    ///
    /// Creates a new label entry with `neg: true`, indicating that the
    /// specified label value no longer applies to the subject.
    pub async fn negate_label(
        &self,
        subject_uri: &str,
        value: LabelValue,
        subject_cid: Option<&str>,
    ) -> anyhow::Result<SignedLabel> {
        let label = Label {
            ver: 1,
            src: self.labeler_did.clone(),
            uri: subject_uri.to_string(),
            cid: subject_cid.map(|s| s.to_string()),
            val: value.to_string(),
            neg: true,
            cts: now_iso8601(),
            exp: None,
        };

        let sig = self.signer.sign(&label)?;
        let signed = SignedLabel { label, sig };

        self.store.insert(&signed).await?;
        Ok(signed)
    }

    /// Query labels for a subject URI.
    pub async fn query_labels(&self, uri: &str) -> anyhow::Result<Vec<SignedLabel>> {
        self.store.query_by_uri(uri).await
    }

    /// Query labels since a cursor for subscription streaming.
    ///
    /// Returns `(row_id, signed_label)` pairs so callers can track the cursor
    /// position for subsequent requests.
    pub async fn query_since(
        &self,
        cursor: i64,
        limit: i64,
    ) -> anyhow::Result<Vec<(i64, SignedLabel)>> {
        self.store.query_since(cursor, limit).await
    }
}

/// Get the current time as an ISO 8601 string.
///
/// Uses `std::time::SystemTime` to avoid adding chrono as a dependency.
/// The output format is `{epoch_seconds}Z` — a simplified representation.
// TODO: Use proper ISO 8601 formatting (YYYY-MM-DDTHH:MM:SSZ) in production,
// either via chrono or a manual computation from epoch seconds.
fn now_iso8601() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}Z", now.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::LabelSigner;
    use sqlx::SqlitePool;

    fn test_signer() -> LabelSigner {
        LabelSigner::new(b"test-key".to_vec())
    }

    async fn setup_service() -> LabelService {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let svc = LabelService::new(db, test_signer(), "did:plc:test-labeler".to_string());
        svc.migrate().await.unwrap();
        svc
    }

    #[test]
    fn now_iso8601_produces_nonempty_string() {
        let ts = now_iso8601();
        assert!(!ts.is_empty());
        assert!(ts.ends_with('Z'));
    }

    #[tokio::test]
    async fn test_new_and_migrate() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let svc = LabelService::new(db, test_signer(), "did:plc:labeler".to_string());
        let result = svc.migrate().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_label() {
        let svc = setup_service().await;
        let uri = "at://did:plc:user/app.bsky.feed.post/abc";

        let signed = svc.create_label(uri, LabelValue::Spam, None).await.unwrap();

        assert_eq!(signed.label.src, "did:plc:test-labeler");
        assert_eq!(signed.label.uri, uri);
        assert_eq!(signed.label.val, "spam");
        assert!(!signed.label.neg);
        assert!(!signed.sig.is_empty());

        // Query back from the store.
        let labels = svc.query_labels(uri).await.unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].label.val, "spam");
        assert_eq!(labels[0].label.src, "did:plc:test-labeler");
    }

    #[tokio::test]
    async fn test_negate_label() {
        let svc = setup_service().await;
        let uri = "at://did:plc:user/app.bsky.feed.post/abc";

        let negated = svc.negate_label(uri, LabelValue::Porn, None).await.unwrap();

        assert!(negated.label.neg);
        assert_eq!(negated.label.val, "porn");
        assert!(!negated.sig.is_empty());

        let labels = svc.query_labels(uri).await.unwrap();
        assert_eq!(labels.len(), 1);
        assert!(labels[0].label.neg);
    }

    #[tokio::test]
    async fn test_create_and_query_multiple() {
        let svc = setup_service().await;
        let uri_a = "at://did:plc:user/post/a";
        let uri_b = "at://did:plc:user/post/b";

        svc.create_label(uri_a, LabelValue::Spam, None)
            .await
            .unwrap();
        svc.create_label(uri_a, LabelValue::Nudity, None)
            .await
            .unwrap();
        svc.create_label(uri_b, LabelValue::Impersonation, None)
            .await
            .unwrap();

        let labels_a = svc.query_labels(uri_a).await.unwrap();
        assert_eq!(labels_a.len(), 2);
        assert_eq!(labels_a[0].label.val, "spam");
        assert_eq!(labels_a[1].label.val, "nudity");

        let labels_b = svc.query_labels(uri_b).await.unwrap();
        assert_eq!(labels_b.len(), 1);
        assert_eq!(labels_b[0].label.val, "impersonation");
    }

    #[tokio::test]
    async fn test_query_since_ordering() {
        let svc = setup_service().await;
        let uri = "at://did:plc:user/post/1";

        svc.create_label(uri, LabelValue::Spam, None).await.unwrap();
        svc.create_label(uri, LabelValue::Porn, None).await.unwrap();
        svc.create_label(uri, LabelValue::Custom("custom".into()), None)
            .await
            .unwrap();

        let results = svc.query_since(0, 10).await.unwrap();
        assert_eq!(results.len(), 3);

        // IDs should be monotonically increasing.
        assert!(results[0].0 < results[1].0);
        assert!(results[1].0 < results[2].0);

        // Values should match insertion order.
        assert_eq!(results[0].1.label.val, "spam");
        assert_eq!(results[1].1.label.val, "porn");
        assert_eq!(results[2].1.label.val, "custom");
    }
}
