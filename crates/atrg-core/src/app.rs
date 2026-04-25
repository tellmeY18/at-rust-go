//! The `AtrgApp` builder — the main entry point for assembling and running an atrg server.
//!
//! A minimal application looks like this:
//!
//! ```rust,no_run
//! use atrg_core::AtrgApp;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     AtrgApp::new()
//!         .mount(axum::Router::new())
//!         .run()
//!         .await
//! }
//! ```

use std::path::Path;
use std::sync::Arc;

use axum::response::IntoResponse;
use axum::routing::any;
use axum::Router;
use futures::future::BoxFuture;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::cors::build_cors_layer;
use crate::error::AtrgError;
use crate::state::AppState;

/// A cleanup task function that receives a database pool and spawns
/// background maintenance work (e.g. expired session cleanup).
type CleanupFn = Box<dyn FnOnce(SqlitePool) + Send>;

/// The application builder. Accumulates user routers and configuration,
/// then boots the full server when [`AtrgApp::run`] is called.
pub struct AtrgApp {
    router: Router<AppState>,
    /// Built-in routes (auth, well-known, etc.) merged before user routes.
    builtin_router: Option<Router<AppState>>,
    /// Optional cleanup task spawner (e.g. session/oauth-state cleanup).
    cleanup_fn: Option<CleanupFn>,
    /// Jetstream event handler registered via [`AtrgApp::on_event`].
    event_handler: Option<atrg_stream::EventHandler<AppState>>,
}

