//! Application state shared across all Axum handlers.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use atrg_db::DbPool;

use crate::config::Config;
use atrg_identity::IdentityResolver;

// ---------------------------------------------------------------------------
// Extensions — a type-erased map for app-specific state
// ---------------------------------------------------------------------------

/// A type-erased container for app-specific state.
///
/// `Extensions` lets applications attach arbitrary typed values to
/// [`AppState`] without modifying the framework. Each type can appear at most
/// once — the type itself is the key.
///
/// # Examples
///
/// ```rust
/// use atrg_core::Extensions;
///
/// struct S3Client { bucket: String }
/// struct SmtpConfig { host: String }
///
/// let mut ext = Extensions::new();
/// ext.insert(S3Client { bucket: "my-blobs".into() });
/// ext.insert(SmtpConfig { host: "smtp.example.com".into() });
///
/// assert_eq!(ext.get::<S3Client>().unwrap().bucket, "my-blobs");
/// assert_eq!(ext.get::<SmtpConfig>().unwrap().host, "smtp.example.com");
/// assert!(ext.get::<u64>().is_none());
/// ```
#[derive(Default)]
pub struct Extensions {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Extensions {
    /// Create a new, empty extensions map.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Insert a value into the map. If a value of this type already exists,
    /// it is replaced and the old value is returned.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) -> Option<T> {
        self.map
            .insert(TypeId::of::<T>(), Box::new(value))
            .and_then(|boxed| boxed.downcast::<T>().ok().map(|b| *b))
    }

    /// Retrieve a reference to a value by type. Returns `None` if the type
    /// has not been inserted.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    /// Returns `true` if the map contains a value of the given type.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    /// Returns the number of entries in the map.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// Manual Debug impl because `dyn Any` is not Debug.
impl std::fmt::Debug for Extensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extensions")
            .field("len", &self.map.len())
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

/// Shared application state passed to every Axum handler.
///
/// This is the central state object that every route handler receives via
/// `axum::extract::State<AppState>`. It holds the parsed configuration,
/// database connection pool, and a shared HTTP client for outbound requests.
///
/// `AppState` is cheaply cloneable — all inner fields are either `Arc`-wrapped
/// or already use internal reference counting (e.g. sqlx pools, `reqwest::Client`).
#[derive(Clone)]
pub struct AppState {
    /// Parsed configuration from `atrg.toml`.
    pub config: Arc<Config>,
    /// Database connection pool. May be SQLite or PostgreSQL depending on
    /// the `[database] url` scheme in `atrg.toml` (and which features are
    /// compiled in to `atrg-db`).
    pub db: DbPool,
    /// Shared HTTP client for outbound requests.
    pub http: reqwest::Client,
    /// DID/handle resolver with TTL-backed in-memory cache.
    pub identity: Arc<IdentityResolver>,
    /// Type-erased container for app-specific state (S3 clients, SMTP config,
    /// domain-specific services, etc.). Access via [`AppState::extension`] or
    /// [`AppState::try_extension`].
    pub extensions: Arc<Extensions>,
}

impl AppState {
    /// Retrieve a reference to an app-specific extension by type.
    ///
    /// # Panics
    ///
    /// Panics if the extension has not been registered. Use
    /// [`try_extension`](Self::try_extension) for a non-panicking variant.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// struct MyService { url: String }
    ///
    /// // In a handler:
    /// async fn my_handler(State(state): State<AppState>) -> impl IntoResponse {
    ///     let svc = state.extension::<MyService>();
    ///     Json(json!({ "url": svc.url }))
    /// }
    /// ```
    pub fn extension<T: Send + Sync + 'static>(&self) -> &T {
        self.extensions.get::<T>().unwrap_or_else(|| {
            panic!(
                "AppState::extension::<{}>() called but no value of that type was registered. \
                 Did you forget to call `AtrgApp::with_extension(value)` during app setup?",
                std::any::type_name::<T>()
            )
        })
    }

    /// Retrieve a reference to an app-specific extension by type, returning
    /// `None` if the type was never registered.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(metrics) = state.try_extension::<MetricsCollector>() {
    ///     metrics.record_request();
    /// }
    /// ```
    pub fn try_extension<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.extensions.get::<T>()
    }

    /// Returns `true` if an extension of type `T` has been registered.
    pub fn has_extension<T: Send + Sync + 'static>(&self) -> bool {
        self.extensions.contains::<T>()
    }
}

// ---------------------------------------------------------------------------
// FromRef implementations — allow Axum sub-extractors to pull individual
// fields out of AppState without the handler needing to destructure manually.
// ---------------------------------------------------------------------------

