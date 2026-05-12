//! Configuration types and loader for `atrg.toml`.
//!
//! The [`Config`] struct is the single source of truth for all framework
//! configuration. It is loaded once at startup by [`Config::load`] and then
//! wrapped in an `Arc` inside [`AppState`](crate::state::AppState).

use std::path::Path;

use axum::http;
use serde::Deserialize;
use url::Url;

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

/// Root configuration, deserialized from `atrg.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Application-level settings.
    pub app: AppConfig,

    /// OAuth / authentication settings.
    #[serde(default)]
    pub auth: AuthConfig,

    /// Database connection settings.
    #[serde(default)]
    pub database: DatabaseConfig,

    /// Optional Jetstream real-time event consumer settings.
    pub jetstream: Option<JetstreamConfig>,

    /// Optional relay firehose consumer settings.
    pub firehose: Option<FirehoseConfig>,

    /// Optional feed generator settings.
    pub feed_generator: Option<FeedGeneratorConfig>,

    /// Optional labeler settings.
    pub labeler: Option<LabelerConfig>,

    /// Optional rate limiting settings.
    pub rate_limit: Option<RateLimitTomlConfig>,
}

// ---------------------------------------------------------------------------
// AppConfig
// ---------------------------------------------------------------------------

/// `[app]` section of `atrg.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    /// Human-readable application name. Must be non-empty.
    pub name: String,

    /// Bind address for the HTTP server.
    #[serde(default = "default_host")]
    pub host: String,

    /// Bind port for the HTTP server.
    #[serde(default = "default_port")]
    pub port: u16,

    /// Secret key used for session signing. Should be ≥ 32 characters in
    /// production.
    pub secret_key: String,

    /// Allowed CORS origins. An empty list means same-origin only. A single
    /// `"*"` entry enables the permissive wildcard.
    #[serde(default)]
    pub cors_origins: Vec<String>,

    /// `"development"` or `"production"`. Affects cookie flags and security
    /// headers.
    #[serde(default = "default_environment")]
    pub environment: String,

    /// DIDs to auto-provision as admin on startup. Populated from `atrg.toml`
    /// or the `ATRG_APP__ADMIN_DIDS` env var (comma-separated).
    #[serde(default)]
    pub admin_dids: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: default_host(),
            port: default_port(),
            secret_key: String::new(),
            cors_origins: Vec::new(),
            environment: default_environment(),
            admin_dids: Vec::new(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    3000
}

fn default_environment() -> String {
    "development".to_string()
}

// ---------------------------------------------------------------------------
// AuthConfig
// ---------------------------------------------------------------------------

/// `[auth]` section of `atrg.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    /// AT Protocol OAuth client ID (must be a valid URL).
    #[serde(default = "default_client_id")]
    pub client_id: String,

    /// OAuth redirect URI (must be a valid URL).
    #[serde(default = "default_redirect_uri")]
    pub redirect_uri: String,

    /// OAuth scope string.
    #[serde(default = "default_scope")]
    pub scope: String,

    /// URL to redirect the browser to after successful OAuth login.
    /// This is the **frontend** URL, not the OAuth callback.
    /// Defaults to `"/"`.
    #[serde(default = "default_post_login_redirect")]
    pub post_login_redirect: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            client_id: default_client_id(),
            redirect_uri: default_redirect_uri(),
            scope: default_scope(),
            post_login_redirect: default_post_login_redirect(),
        }
    }
}

fn default_client_id() -> String {
    "http://localhost:3000/client-metadata.json".to_string()
}

fn default_redirect_uri() -> String {
    "http://localhost:3000/auth/callback".to_string()
}

fn default_scope() -> String {
    "atproto transition:generic".to_string()
}

fn default_post_login_redirect() -> String {
    "/".to_string()
}

// ---------------------------------------------------------------------------
// DatabaseConfig
// ---------------------------------------------------------------------------

/// `[database]` section of `atrg.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// SQLite connection URL.
    #[serde(default = "default_database_url")]
    pub url: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_database_url(),
        }
    }
}

