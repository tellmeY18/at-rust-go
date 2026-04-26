//! Axum routes for labeler XRPC endpoints.

use std::sync::Arc;

use axum::extract::Query;
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};

use crate::label::LabelService;
use crate::types::SignedLabel;
use atrg_core::AppState;
use atrg_xrpc::{XrpcError, XrpcErrorName};

/// Build the labeler router with the label query endpoint.
///
/// Registers:
/// - `GET /xrpc/com.atproto.label.queryLabels`
///
/// The returned router must be merged into the application router.
/// The `label_service` is injected via an Axum [`Extension`] layer so that
/// handlers can access it without polluting [`AppState`].
pub fn labeler_routes(label_service: Arc<LabelService>) -> Router<AppState> {
    Router::new()
        .route(
            "/xrpc/com.atproto.label.queryLabels",
            get(query_labels_handler),
        )
        .layer(Extension(label_service))
}

/// Query parameters for `com.atproto.label.queryLabels`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryLabelsParams {
    /// URI patterns to match against label subjects.
    /// Multiple values can be provided as repeated query parameters.
    #[serde(default)]
    uri_patterns: Vec<String>,
    /// Filter by label source DIDs.
    #[serde(default)]
    sources: Vec<String>,
    /// Cursor for pagination (opaque string representing the last seen row id).
    cursor: Option<String>,
    /// Maximum number of labels to return (default 50, max 250).
    limit: Option<i64>,
}

/// Response body for `com.atproto.label.queryLabels`.
#[derive(Debug, Serialize)]
struct QueryLabelsResponse {
    /// The matching labels.
    labels: Vec<SignedLabel>,
    /// Cursor for the next page, if more results are available.
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
}