impl axum::extract::FromRef<AppState> for DbPool {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<Config> {
    fn from_ref(state: &AppState) -> Self {
        state.config.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<IdentityResolver> {
    fn from_ref(state: &AppState) -> Self {
        state.identity.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<Extensions> {
    fn from_ref(state: &AppState) -> Self {
        state.extensions.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time assertion helper.
    fn _assert_send_sync_clone<T: Send + Sync + Clone>() {}

    #[test]
    fn app_state_is_send_sync_clone() {
        _assert_send_sync_clone::<AppState>();
    }

    // -- Extensions unit tests ------------------------------------------------

    #[test]
    fn extensions_insert_and_get() {
        struct Foo(u32);
        struct Bar(String);

        let mut ext = Extensions::new();
        ext.insert(Foo(42));
        ext.insert(Bar("hello".into()));

        assert_eq!(ext.get::<Foo>().unwrap().0, 42);
        assert_eq!(ext.get::<Bar>().unwrap().0, "hello");
    }

    #[test]
    fn extensions_get_missing_returns_none() {
        let ext = Extensions::new();
        assert!(ext.get::<u32>().is_none());
    }

    #[test]
    fn extensions_insert_replaces_and_returns_old() {
        struct Config(String);

        let mut ext = Extensions::new();
        let old = ext.insert(Config("v1".into()));
        assert!(old.is_none());

        let old = ext.insert(Config("v2".into()));
        assert_eq!(old.unwrap().0, "v1");
        assert_eq!(ext.get::<Config>().unwrap().0, "v2");
    }

    #[test]
    fn extensions_contains() {
        struct Present;

        let mut ext = Extensions::new();
        assert!(!ext.contains::<Present>());
        ext.insert(Present);
        assert!(ext.contains::<Present>());
    }

    #[test]
    fn extensions_len_and_is_empty() {
        struct A;
        struct B;

        let mut ext = Extensions::new();
        assert!(ext.is_empty());
        assert_eq!(ext.len(), 0);

        ext.insert(A);
        assert!(!ext.is_empty());
        assert_eq!(ext.len(), 1);

        ext.insert(B);
        assert_eq!(ext.len(), 2);
    }

    #[test]
    fn extensions_debug_shows_len() {
        let mut ext = Extensions::new();
        ext.insert(42u32);
        let dbg = format!("{:?}", ext);
        assert!(dbg.contains("Extensions"));
        assert!(dbg.contains("len"));
    }

    #[tokio::test]
    async fn app_state_extension_returns_value() {
        struct MyService {
            name: String,
        }

        let mut ext = Extensions::new();
        ext.insert(MyService {
            name: "test".into(),
        });

        let db = atrg_db::connect("sqlite::memory:").await.unwrap();
        let state = AppState {
            config: Arc::new(crate::config::Config {
                app: crate::config::AppConfig {
                    name: "test".into(),
                    host: "127.0.0.1".into(),
                    port: 3000,
                    secret_key: "secret".into(),
                    cors_origins: vec![],
                    environment: "development".into(),
                    admin_dids: vec![],
                },
                auth: crate::config::AuthConfig {
                    client_id: "http://localhost/client-metadata.json".into(),
                    redirect_uri: "http://localhost/auth/callback".into(),
                    scope: "atproto transition:generic".into(),
                    post_login_redirect: "/".into(),
                },
                database: crate::config::DatabaseConfig {
                    url: "sqlite::memory:".into(),
                },
                jetstream: None,
                firehose: None,
                feed_generator: None,
                labeler: None,
                rate_limit: None,
            }),
            db,
            http: reqwest::Client::new(),
            identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
                reqwest::Client::new(),
            )),
            extensions: Arc::new(ext),
        };

        assert_eq!(state.extension::<MyService>().name, "test");
    }

    #[tokio::test]
    async fn app_state_try_extension_returns_none_when_missing() {
        struct NotRegistered;

        let db = atrg_db::connect("sqlite::memory:").await.unwrap();
        let state = AppState {
            config: Arc::new(crate::config::Config {
                app: crate::config::AppConfig {
                    name: "test".into(),
                    host: "127.0.0.1".into(),
                    port: 3000,
                    secret_key: "secret".into(),
                    cors_origins: vec![],
                    environment: "development".into(),
                    admin_dids: vec![],
                },
                auth: crate::config::AuthConfig {
                    client_id: "http://localhost/client-metadata.json".into(),
                    redirect_uri: "http://localhost/auth/callback".into(),
                    scope: "atproto transition:generic".into(),
                    post_login_redirect: "/".into(),
                },
                database: crate::config::DatabaseConfig {
                    url: "sqlite::memory:".into(),
                },
                jetstream: None,
                firehose: None,
                feed_generator: None,
                labeler: None,
                rate_limit: None,
            }),
            db,
            http: reqwest::Client::new(),
            identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
                reqwest::Client::new(),
            )),
            extensions: Arc::new(Extensions::new()),
        };

        assert!(state.try_extension::<NotRegistered>().is_none());
        assert!(!state.has_extension::<NotRegistered>());
    }

    #[tokio::test]
    #[should_panic(expected = "no value of that type was registered")]
    async fn app_state_extension_panics_when_missing() {
        struct NotRegistered;

        let db = atrg_db::connect("sqlite::memory:").await.unwrap();
        let state = AppState {
            config: Arc::new(crate::config::Config {
                app: crate::config::AppConfig {
                    name: "test".into(),
                    host: "127.0.0.1".into(),
                    port: 3000,
                    secret_key: "secret".into(),
                    cors_origins: vec![],
                    environment: "development".into(),
                    admin_dids: vec![],
                },
                auth: crate::config::AuthConfig {
                    client_id: "http://localhost/client-metadata.json".into(),
                    redirect_uri: "http://localhost/auth/callback".into(),
                    scope: "atproto transition:generic".into(),
                    post_login_redirect: "/".into(),
                },
                database: crate::config::DatabaseConfig {
                    url: "sqlite::memory:".into(),
                },
                jetstream: None,
                firehose: None,
                feed_generator: None,
                labeler: None,
                rate_limit: None,
            }),
            db,
            http: reqwest::Client::new(),
            identity: Arc::new(atrg_identity::IdentityResolver::with_defaults(
                reqwest::Client::new(),
            )),
            extensions: Arc::new(Extensions::new()),
        };

        let _ = state.extension::<NotRegistered>();
    }
}