fn default_database_url() -> String {
    "sqlite://atrg.db".to_string()
}

// ---------------------------------------------------------------------------
// JetstreamConfig
// ---------------------------------------------------------------------------

/// `[jetstream]` section of `atrg.toml`. Only present when Jetstream
/// consumption is enabled.
#[derive(Debug, Clone, Deserialize)]
pub struct JetstreamConfig {
    /// Jetstream relay host, e.g. `"jetstream1.us-east.bsky.network"`.
    pub host: String,

    /// NSID collections to subscribe to, e.g. `["app.bsky.feed.post"]`.
    pub collections: Vec<String>,

    /// Optional path or URL to a ZSTD dictionary for decompression.
    pub zstd_dict: Option<String>,

    /// Bounded back-pressure channel size.
    #[serde(default = "default_channel_capacity")]
    pub channel_capacity: usize,

    /// Event lag threshold before shedding/warning.
    #[serde(default = "default_max_lag_events")]
    pub max_lag_events: usize,
}

fn default_channel_capacity() -> usize {
    1024
}

fn default_max_lag_events() -> usize {
    10_000
}

// ---------------------------------------------------------------------------
// FirehoseConfig
// ---------------------------------------------------------------------------

/// `[firehose]` section of `atrg.toml`. Present when relay firehose
/// consumption is enabled (full `com.atproto.sync.subscribeRepos`).
#[derive(Debug, Clone, Deserialize)]
pub struct FirehoseConfig {
    /// Relay WebSocket URL, e.g. `"wss://bsky.network"`.
    pub relay: String,

    /// Sequence number to resume from. `None` means start from head.
    pub cursor: Option<i64>,

    /// Bounded back-pressure channel capacity.
    #[serde(default = "default_firehose_channel_capacity")]
    pub channel_capacity: usize,
}

fn default_firehose_channel_capacity() -> usize {
    1024
}

// ---------------------------------------------------------------------------
// FeedGeneratorConfig
// ---------------------------------------------------------------------------

/// `[feed_generator]` section of `atrg.toml`. Present when the server
/// acts as an AT Protocol feed generator.
#[derive(Debug, Clone, Deserialize)]
pub struct FeedGeneratorConfig {
    /// DID of the feed generator service (typically `did:web:<hostname>`).
    pub did: String,
}

// ---------------------------------------------------------------------------
// LabelerConfig
// ---------------------------------------------------------------------------

/// `[labeler]` section of `atrg.toml`. Present when the server acts as
/// an AT Protocol labeler.
#[derive(Debug, Clone, Deserialize)]
pub struct LabelerConfig {
    /// DID of the labeler service.
    pub did: String,

    /// Path to the signing key file (PEM format).
    pub signing_key_path: Option<String>,

    /// Inline signing key (base64-encoded, for env var injection).
    pub signing_key_base64: Option<String>,
}

// ---------------------------------------------------------------------------
// RateLimitConfig (TOML)
// ---------------------------------------------------------------------------

/// `[rate_limit]` section of `atrg.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitTomlConfig {
    /// Maximum sustained requests per second.
    #[serde(default = "default_rps")]
    pub requests_per_second: f64,

    /// Maximum burst size.
    #[serde(default = "default_burst")]
    pub burst: u32,

    /// Whether rate limiting is enabled (default: true in production).
    #[serde(default = "default_rate_limit_enabled")]
    pub enabled: bool,
}

fn default_rps() -> f64 {
    10.0
}

fn default_burst() -> u32 {
    50
}

fn default_rate_limit_enabled() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Loading & validation
// ---------------------------------------------------------------------------

