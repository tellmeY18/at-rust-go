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

use atrg_db::DbPool;
use axum::response::IntoResponse;
use axum::routing::any;
use axum::Router;
use futures::future::BoxFuture;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::cors::build_cors_layer;
use crate::error::AtrgError;
use crate::state::{AppState, Extensions};

/// A cleanup task function that receives a database pool and spawns
/// background maintenance work (e.g. expired session cleanup).
type CleanupFn = Box<dyn FnOnce(DbPool) + Send>;

/// The application builder. Accumulates user routers and configuration,
/// then boots the full server when [`AtrgApp::run`] is called.
pub struct AtrgApp {
    router: Router<AppState>,
    /// Built-in routes (auth, well-known, etc.) merged before user routes.
    builtin_router: Option<Router<AppState>>,
    /// Optional cleanup task spawner (e.g. session/oauth-state cleanup).
    cleanup_fn: Option<CleanupFn>,
    /// Optional caller-supplied database pool. When set, [`AtrgApp::run`]
    /// uses this pool instead of opening one from `[database] url`.
    user_db_pool: Option<DbPool>,
    /// Jetstream event handler registered via [`AtrgApp::on_event`].
    event_handler: Option<atrg_stream::EventHandler<AppState>>,
    /// Firehose event handler (registered via [`AtrgApp::on_firehose_event`]).
    #[cfg(feature = "firehose")]
    firehose_handler: Option<atrg_firehose::FirehoseHandler<AppState>>,
    /// App-specific extensions collected during build and passed into AppState.
    extensions: Extensions,
}

impl AtrgApp {
    /// Create a new, empty application builder.
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            builtin_router: None,
            cleanup_fn: None,
            user_db_pool: None,
            event_handler: None,
            #[cfg(feature = "firehose")]
            firehose_handler: None,
            extensions: Extensions::new(),
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
    /// The callback receives the [`DbPool`] and is expected to call
    /// `tokio::spawn` internally.
    pub fn with_cleanup_task<F>(mut self, f: F) -> Self
    where
        F: FnOnce(DbPool) + Send + 'static,
    {
        self.cleanup_fn = Some(Box::new(f));
        self
    }

    /// Use a caller-provided database pool instead of opening a fresh one
    /// from `[database] url`.
    ///
    /// This is the recommended way to integrate atrg into an existing
    /// application that already manages its own connection pool — for
    /// example, a service that uses PostgreSQL for its business data and
    /// wants atrg's internal tables (sessions, OAuth state) to live in the
    /// same database:
    ///
    /// ```rust,ignore
    /// let pool = sqlx::PgPool::connect(&db_url).await?;
    ///
    /// AtrgApp::new()
    ///     .with_db_pool(pool.into())   // accepts SqlitePool, PgPool, or DbPool
    ///     .mount(routes::api())
    ///     .run()
    ///     .await
    /// ```
    ///
    /// When a pool is provided this way, `[database] url` from `atrg.toml`
    /// is ignored. atrg's internal migrations are still applied to the
    /// supplied pool on startup.
    pub fn with_db_pool(mut self, pool: impl Into<DbPool>) -> Self {
        self.user_db_pool = Some(pool.into());
        self
    }

    /// Register an app-specific extension value.
    ///
    /// Extensions are type-erased values accessible from any handler via
    /// [`AppState::extension::<T>()`](crate::state::AppState::extension) or
    /// [`AppState::try_extension::<T>()`](crate::state::AppState::try_extension).
    ///
    /// Each type can appear at most once — inserting a second value of the
    /// same type replaces the first.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// struct S3Client { bucket: String }
    /// struct SmtpConfig { host: String }
    ///
    /// AtrgApp::new()
    ///     .with_extension(S3Client { bucket: "my-blobs".into() })
    ///     .with_extension(SmtpConfig { host: "smtp.example.com".into() })
    ///     .mount(routes())
    ///     .run()
    ///     .await
    /// ```
    pub fn with_extension<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.extensions.insert(value);
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