impl AtrgApp {
    /// Create a new, empty application builder.
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            builtin_router: None,
            cleanup_fn: None,
            event_handler: None,
        }
    }

    /// Mount an additional [`axum::Router`] into the application.
    ///
    /// Routes are merged, so multiple calls to `mount` accumulate routes.
    pub fn mount(mut self, router: Router<AppState>) -> Self {
        self.router = self.router.merge(router);
        self
    }

    /// Register built-in auth routes (OAuth login/callback/logout, client-metadata, well-known).
    ///
    /// The supplied router is merged **before** user routes so that atrg's
    /// built-in endpoints take precedence.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use atrg_core::AtrgApp;
    ///
    /// AtrgApp::new()
    ///     .with_auth_routes(atrg_auth::routes::auth_router())
    ///     // ...
    /// # ;
    /// ```
    pub fn with_auth_routes(mut self, router: Router<AppState>) -> Self {
        self.builtin_router = Some(router);
        self
    }

    /// Register a background cleanup task that is spawned after the server
    /// starts. Typically used for periodic session / OAuth-state expiry.
    ///
    /// The callback receives the [`SqlitePool`] and is expected to call
    /// `tokio::spawn` internally.
    pub fn with_cleanup_task<F>(mut self, f: F) -> Self
    where
        F: FnOnce(SqlitePool) + Send + 'static,
    {
        self.cleanup_fn = Some(Box::new(f));
        self
    }

    /// Register a Jetstream event handler.
    ///
    /// The handler is called for every event received from the Jetstream
    /// firehose. It is spawned as a background task inside [`AtrgApp::run`]
    /// when `[jetstream]` is present in `atrg.toml`.
    pub fn on_event<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(atrg_stream::JetstreamEvent, AppState) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.event_handler = Some(Arc::new(move |event, state| {
            Box::pin(handler(event, state)) as BoxFuture<'static, anyhow::Result<()>>
        }));
        self
    }

    /// Boot the server.
    ///
    /// This is the **only** async entry point. It:
    ///
    /// 1. Initialises tracing (respects `RUST_LOG`).
    /// 2. Loads `atrg.toml` (or `$ATRG_CONFIG`).
    /// 3. Connects to SQLite and runs migrations.
    /// 4. Builds [`AppState`] (including the identity resolver).
    /// 5. Assembles the Axum router with CORS, tracing, and a JSON 404 fallback.
    /// 6. Spawns optional cleanup tasks.
    /// 7. Binds a TCP listener and serves.
    pub async fn run(self) -> anyhow::Result<()> {
        // 1. Init tracing -------------------------------------------------------
        let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(
                    "info,atrg_core=debug,atrg_db=debug,atrg_auth=debug,atrg_cli=debug,tower_http=debug",
                )
            });

        // If another test or binary already initialised the subscriber, silently
        // ignore the error rather than panicking.
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .try_init();

        // 2. Load config --------------------------------------------------------
        let config_path = std::env::var("ATRG_CONFIG").unwrap_or_else(|_| "./atrg.toml".into());
        tracing::info!(path = %config_path, "loading configuration");
        let config = Config::load(&config_path)?;
        let config = Arc::new(config);

        // 3. Connect DB + migrations --------------------------------------------
        let db = atrg_db::connect(&config.database.url).await?;
        atrg_db::run_internal_migrations(&db).await?;

        let user_migrations = Path::new("./migrations");
        if user_migrations.is_dir() {
            atrg_db::run_user_migrations(&db, user_migrations).await?;
        }

        // 4. Build HTTP client --------------------------------------------------
        let http = reqwest::Client::builder()
            .user_agent(format!("atrg/{}", crate::version()))
            .build()?;

        // 4b. Build identity resolver -------------------------------------------
        let identity = Arc::new(atrg_identity::IdentityResolver::with_defaults(http.clone()));

        // 5. Assemble AppState --------------------------------------------------
        let state = AppState {
            config: config.clone(),
            db,
            http,
            identity,
        };

        // 6. Build CORS layer ---------------------------------------------------
        let cors = build_cors_layer(&config.app.cors_origins);

        // 7. Build router -------------------------------------------------------
        let mut router = Router::new();

        // Built-in health endpoints
        router = router
            .route("/healthz", axum::routing::get(crate::health::healthz))
            .route("/readyz", axum::routing::get(crate::health::readyz));

        // Merge built-in auth routes (if registered via with_auth_routes)
        if let Some(builtin) = self.builtin_router {
            router = router.merge(builtin);
        }

        let mut router = router
            // User routes
            .merge(self.router)
            // JSON 404 fallback for any unmatched path
            .fallback(any(fallback_not_found))
            .with_state(state.clone())
            .layer(cors)
            .layer(axum::middleware::from_fn(
                crate::request_id::request_id_middleware,
            ))
            .layer(TraceLayer::new_for_http());

        // Apply security headers in non-development mode
        if config.app.environment != "development" {
            router = router.layer(axum::middleware::from_fn(
                crate::security::security_headers_middleware,
            ));
        }

        // 8. Jetstream ----------------------------------------------------------
        if let Some(ref js_config) = config.jetstream {
            if let Some(handler) = self.event_handler {
                let stream_config = atrg_stream::StreamConfig {
                    host: js_config.host.clone(),
                    collections: js_config.collections.clone(),
                    zstd_dict: js_config.zstd_dict.clone(),
                    channel_capacity: js_config.channel_capacity,
                    max_lag_events: js_config.max_lag_events,
                };
                atrg_stream::spawn_consumer(&stream_config, state.clone(), handler).await?;
            } else {
                tracing::warn!("jetstream configured but no on_event handler registered");
            }
        }

        // 8b. Spawn cleanup task (if registered) --------------------------------
        if let Some(cleanup) = self.cleanup_fn {
            cleanup(state.db.clone());
        }

        // 9. Serve --------------------------------------------------------------
        let addr = format!("{}:{}", config.app.host, config.app.port);
        tracing::info!(addr = %addr, name = %config.app.name, "at-rust-go API serving");
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, router).await?;

        Ok(())
    }
}

impl Default for AtrgApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Global fallback handler — returns a JSON 404 for any unmatched route.
async fn fallback_not_found() -> impl IntoResponse {
    AtrgError::NotFound
}

/// Build a fully-wired [`Router`] for testing purposes (no TCP listener).
///
/// This is **not** part of the public API — it exists so integration tests
/// can exercise the full middleware stack via `tower::ServiceExt::oneshot`.
#[cfg(test)]
pub(crate) fn build_test_router(user_router: Router<AppState>, state: AppState) -> Router {
    build_test_router_with_auth(None, user_router, state)
}