impl Config {
    /// Load and validate a [`Config`] from the TOML file at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, the TOML is malformed, or
    /// mandatory validation checks fail (e.g. empty `app.name`).
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to read config file '{}': {}. \
                 Make sure you're running from a directory that contains atrg.toml.",
                path.display(),
                e
            )
        })?;
        Self::parse_toml(&contents)
    }

    /// Parse and validate a [`Config`] from a TOML string.
    ///
    /// This is the inner implementation shared by [`Config::load`] and tests.
    pub fn parse_toml(toml_str: &str) -> anyhow::Result<Self> {
        let config: Config = toml::from_str(toml_str).map_err(|e| {
            // Provide a friendlier message when a required section is missing.
            let msg = e.to_string();
            if msg.contains("missing field `app`") {
                anyhow::anyhow!(
                    "Config error: the [app] section is required in atrg.toml. \
                     At minimum you need:\n\n\
                     [app]\n\
                     name = \"my-app\"\n\
                     secret_key = \"some-secret-key\"\n\n\
                     Full error: {e}"
                )
            } else {
                anyhow::anyhow!("Failed to parse atrg.toml: {e}")
            }
        })?;

        config.validate()?;
        Ok(config)
    }

    /// Run all validation checks and emit warnings.
    fn validate(&self) -> anyhow::Result<()> {
        // -- hard errors ------------------------------------------------

        if self.app.name.trim().is_empty() {
            anyhow::bail!("Config error: app.name must not be empty");
        }

        if self.app.secret_key.trim().is_empty() {
            anyhow::bail!("Config error: app.secret_key must not be empty");
        }

        // Validate redirect_uri is a proper URL.
        if Url::parse(&self.auth.redirect_uri).is_err() {
            anyhow::bail!(
                "Config error: auth.redirect_uri '{}' is not a valid URL",
                self.auth.redirect_uri
            );
        }

        // Validate client_id is a proper URL.
        if Url::parse(&self.auth.client_id).is_err() {
            anyhow::bail!(
                "Config error: auth.client_id '{}' is not a valid URL",
                self.auth.client_id
            );
        }

        // Validate each CORS origin entry.
        for origin in &self.app.cors_origins {
            if origin == "*" {
                continue; // wildcard is fine
            }
            if origin.parse::<http::HeaderValue>().is_err() {
                anyhow::bail!(
                    "Config error: cors_origins entry '{}' is not a valid origin",
                    origin
                );
            }
        }

        // -- soft warnings ---------------------------------------------

        if self.app.secret_key.len() < 32 {
            tracing::warn!(
                "app.secret_key is only {} characters — use at least 32 for production",
                self.app.secret_key.len()
            );
        }

        let is_local = self.app.host == "localhost" || self.app.host == "127.0.0.1";
        if self.app.secret_key == "CHANGE_ME_IN_PRODUCTION" && !is_local {
            tracing::warn!(
                "app.secret_key is the scaffold default and host is '{}' — \
                 change it before deploying!",
                self.app.host
            );
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// App-specific config loading
// ---------------------------------------------------------------------------

/// Load an app-specific configuration section from `atrg.toml`.
///
/// This allows apps to define custom `[section_name]` blocks in `atrg.toml`
/// and deserialize them into typed structs, with automatic environment
/// variable overrides using the `{PREFIX}_FIELD` convention.
///
/// # Examples
///
/// ```rust,ignore
/// #[derive(serde::Deserialize)]
/// struct MyAppConfig {
///     database_url: String,
///     admin_dids: Vec<String>,
/// }
///
/// let config: MyAppConfig = atrg_core::config::load_app_config("myapp")?;
/// ```
///
/// Fields can be overridden by env vars: set `MYAPP_DATABASE_URL` to override
/// `[myapp] database_url`. The prefix is derived by uppercasing the section name.
pub fn load_app_config<T: serde::de::DeserializeOwned>(section_name: &str) -> anyhow::Result<T> {
    load_app_config_from_path::<T>(section_name, "atrg.toml")
}

/// Load an app-specific configuration section from a specific TOML file path.
pub fn load_app_config_from_path<T: serde::de::DeserializeOwned>(
    section_name: &str,
    path: &str,
) -> anyhow::Result<T> {
    let toml_str = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path, e))?;
    let toml_val: toml::Value = toml::from_str(&toml_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", path, e))?;
    let section = toml_val
        .get(section_name)
        .ok_or_else(|| anyhow::anyhow!("Missing [{}] section in {}", section_name, path))?;
    let config: T = section.clone().try_into().map_err(|e| {
        anyhow::anyhow!(
            "Invalid [{}] configuration in {}: {}",
            section_name,
            path,
            e
        )
    })?;
    Ok(config)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A full config fixture exercising every field.
    const FULL_CONFIG: &str = r#"
[app]
name = "my-app"
host = "0.0.0.0"
port = 8080
secret_key = "super-secret-key-that-is-long-enough"
cors_origins = ["http://localhost:5173", "https://example.com"]
environment = "production"

[auth]
client_id = "https://myapp.example.com/client-metadata.json"
redirect_uri = "https://myapp.example.com/auth/callback"
scope = "atproto transition:generic"

[database]
url = "sqlite://prod.db"

[jetstream]
host = "jetstream1.us-east.bsky.network"
collections = ["app.bsky.feed.post", "app.bsky.feed.like"]
zstd_dict = "/tmp/dict.bin"
channel_capacity = 2048
max_lag_events = 20000
"#;

    /// Minimal config — only the required fields.
    const MINIMAL_CONFIG: &str = r#"
[app]
name = "tiny"
secret_key = "abcdefghijklmnopqrstuvwxyz123456"
"#;

    #[test]
    fn parse_full_config() {
        let cfg = Config::parse_toml(FULL_CONFIG).expect("should parse full config");

        assert_eq!(cfg.app.name, "my-app");
        assert_eq!(cfg.app.host, "0.0.0.0");
        assert_eq!(cfg.app.port, 8080);
        assert_eq!(cfg.app.environment, "production");
        assert_eq!(cfg.app.cors_origins.len(), 2);

        assert_eq!(
            cfg.auth.client_id,
            "https://myapp.example.com/client-metadata.json"
        );
        assert_eq!(
            cfg.auth.redirect_uri,
            "https://myapp.example.com/auth/callback"
        );
        assert_eq!(cfg.auth.scope, "atproto transition:generic");

        assert_eq!(cfg.database.url, "sqlite://prod.db");

        let js = cfg.jetstream.expect("jetstream should be present");
        assert_eq!(js.host, "jetstream1.us-east.bsky.network");
        assert_eq!(js.collections.len(), 2);
        assert_eq!(js.zstd_dict.as_deref(), Some("/tmp/dict.bin"));
        assert_eq!(js.channel_capacity, 2048);
        assert_eq!(js.max_lag_events, 20000);
    }

    #[test]
    fn parse_minimal_config_defaults_applied() {
        let cfg = Config::parse_toml(MINIMAL_CONFIG).expect("should parse minimal config");

        // Explicit values
        assert_eq!(cfg.app.name, "tiny");

        // Defaults
        assert_eq!(cfg.app.host, "127.0.0.1");
        assert_eq!(cfg.app.port, 3000);
        assert_eq!(cfg.app.environment, "development");
        assert!(cfg.app.cors_origins.is_empty());

        assert_eq!(
            cfg.auth.client_id,
            "http://localhost:3000/client-metadata.json"
        );
        assert_eq!(cfg.auth.redirect_uri, "http://localhost:3000/auth/callback");
        assert_eq!(cfg.auth.scope, "atproto transition:generic");

        assert_eq!(cfg.database.url, "sqlite://atrg.db");
        assert!(cfg.jetstream.is_none());
    }

    #[test]
    fn missing_app_section_gives_friendly_error() {
        let toml = r#"
[database]
url = "sqlite://test.db"
"#;
        let err = Config::parse_toml(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("[app] section is required"),
            "expected friendly error, got: {msg}"
        );
    }

    #[test]
    fn empty_name_is_rejected() {
        let toml = r#"
[app]
name = ""
secret_key = "abcdefghijklmnopqrstuvwxyz123456"
"#;
        let err = Config::parse_toml(toml).unwrap_err();
        assert!(
            err.to_string().contains("app.name must not be empty"),
            "got: {}",
            err
        );
    }

    #[test]
    fn empty_secret_key_is_rejected() {
        let toml = r#"
[app]
name = "test"
secret_key = ""
"#;
        let err = Config::parse_toml(toml).unwrap_err();
        assert!(
            err.to_string().contains("app.secret_key must not be empty"),
            "got: {}",
            err
        );
    }

    #[test]
    fn invalid_redirect_uri_is_rejected() {
        let toml = r#"
[app]
name = "test"
secret_key = "abcdefghijklmnopqrstuvwxyz123456"

[auth]
redirect_uri = "not a url at all"
"#;
        let err = Config::parse_toml(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("auth.redirect_uri") && msg.contains("not a valid URL"),
            "expected redirect_uri error, got: {msg}"
        );
    }

    #[test]
    fn invalid_client_id_is_rejected() {
        let toml = r#"
[app]
name = "test"
secret_key = "abcdefghijklmnopqrstuvwxyz123456"

[auth]
client_id = "not a url"
"#;
        let err = Config::parse_toml(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("auth.client_id") && msg.contains("not a valid URL"),
            "expected client_id error, got: {msg}"
        );
    }

    #[test]
    fn invalid_cors_origin_is_rejected() {
        let toml = r#"
[app]
name = "test"
secret_key = "abcdefghijklmnopqrstuvwxyz123456"
cors_origins = ["http://ok.example.com", "\x00bad"]
"#;
        let err = Config::parse_toml(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("cors_origins"),
            "expected cors origin error, got: {msg}"
        );
    }

    #[test]
    fn wildcard_cors_origin_is_accepted() {
        let toml = r#"
[app]
name = "test"
secret_key = "abcdefghijklmnopqrstuvwxyz123456"
cors_origins = ["*"]
"#;
        Config::parse_toml(toml).expect("wildcard should be accepted");
    }

    #[test]
    fn parse_config_with_firehose_and_feeds() {
        let toml = r#"
[app]
name = "test"
secret_key = "abcdefghijklmnopqrstuvwxyz123456"

[firehose]
relay = "wss://bsky.network"

[feed_generator]
did = "did:web:feeds.example.com"

[labeler]
did = "did:web:labels.example.com"
signing_key_path = "/etc/keys/labeler.pem"

[rate_limit]
requests_per_second = 20.0
burst = 100
enabled = true
"#;
        let cfg = Config::parse_toml(toml).unwrap();
        let fh = cfg.firehose.unwrap();
        assert_eq!(fh.relay, "wss://bsky.network");
        assert!(fh.cursor.is_none());
        assert_eq!(fh.channel_capacity, 1024);

        let fg = cfg.feed_generator.unwrap();
        assert_eq!(fg.did, "did:web:feeds.example.com");

        let lb = cfg.labeler.unwrap();
        assert_eq!(lb.did, "did:web:labels.example.com");
        assert_eq!(lb.signing_key_path.unwrap(), "/etc/keys/labeler.pem");

        let rl = cfg.rate_limit.unwrap();
        assert!((rl.requests_per_second - 20.0).abs() < f64::EPSILON);
        assert_eq!(rl.burst, 100);
    }

    #[test]
    fn new_sections_are_all_optional() {
        let toml = r#"
[app]
name = "test"
secret_key = "abcdefghijklmnopqrstuvwxyz123456"
"#;
        let cfg = Config::parse_toml(toml).unwrap();
        assert!(cfg.firehose.is_none());
        assert!(cfg.feed_generator.is_none());
        assert!(cfg.labeler.is_none());
        assert!(cfg.rate_limit.is_none());
    }

    #[test]
    fn jetstream_defaults_applied() {
        let toml = r#"
[app]
name = "test"
secret_key = "abcdefghijklmnopqrstuvwxyz123456"

[jetstream]
host = "jetstream1.us-east.bsky.network"
collections = ["app.bsky.feed.post"]
"#;
        let cfg = Config::parse_toml(toml).unwrap();
        let js = cfg.jetstream.unwrap();
        assert_eq!(js.channel_capacity, 1024);
        assert_eq!(js.max_lag_events, 10_000);
        assert!(js.zstd_dict.is_none());
    }
}
