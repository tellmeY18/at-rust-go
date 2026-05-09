//! Environment variable overrides for `atrg.toml` configuration.
//!
//! Any field in [`Config`] can be overridden at runtime
//! via an environment variable. The naming convention uses a double-underscore
//! (`__`) to separate the section from the field:
//!
//! | Config field            | Env var                    |
//! |-------------------------|----------------------------|
//! | `app.name`              | `ATRG_APP__NAME`           |
//! | `app.host`              | `ATRG_APP__HOST`           |
//! | `app.port`              | `ATRG_APP__PORT`           |
//! | `app.secret_key`        | `ATRG_APP__SECRET_KEY`     |
//! | `app.environment`       | `ATRG_APP__ENVIRONMENT`    |
//! | `app.cors_origins`      | `ATRG_APP__CORS_ORIGINS`   |
//! | `auth.client_id`        | `ATRG_AUTH__CLIENT_ID`     |
//! | `auth.redirect_uri`     | `ATRG_AUTH__REDIRECT_URI`  |
//! | `auth.scope`            | `ATRG_AUTH__SCOPE`         |
//! | `database.url`          | `ATRG_DATABASE__URL`       |
//!
//! For `app.cors_origins`, provide a comma-separated list of origins, e.g.
//! `ATRG_APP__CORS_ORIGINS="http://localhost:5173,https://example.com"`.
//!
//! Invalid values (e.g. a non-numeric `ATRG_APP__PORT`) are logged as warnings
//! and silently ignored — the original config value is preserved.

use crate::config::Config;