/// Handler for `GET /xrpc/com.atproto.label.queryLabels`.
///
/// Supports filtering by URI patterns and source DIDs. When a cursor is
/// provided, returns labels created after that cursor position. Results
/// are ordered by creation time (ascending).
async fn query_labels_handler(
    Extension(service): Extension<Arc<LabelService>>,
    Query(params): Query<QueryLabelsParams>,
) -> Result<Json<QueryLabelsResponse>, XrpcError> {
    let limit = params.limit.unwrap_or(50).clamp(1, 250);

    // If a cursor is provided, use cursor-based pagination via query_since.
    if let Some(ref cursor_str) = params.cursor {
        let cursor_id: i64 = cursor_str.parse().map_err(|_| XrpcError {
            name: XrpcErrorName::InvalidRequest,
            message: "Invalid cursor value".to_string(),
        })?;

        let results = service.query_since(cursor_id, limit).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to query labels since cursor");
            XrpcError {
                name: XrpcErrorName::InternalServerError,
                message: "Failed to query labels".to_string(),
            }
        })?;

        let next_cursor = results.last().map(|(id, _)| id.to_string());
        let mut labels: Vec<SignedLabel> = results.into_iter().map(|(_, label)| label).collect();

        // Apply source filter if provided.
        if !params.sources.is_empty() {
            labels.retain(|l| params.sources.contains(&l.label.src));
        }

        return Ok(Json(QueryLabelsResponse {
            labels,
            cursor: next_cursor,
        }));
    }

    // No cursor — query by URI patterns.
    if params.uri_patterns.is_empty() {
        return Err(XrpcError {
            name: XrpcErrorName::InvalidRequest,
            message: "At least one uriPatterns value or a cursor is required".to_string(),
        });
    }

    let mut all_labels = Vec::new();
    for pattern in &params.uri_patterns {
        let labels = service.query_labels(pattern).await.map_err(|e| {
            tracing::error!(error = %e, uri = %pattern, "Failed to query labels by URI");
            XrpcError {
                name: XrpcErrorName::InternalServerError,
                message: "Failed to query labels".to_string(),
            }
        })?;
        all_labels.extend(labels);
    }

    // Apply source filter if provided.
    if !params.sources.is_empty() {
        all_labels.retain(|l| params.sources.contains(&l.label.src));
    }

    // Truncate to limit.
    all_labels.truncate(limit as usize);

    Ok(Json(QueryLabelsResponse {
        labels: all_labels,
        cursor: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::LabelSigner;
    use crate::types::LabelValue;
    use atrg_core::config::{AppConfig, AuthConfig, Config, DatabaseConfig};
    use axum::body::Body;
    use http_body_util::BodyExt;
    use hyper::Request;
    use tower::ServiceExt;

    async fn test_state() -> AppState {
        let db = atrg_db::connect("sqlite::memory:").await.unwrap();
        atrg_db::run_internal_migrations(&db).await.unwrap();
        AppState {
            config: Arc::new(Config {
                app: AppConfig {
                    name: "test".into(),
                    host: "127.0.0.1".into(),
                    port: 3000,
                    secret_key: "a]3)FRd9-x4bQ7Y!kN2mW#pL8v$Tz0cS".into(),
                    cors_origins: vec![],
                    environment: "development".into(),
                },
                auth: AuthConfig::default(),
                database: DatabaseConfig::default(),
                jetstream: None,
            }),
            db,
            http: reqwest::Client::new(),
            identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
                reqwest::Client::new(),
            )),
        }
    }

    async fn setup_service() -> Arc<LabelService> {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        let signer = LabelSigner::new(b"test-key".to_vec());
        let svc = LabelService::new(db, signer, "did:plc:test-labeler".to_string());
        svc.migrate().await.unwrap();
        Arc::new(svc)
    }

    async fn build_app(service: Arc<LabelService>) -> axum::Router {
        let state = test_state().await;
        labeler_routes(service).with_state(state)
    }

    fn parse_body(bytes: &[u8]) -> serde_json::Value {
        serde_json::from_slice(bytes).unwrap()
    }

    /// Helper: fetch the endpoint and return (status, parsed JSON body).
    async fn get_labels(app: axum::Router, query: &str) -> (u16, serde_json::Value) {
        let uri = format!("/xrpc/com.atproto.label.queryLabels{}", query);
        let resp = app
            .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let bytes = Body::new(resp.into_body())
            .collect()
            .await
            .unwrap()
            .to_bytes();
        (status, parse_body(&bytes))
    }

    // --- Cursor-based tests (the primary pagination path) ---

    #[tokio::test]
    async fn test_query_labels_returns_labels() {
        let svc = setup_service().await;
        svc.create_label("at://did:plc:user/post/1", LabelValue::Spam, None)
            .await
            .unwrap();
        svc.create_label("at://did:plc:user/post/1", LabelValue::Porn, None)
            .await
            .unwrap();

        let app = build_app(svc).await;
        let (status, body) = get_labels(app, "?cursor=0&limit=10").await;

        assert_eq!(status, 200);
        let labels = body["labels"].as_array().unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0]["val"], "spam");
        assert_eq!(labels[1]["val"], "porn");
        // Cursor should be present when there are results.
        assert!(body["cursor"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_query_labels_with_cursor() {
        let svc = setup_service().await;
        for i in 0..5 {
            svc.create_label(
                "at://did:plc:user/post/1",
                LabelValue::Custom(format!("val-{}", i)),
                None,
            )
            .await
            .unwrap();
        }

        // First page: cursor=0, limit=3.
        let app = build_app(Arc::clone(&svc)).await;
        let (status, body) = get_labels(app, "?cursor=0&limit=3").await;

        assert_eq!(status, 200);
        let labels = body["labels"].as_array().unwrap();
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[0]["val"], "val-0");
        assert_eq!(labels[2]["val"], "val-2");
        let cursor = body["cursor"].as_str().unwrap();

        // Second page using returned cursor.
        let app2 = build_app(svc).await;
        let (status2, body2) = get_labels(app2, &format!("?cursor={}&limit=3", cursor)).await;

        assert_eq!(status2, 200);
        let labels2 = body2["labels"].as_array().unwrap();
        assert_eq!(labels2.len(), 2);
        assert_eq!(labels2[0]["val"], "val-3");
        assert_eq!(labels2[1]["val"], "val-4");
    }

    #[tokio::test]
    async fn test_query_labels_empty() {
        let svc = setup_service().await;
        let app = build_app(svc).await;

        // No labels inserted — cursor-based query returns empty list.
        let (status, body) = get_labels(app, "?cursor=0&limit=10").await;

        assert_eq!(status, 200);
        let labels = body["labels"].as_array().unwrap();
        assert!(labels.is_empty());
        // No cursor when there are no results.
        assert!(body.get("cursor").is_none() || body["cursor"].is_null());
    }

    #[tokio::test]
    async fn test_query_labels_default_limit() {
        let svc = setup_service().await;
        // Insert 60 labels — more than the default limit of 50.
        for i in 0..60 {
            svc.create_label(
                "at://did:plc:user/post/1",
                LabelValue::Custom(format!("v{}", i)),
                None,
            )
            .await
            .unwrap();
        }

        // No explicit limit — should default to 50.
        let app = build_app(svc).await;
        let (status, body) = get_labels(app, "?cursor=0").await;

        assert_eq!(status, 200);
        let labels = body["labels"].as_array().unwrap();
        assert_eq!(labels.len(), 50);
    }

    // --- Error cases ---

    #[tokio::test]
    async fn test_query_labels_no_patterns_no_cursor_returns_error() {
        let svc = setup_service().await;
        let app = build_app(svc).await;

        let (status, body) = get_labels(app, "").await;

        assert_eq!(status, 400);
        assert_eq!(body["error"], "InvalidRequest");
    }

    #[tokio::test]
    async fn test_query_labels_invalid_cursor_returns_error() {
        let svc = setup_service().await;
        let app = build_app(svc).await;

        let (status, body) = get_labels(app, "?cursor=not-a-number").await;

        assert_eq!(status, 400);
        assert_eq!(body["error"], "InvalidRequest");
        assert!(body["message"].as_str().unwrap().contains("Invalid cursor"));
    }

    // --- Limit clamping ---

    #[tokio::test]
    async fn test_query_labels_limit_clamped_to_max_250() {
        let svc = setup_service().await;
        for i in 0..260 {
            svc.create_label(
                "at://did:plc:user/post/1",
                LabelValue::Custom(format!("v{}", i)),
                None,
            )
            .await
            .unwrap();
        }

        let app = build_app(svc).await;
        let (status, body) = get_labels(app, "?cursor=0&limit=999").await;

        assert_eq!(status, 200);
        let labels = body["labels"].as_array().unwrap();
        // limit is clamped to 250.
        assert_eq!(labels.len(), 250);
    }

    #[tokio::test]
    async fn test_query_labels_limit_clamped_to_min_1() {
        let svc = setup_service().await;
        svc.create_label("at://did:plc:user/post/1", LabelValue::Spam, None)
            .await
            .unwrap();

        let app = build_app(svc).await;
        let (status, body) = get_labels(app, "?cursor=0&limit=0").await;

        assert_eq!(status, 200);
        let labels = body["labels"].as_array().unwrap();
        assert_eq!(labels.len(), 1);
    }

    // --- Response shape ---

    #[tokio::test]
    async fn test_query_labels_response_contains_label_fields() {
        let svc = setup_service().await;
        svc.create_label("at://did:plc:user/post/1", LabelValue::Spam, None)
            .await
            .unwrap();

        let app = build_app(svc).await;
        let (status, body) = get_labels(app, "?cursor=0&limit=1").await;

        assert_eq!(status, 200);
        let label = &body["labels"][0];
        assert_eq!(label["src"], "did:plc:test-labeler");
        assert_eq!(label["uri"], "at://did:plc:user/post/1");
        assert_eq!(label["val"], "spam");
        assert_eq!(label["neg"], false);
        assert_eq!(label["ver"], 1);
        // sig should be present and non-empty.
        assert!(!label["sig"].as_str().unwrap().is_empty());
        // cts should be present.
        assert!(label["cts"].as_str().is_some());
    }
}
