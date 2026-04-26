//! Axum route handlers for feed generator XRPC endpoints.
//!
//! Provides handlers for:
//! - `app.bsky.feed.describeFeedGenerator` — lists available feeds
//! - `app.bsky.feed.getFeedSkeleton` — returns a feed skeleton for a given feed

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};

use crate::handler::{FeedHandler, FeedRequest};
use crate::types::{DescribeFeedGeneratorResponse, FeedConfig, FeedDescription, FeedSkeleton};
use atrg_auth::AuthUser;
use atrg_core::AppState;
use atrg_xrpc::XrpcError;

/// Shared state for feed routes, injected via Axum extension.
#[derive(Clone)]
pub(crate) struct FeedState {
    /// DID of the feed generator service.
    pub(crate) did: String,
    /// Registered feeds: id -> (config, handler).
    pub(crate) feeds: Arc<HashMap<String, (FeedConfig, FeedHandler)>>,
}

/// Build the feed generator router with `describeFeedGenerator` and
/// `getFeedSkeleton` XRPC endpoints.
pub fn build_router(
    did: String,
    feeds: HashMap<String, (FeedConfig, FeedHandler)>,
) -> Router<AppState> {
    let feed_state = FeedState {
        did,
        feeds: Arc::new(feeds),
    };

    Router::new()
        .route(
            "/xrpc/app.bsky.feed.describeFeedGenerator",
            get(describe_feed_generator),
        )
        .route(
            "/xrpc/app.bsky.feed.getFeedSkeleton",
            get(get_feed_skeleton),
        )
        .layer(axum::Extension(feed_state))
}

/// Handler for `app.bsky.feed.describeFeedGenerator`.
///
/// Returns a list of all feeds served by this generator.
async fn describe_feed_generator(
    axum::Extension(feed_state): axum::Extension<FeedState>,
) -> Result<Json<DescribeFeedGeneratorResponse>, XrpcError> {
    let feeds = feed_state
        .feeds
        .iter()
        .map(|(id, (_config, _handler))| FeedDescription {
            uri: format!("at://{}/app.bsky.feed.generator/{}", feed_state.did, id),
            cid: None,
        })
        .collect();

    Ok(Json(DescribeFeedGeneratorResponse {
        did: feed_state.did.clone(),
        feeds,
    }))
}

/// Query parameters for `getFeedSkeleton`.
#[derive(serde::Deserialize)]
struct GetSkeletonParams {
    /// AT-URI of the feed being requested.
    feed: String,
    /// Maximum number of items to return (default 50).
    #[serde(default = "default_limit")]
    limit: usize,
    /// Pagination cursor.
    cursor: Option<String>,
}

/// Default limit for feed skeleton requests.
fn default_limit() -> usize {
    50
}