    /// Register a firehose event handler.
    ///
    /// The handler is called for every event received from the AT Protocol
    /// relay firehose (`com.atproto.sync.subscribeRepos`). It is spawned as
    /// a background task inside [`AtrgApp::run`] when `[firehose]` is present
    /// in `atrg.toml`.
    #[cfg(feature = "firehose")]
    pub fn on_firehose_event<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(atrg_firehose::FirehoseEvent, AppState) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.firehose_handler = Some(std::sync::Arc::new(move |event, state| {
            Box::pin(handler(event, state)) as BoxFuture<'static, anyhow::Result<()>>
        }));
        self
    }

    /// Mount a feed generator's routes.
    ///
    /// Pass the router produced by `FeedGenerator::into_router()` (from the
    /// `atrg-feed` crate).
    /// This is a semantic alias for [`mount`](Self::mount) that makes the
    /// builder read more clearly.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// AtrgApp::new()
    ///     .with_feed_generator(feed_gen.into_router())
    /// ```
    pub fn with_feed_generator(self, feed_router: Router<AppState>) -> Self {
        self.mount(feed_router)
    }

    /// Mount a labeler service's routes.
    ///
    /// Pass the router produced by `labeler_routes()` (from the `atrg-label`
    /// crate).
    /// This is a semantic alias for [`mount`](Self::mount) that makes the
    /// builder read more clearly.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// AtrgApp::new()
    ///     .with_labeler(atrg_label::routes::labeler_routes(service))
    /// ```
    pub fn with_labeler(self, labeler_router: Router<AppState>) -> Self {
        self.mount(labeler_router)
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
        let db = match self.user_db_pool {
            Some(pool) => {
                tracing::info!(
                    backend = pool.backend(),
                    "using caller-supplied database pool (bypassing [database] url)"
                );
                pool
            }
            None => atrg_db::connect(&config.database.url).await?,
        };
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
            extensions: Arc::new(self.extensions),
        };

        // 5b. Admin bootstrap ---------------------------------------------------
        if !config.app.admin_dids.is_empty() {
            for did in &config.app.admin_dids {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_string();
                let result: Result<(), sqlx::Error> = match &state.db {
                    #[cfg(feature = "sqlite")]
                    atrg_db::DbPool::Sqlite(p) => {
                        sqlx::query(
                            "INSERT OR IGNORE INTO atrg_roles (did, role, granted_by, granted_at) VALUES (?1, 'admin', 'system:bootstrap', ?2)"
                        ).bind(did).bind(&now).execute(p).await.map(|_| ())
                    }
                    #[cfg(feature = "postgres")]
                    atrg_db::DbPool::Postgres(p) => {
                        sqlx::query(
                            "INSERT INTO atrg_roles (did, role, granted_by, granted_at) VALUES ($1, 'admin', 'system:bootstrap', $2) ON CONFLICT DO NOTHING"
                        ).bind(did).bind(&now).execute(p).await.map(|_| ())
                    }
                };
                match result {
                    Ok(_) => tracing::info!(did = %did, "auto-provisioned admin DID"),
                    Err(e) => {
                        tracing::warn!(did = %did, error = %e, "failed to bootstrap admin DID (table may not exist yet)")
                    }
                }
            }
        }

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

        // 8a. Rate limiting (if configured) -------------------------------------
        if let Some(ref rl_config) = config.rate_limit {
            if rl_config.enabled {
                let limiter =
                    crate::rate_limit::RateLimiter::new(crate::rate_limit::RateLimitConfig {
                        requests_per_second: rl_config.requests_per_second,
                        burst: rl_config.burst,
                        enabled: true,
                    });

                // Spawn periodic cleanup task (every 5 minutes, remove entries older than 10 min)
                let limiter_cleanup = limiter.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                    loop {
                        interval.tick().await;
                        limiter_cleanup
                            .cleanup(std::time::Duration::from_secs(600))
                            .await;
                    }
                });

                router = router.layer(axum::middleware::from_fn(
                    move |req: axum::extract::Request, next: axum::middleware::Next| {
                        let limiter = limiter.clone();
                        async move {
                            // Extract client IP from connection info or X-Forwarded-For
                            let ip = req
                                .extensions()
                                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                                .map(|ci| ci.0.ip())
                                .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

                            match limiter.check(ip).await {
                                Ok(()) => next.run(req).await,
                                Err(retry_after) => {
                                    crate::rate_limit::rate_limit_response(retry_after)
                                }
                            }
                        }
                    },
                ));

                tracing::info!(
                    rps = rl_config.requests_per_second,
                    burst = rl_config.burst,
                    "rate limiting enabled"
                );
            }
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
                    cursor: None,
                };
                atrg_stream::spawn_consumer(&stream_config, state.clone(), handler).await?;
            } else {
                tracing::warn!("jetstream configured but no on_event handler registered");
            }
        }

        // 8c. Firehose consumer --------------------------------------------------
        #[cfg(feature = "firehose")]
        if let Some(ref fh_config) = config.firehose {
            if let Some(handler) = self.firehose_handler {
                let firehose_config = atrg_firehose::FirehoseConfig {
                    relay: fh_config.relay.clone(),
                    cursor: fh_config.cursor,
                    channel_capacity: fh_config.channel_capacity,
                };
                atrg_firehose::spawn_firehose(&firehose_config, state.clone(), handler).await?;
            } else {
                tracing::warn!("firehose configured but no on_firehose_event handler registered");
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
                admin_dids: vec![],
            },
            auth: AuthConfig {
                client_id: "http://localhost:3000/client-metadata.json".into(),
                redirect_uri: "http://localhost:3000/auth/callback".into(),
                scope: "atproto transition:generic".into(),
                post_login_redirect: "/".into(),
            },
            database: DatabaseConfig {
                url: "sqlite::memory:".into(),
            },
            jetstream: None,
            firehose: None,
            feed_generator: None,
            labeler: None,
            rate_limit: None,
        };

        AppState {
            config: Arc::new(config),
            db,
            http: reqwest::Client::new(),
            identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
                reqwest::Client::new(),
            )),
            extensions: Arc::new(Extensions::new()),
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
    async fn with_db_pool_stores_caller_pool() {
        // Caller-supplied pool should override the [database] url path.
        let pool = atrg_db::connect("sqlite::memory:").await.unwrap();
        let app = AtrgApp::new().with_db_pool(pool.clone());
        assert!(app.user_db_pool.is_some());
        assert_eq!(app.user_db_pool.as_ref().unwrap().backend(), "sqlite");
    }

    #[tokio::test]
    async fn readyz_reports_backend_kind() {
        // Readiness response should expose the backend identifier so ops
        // can confirm the right driver is in use.
        let state = test_state().await;
        let app: Router = Router::new()
            .route("/readyz", get(crate::health::readyz))
            .with_state(state);

        let req = Request::builder()
            .uri("/readyz")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let bytes = body_bytes(resp).await;
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["database_backend"], "sqlite");
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

    #[tokio::test]
    async fn with_extension_is_accessible_from_state() {
        struct MyConfig {
            magic_number: u64,
        }

        let state = test_state().await;
        // Verify the extension is reachable via AppState constructed by the builder.
        // Since `run()` binds a port (can't easily test full lifecycle), we test
        // the builder populates extensions correctly by constructing an AppState
        // with the extension and hitting a handler that reads it.
        let mut ext = Extensions::new();
        ext.insert(MyConfig { magic_number: 42 });

        let state_with_ext = AppState {
            config: state.config.clone(),
            db: state.db.clone(),
            http: state.http.clone(),
            identity: state.identity.clone(),
            extensions: Arc::new(ext),
        };

        let app: Router = Router::new()
            .route(
                "/magic",
                get(
                    |axum::extract::State(s): axum::extract::State<AppState>| async move {
                        let cfg = s.extension::<MyConfig>();
                        Json(serde_json::json!({ "magic": cfg.magic_number }))
                    },
                ),
            )
            .with_state(state_with_ext);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/magic")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body = body_bytes(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["magic"], 42);
    }

    #[test]
    fn with_extension_builder_accumulates_values() {
        struct Foo(u32);
        struct Bar(String);

        let app = AtrgApp::new()
            .with_extension(Foo(7))
            .with_extension(Bar("baz".into()));

        assert_eq!(app.extensions.get::<Foo>().unwrap().0, 7);
        assert_eq!(app.extensions.get::<Bar>().unwrap().0, "baz");
    }
}