/// Like [`build_test_router`], but also merges optional auth routes.
#[cfg(test)]
pub(crate) fn build_test_router_with_auth(
    auth_router: Option<Router<AppState>>,
    user_router: Router<AppState>,
    state: AppState,
) -> Router {
    let cors = build_cors_layer(&state.config.app.cors_origins);

    let mut router = Router::new();
    if let Some(auth) = auth_router {
        router = router.merge(auth);
    }

    router
        .merge(user_router)
        .fallback(any(fallback_not_found))
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, AuthConfig, Config, DatabaseConfig};
    use axum::body::Body;
    use axum::routing::get;
    use axum::Json;
    use http_body_util::BodyExt;
    use hyper::Request;
    use tower::ServiceExt;

    /// Build an [`AppState`] backed by an in-memory SQLite database.
    async fn test_state() -> AppState {
        let db = atrg_db::connect("sqlite::memory:").await.unwrap();
        atrg_db::run_internal_migrations(&db).await.unwrap();

        let config = Config {
            app: AppConfig {
                name: "test-app".into(),
                host: "127.0.0.1".into(),
                port: 3000,
                secret_key: "a]3)FRd9-x4bQ7Y!kN2mW#pL8v$Tz0cS".into(),
                cors_origins: vec![],
                environment: "development".into(),
            },
            auth: AuthConfig {
                client_id: "http://localhost:3000/client-metadata.json".into(),
                redirect_uri: "http://localhost:3000/auth/callback".into(),
                scope: "atproto transition:generic".into(),
            },
            database: DatabaseConfig {
                url: "sqlite::memory:".into(),
            },
            jetstream: None,
        };

        AppState {
            config: Arc::new(config),
            db,
            http: reqwest::Client::new(),
            identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
                reqwest::Client::new(),
            )),
        }
    }

    /// Helper: extract the full body bytes from a response.
    async fn body_bytes(response: axum::response::Response) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec()
    }

    #[test]
    fn atrg_app_default_is_new() {
        // Just ensure Default compiles and doesn't panic.
        let _app = AtrgApp::default();
    }

    #[test]
    fn on_event_sets_handler() {
        let app = AtrgApp::new().on_event(|_event, _state| async { Ok(()) });
        assert!(app.event_handler.is_some());
    }

    #[tokio::test]
    async fn mount_ping_returns_200_json() {
        let state = test_state().await;

        let user_router: Router<AppState> = Router::new().route(
            "/ping",
            get(|| async { Json(serde_json::json!({"pong": true})) }),
        );

        let app = build_test_router(user_router, state);

        let request = Request::builder().uri("/ping").body(Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let ct = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            ct.contains("application/json"),
            "expected application/json, got {ct}"
        );

        let bytes = body_bytes(response).await;
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["pong"], true);
    }

    #[tokio::test]
    async fn unknown_route_returns_404_json() {
        let state = test_state().await;
        let app = build_test_router(Router::new(), state);

        let request = Request::builder()
            .uri("/does-not-exist")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 404);

        let ct = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            ct.contains("application/json"),
            "expected application/json, got {ct}"
        );

        let bytes = body_bytes(response).await;
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "not_found");
        assert_eq!(body["message"], "Not found");
    }

    #[tokio::test]
    async fn multiple_mounts_accumulate_routes() {
        let state = test_state().await;

        let r1: Router<AppState> = Router::new().route(
            "/a",
            get(|| async { Json(serde_json::json!({"route": "a"})) }),
        );
        let r2: Router<AppState> = Router::new().route(
            "/b",
            get(|| async { Json(serde_json::json!({"route": "b"})) }),
        );

        let app = build_test_router(r1.merge(r2), state);

        let resp_a = app
            .clone()
            .oneshot(Request::builder().uri("/a").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp_a.status(), 200);

        let resp_b = app
            .oneshot(Request::builder().uri("/b").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp_b.status(), 200);
    }

    #[tokio::test]
    async fn with_auth_routes_merges_builtin() {
        let state = test_state().await;

        // Simulate an auth router with a test endpoint
        let auth_router: Router<AppState> = Router::new().route(
            "/auth/test",
            get(|| async { Json(serde_json::json!({"auth": true})) }),
        );

        let user_router: Router<AppState> = Router::new().route(
            "/ping",
            get(|| async { Json(serde_json::json!({"pong": true})) }),
        );

        let app = build_test_router_with_auth(Some(auth_router), user_router, state);

        // Auth route works
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/auth/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // User route also works
        let resp = app
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }
}