/// Handler for `app.bsky.feed.getFeedSkeleton`.
///
/// Extracts the feed ID from the AT-URI, looks up the registered handler,
/// and delegates skeleton generation.
async fn get_feed_skeleton(
    State(app_state): State<AppState>,
    axum::Extension(feed_state): axum::Extension<FeedState>,
    AuthUser(user): AuthUser,
    Query(params): Query<GetSkeletonParams>,
) -> Result<Json<FeedSkeleton>, XrpcError> {
    // Extract the feed ID from the AT-URI.
    // Expected format: at://did:xxx/app.bsky.feed.generator/feed-id
    let feed_id = params
        .feed
        .rsplit('/')
        .next()
        .ok_or_else(|| atrg_xrpc::xrpc_invalid_request("invalid feed URI"))?;

    let (_config, handler) = feed_state
        .feeds
        .get(feed_id)
        .ok_or_else(|| atrg_xrpc::xrpc_not_found(format!("feed '{}' not found", feed_id)))?;

    // Clamp limit to [1, 100]
    let limit = params.limit.clamp(1, 100);

    let request = FeedRequest {
        feed: params.feed,
        cursor: params.cursor,
        limit,
        requester_did: user.map(|u| u.did),
    };

    let skeleton = handler(request, app_state).await?;
    Ok(Json(skeleton))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::FeedGenerator;
    use crate::types::SkeletonItem;
    use atrg_core::config::{AppConfig, AuthConfig, Config, DatabaseConfig};
    use axum::body::Body;
    use http_body_util::BodyExt;
    use hyper::Request;
    use std::sync::Arc;
    use tower::ServiceExt;

    /// Build a test `AppState` with in-memory SQLite.
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

    /// A simple test feed handler that returns hardcoded posts.
    async fn mock_handler(req: FeedRequest, _state: AppState) -> Result<FeedSkeleton, XrpcError> {
        let items: Vec<SkeletonItem> = (0..req.limit)
            .map(|i| SkeletonItem::new(format!("at://did:plc:test/app.bsky.feed.post/{}", i)))
            .collect();
        Ok(FeedSkeleton {
            feed: items,
            cursor: Some("next-cursor".to_string()),
        })
    }

    /// Build a test app with one registered feed.
    async fn test_app() -> (axum::Router, AppState) {
        let state = test_state().await;
        let router = FeedGenerator::new("did:web:feeds.test")
            .feed("test-feed", "Test Feed", Some("A test feed"), mock_handler)
            .into_router()
            .with_state(state.clone());
        (router, state)
    }

    #[tokio::test]
    async fn describe_returns_registered_feeds() {
        let (app, _state) = test_app().await;
        let resp = app
            .oneshot(
                Request::get("/xrpc/app.bsky.feed.describeFeedGenerator")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let bytes = Body::new(resp.into_body())
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(body["did"], "did:web:feeds.test");
        let feeds = body["feeds"].as_array().unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(
            feeds[0]["uri"],
            "at://did:web:feeds.test/app.bsky.feed.generator/test-feed"
        );
    }

    #[tokio::test]
    async fn get_skeleton_returns_feed_items() {
        let (app, _state) = test_app().await;
        let uri = "/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:web:feeds.test/app.bsky.feed.generator/test-feed&limit=3";
        let resp = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let bytes = Body::new(resp.into_body())
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        let items = body["feed"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["post"], "at://did:plc:test/app.bsky.feed.post/0");
        assert_eq!(body["cursor"], "next-cursor");
    }

    #[tokio::test]
    async fn get_skeleton_unknown_feed_returns_404() {
        let (app, _state) = test_app().await;
        let uri = "/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:web:feeds.test/app.bsky.feed.generator/nonexistent";
        let resp = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), 404);
        let bytes = Body::new(resp.into_body())
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "NotFound");
    }

    #[tokio::test]
    async fn get_skeleton_clamps_limit_to_max_100() {
        let (app, _state) = test_app().await;
        let uri = "/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:web:feeds.test/app.bsky.feed.generator/test-feed&limit=999";
        let resp = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let bytes = Body::new(resp.into_body())
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        let items = body["feed"].as_array().unwrap();
        assert_eq!(items.len(), 100);
    }

    #[tokio::test]
    async fn get_skeleton_clamps_limit_to_min_1() {
        let (app, _state) = test_app().await;
        let uri = "/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:web:feeds.test/app.bsky.feed.generator/test-feed&limit=0";
        let resp = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let bytes = Body::new(resp.into_body())
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        let items = body["feed"].as_array().unwrap();
        assert_eq!(items.len(), 1);
    }

    #[tokio::test]
    async fn describe_with_multiple_feeds() {
        let state = test_state().await;
        let router = FeedGenerator::new("did:web:feeds.test")
            .feed("feed-a", "Feed A", None, mock_handler)
            .feed("feed-b", "Feed B", Some("Second feed"), mock_handler)
            .into_router()
            .with_state(state);

        let resp = router
            .oneshot(
                Request::get("/xrpc/app.bsky.feed.describeFeedGenerator")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let bytes = Body::new(resp.into_body())
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        let feeds = body["feeds"].as_array().unwrap();
        assert_eq!(feeds.len(), 2);
    }
}