/// Apply environment variable overrides to a mutable [`Config`].
///
/// Call this after loading `atrg.toml` but before validation, so that env vars
/// can fix up values for the current deployment environment.
pub fn apply_env_overrides(config: &mut Config) {
    // -- [app] ----------------------------------------------------------
    if let Ok(val) = std::env::var("ATRG_APP__NAME") {
        tracing::debug!(key = "ATRG_APP__NAME", "applying env override");
        config.app.name = val;
    }

    if let Ok(val) = std::env::var("ATRG_APP__HOST") {
        tracing::debug!(key = "ATRG_APP__HOST", "applying env override");
        config.app.host = val;
    }

    if let Ok(val) = std::env::var("ATRG_APP__PORT") {
        match val.parse::<u16>() {
            Ok(port) => {
                tracing::debug!(key = "ATRG_APP__PORT", port, "applying env override");
                config.app.port = port;
            }
            Err(e) => {
                tracing::warn!(
                    key = "ATRG_APP__PORT",
                    value = %val,
                    error = %e,
                    "ignoring invalid ATRG_APP__PORT value"
                );
            }
        }
    }

    if let Ok(val) = std::env::var("ATRG_APP__SECRET_KEY") {
        tracing::debug!(key = "ATRG_APP__SECRET_KEY", "applying env override");
        config.app.secret_key = val;
    }

    if let Ok(val) = std::env::var("ATRG_APP__ENVIRONMENT") {
        tracing::debug!(key = "ATRG_APP__ENVIRONMENT", "applying env override");
        config.app.environment = val;
    }

    if let Ok(val) = std::env::var("ATRG_APP__CORS_ORIGINS") {
        tracing::debug!(key = "ATRG_APP__CORS_ORIGINS", "applying env override");
        config.app.cors_origins = val
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    // -- [auth] ---------------------------------------------------------
    if let Ok(val) = std::env::var("ATRG_AUTH__CLIENT_ID") {
        tracing::debug!(key = "ATRG_AUTH__CLIENT_ID", "applying env override");
        config.auth.client_id = val;
    }

    if let Ok(val) = std::env::var("ATRG_AUTH__REDIRECT_URI") {
        tracing::debug!(key = "ATRG_AUTH__REDIRECT_URI", "applying env override");
        config.auth.redirect_uri = val;
    }

    if let Ok(val) = std::env::var("ATRG_AUTH__SCOPE") {
        tracing::debug!(key = "ATRG_AUTH__SCOPE", "applying env override");
        config.auth.scope = val;
    }

    // -- [database] -----------------------------------------------------
    if let Ok(val) = std::env::var("ATRG_DATABASE__URL") {
        tracing::debug!(key = "ATRG_DATABASE__URL", "applying env override");
        config.database.url = val;
    }

    // -- [firehose] -----------------------------------------------------
    if let Ok(val) = std::env::var("ATRG_FIREHOSE__RELAY") {
        if let Some(ref mut fh) = config.firehose {
            tracing::debug!(key = "ATRG_FIREHOSE__RELAY", "applying env override");
            fh.relay = val;
        }
    }

    // -- [feed_generator] -----------------------------------------------
    if let Ok(val) = std::env::var("ATRG_FEED_GENERATOR__DID") {
        if let Some(ref mut fg) = config.feed_generator {
            tracing::debug!(key = "ATRG_FEED_GENERATOR__DID", "applying env override");
            fg.did = val;
        }
    }

    // -- [labeler] ------------------------------------------------------
    if let Ok(val) = std::env::var("ATRG_LABELER__DID") {
        if let Some(ref mut lb) = config.labeler {
            tracing::debug!(key = "ATRG_LABELER__DID", "applying env override");
            lb.did = val;
        }
    }
    if let Ok(val) = std::env::var("ATRG_LABELER__SIGNING_KEY_PATH") {
        if let Some(ref mut lb) = config.labeler {
            tracing::debug!(
                key = "ATRG_LABELER__SIGNING_KEY_PATH",
                "applying env override"
            );
            lb.signing_key_path = Some(val);
        }
    }
    if let Ok(val) = std::env::var("ATRG_LABELER__SIGNING_KEY_BASE64") {
        if let Some(ref mut lb) = config.labeler {
            tracing::debug!(
                key = "ATRG_LABELER__SIGNING_KEY_BASE64",
                "applying env override"
            );
            lb.signing_key_base64 = Some(val);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, AuthConfig, Config, DatabaseConfig};

    /// Build a baseline config for testing. No file I/O needed.
    fn base_config() -> Config {
        Config {
            app: AppConfig {
                name: "test-app".into(),
                host: "127.0.0.1".into(),
                port: 3000,
                secret_key: "abcdefghijklmnopqrstuvwxyz123456".into(),
                cors_origins: vec![],
                environment: "development".into(),
            },
            auth: AuthConfig {
                client_id: "http://localhost:3000/client-metadata.json".into(),
                redirect_uri: "http://localhost:3000/auth/callback".into(),
                scope: "atproto transition:generic".into(),
                post_login_redirect: "/".into(),
            },
            database: DatabaseConfig {
                url: "sqlite://atrg.db".into(),
            },
            jetstream: None,
            firehose: None,
            feed_generator: None,
            labeler: None,
            rate_limit: None,
        }
    }

    /// Helper: set an env var, run a closure, then remove the var.
    /// Ensures cleanup even on panic.
    fn with_env_var<F: FnOnce()>(key: &str, value: &str, f: F) {
        std::env::set_var(key, value);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        std::env::remove_var(key);
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    fn override_port() {
        let mut cfg = base_config();
        with_env_var("ATRG_APP__PORT", "9090", || {
            apply_env_overrides(&mut cfg);
        });
        assert_eq!(cfg.app.port, 9090);
    }

    #[test]
    fn override_secret_key() {
        let mut cfg = base_config();
        with_env_var(
            "ATRG_APP__SECRET_KEY",
            "new-super-secret-key-for-prod!!",
            || {
                apply_env_overrides(&mut cfg);
            },
        );
        assert_eq!(cfg.app.secret_key, "new-super-secret-key-for-prod!!");
    }

    #[test]
    fn override_database_url() {
        let mut cfg = base_config();
        with_env_var("ATRG_DATABASE__URL", "sqlite://prod.db", || {
            apply_env_overrides(&mut cfg);
        });
        assert_eq!(cfg.database.url, "sqlite://prod.db");
    }

    #[test]
    fn override_cors_origins_comma_separated() {
        let mut cfg = base_config();
        with_env_var(
            "ATRG_APP__CORS_ORIGINS",
            "http://localhost:5173, https://example.com , https://app.example.com",
            || {
                apply_env_overrides(&mut cfg);
            },
        );
        assert_eq!(
            cfg.app.cors_origins,
            vec![
                "http://localhost:5173",
                "https://example.com",
                "https://app.example.com",
            ]
        );
    }

    #[test]
    fn invalid_port_is_ignored() {
        let mut cfg = base_config();
        let original_port = cfg.app.port;
        with_env_var("ATRG_APP__PORT", "not_a_number", || {
            apply_env_overrides(&mut cfg);
        });
        assert_eq!(cfg.app.port, original_port);
    }

    #[test]
    fn override_app_name() {
        let mut cfg = base_config();
        with_env_var("ATRG_APP__NAME", "overridden-app", || {
            apply_env_overrides(&mut cfg);
        });
        assert_eq!(cfg.app.name, "overridden-app");
    }

    #[test]
    fn override_auth_client_id() {
        let mut cfg = base_config();
        with_env_var(
            "ATRG_AUTH__CLIENT_ID",
            "https://prod.example.com/client-metadata.json",
            || {
                apply_env_overrides(&mut cfg);
            },
        );
        assert_eq!(
            cfg.auth.client_id,
            "https://prod.example.com/client-metadata.json"
        );
    }

    #[test]
    fn empty_cors_origins_value_produces_empty_vec() {
        let mut cfg = base_config();
        cfg.app.cors_origins = vec!["http://existing.example.com".into()];
        with_env_var("ATRG_APP__CORS_ORIGINS", "", || {
            apply_env_overrides(&mut cfg);
        });
        assert!(cfg.app.cors_origins.is_empty());
    }

    #[test]
    fn absent_env_vars_leave_config_unchanged() {
        // Make sure none of the ATRG_ vars are set.
        for key in &[
            "ATRG_APP__NAME",
            "ATRG_APP__HOST",
            "ATRG_APP__PORT",
            "ATRG_APP__SECRET_KEY",
            "ATRG_APP__ENVIRONMENT",
            "ATRG_APP__CORS_ORIGINS",
            "ATRG_AUTH__CLIENT_ID",
            "ATRG_AUTH__REDIRECT_URI",
            "ATRG_AUTH__SCOPE",
            "ATRG_DATABASE__URL",
        ] {
            std::env::remove_var(key);
        }

        let mut cfg = base_config();
        let before = format!("{:?}", cfg);
        apply_env_overrides(&mut cfg);
        let after = format!("{:?}", cfg);
        assert_eq!(before, after);
    }
}
