//! `atrg new <name>` — scaffold a new at-rust-go API project.
//!
//! Generates a complete project directory with Cargo.toml, atrg.toml,
//! src/main.rs, src/routes.rs, migrations/.gitkeep, .gitignore, and README.md.
//! Template placeholders `{{name}}` and `{{secret_key}}` are replaced with
//! the actual project name and a cryptographically random 32-byte hex key.

use std::path::Path;

use anyhow::{bail, Context};
use rand::RngCore;

// ---------------------------------------------------------------------------
// Embedded scaffold templates
// ---------------------------------------------------------------------------

const TMPL_CARGO_TOML: &str = r#"[package]
name = "{{name}}"
version = "0.1.0"
edition = "2021"

[dependencies]
atrg-core = "0.1"
atrg-auth = "0.1"
atrg-db = "0.1"
axum = { version = "0.8", features = ["macros"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Uncomment to enable additional features:
# atrg-repo = "0.1"        # Record CRUD, blob uploads, AT-URI helpers
# atrg-xrpc = "0.1"        # XRPC route helpers
# atrg-feed = "0.1"        # Feed generator framework
# atrg-label = "0.1"       # Labeler framework
# atrg-firehose = "0.1"    # Full relay firehose consumer
"#;

const TMPL_RUST_TOOLCHAIN: &str = r#"[toolchain]
channel = "stable"
"#;

const TMPL_ATRG_TOML: &str = r#"[app]
name = "{{name}}"
host = "127.0.0.1"
port = 3000
secret_key = "{{secret_key}}"
cors_origins = ["http://localhost:5173"]
environment = "development"

[auth]
client_id = "http://localhost:3000/client-metadata.json"
redirect_uri = "http://localhost:3000/auth/callback"
scope = "atproto transition:generic"

[database]
# atrg supports SQLite (default) and PostgreSQL. The backend is chosen from
# the URL scheme:
#   - sqlite://atrg.db          (file-backed SQLite)
#   - sqlite::memory:           (in-memory SQLite, useful for tests)
#   - postgres://user:pw@host/db (requires building atrg with the
#                                 `postgres` feature, e.g.
#                                 `cargo build --features atrg-cli/postgres`)
url = "sqlite://atrg.db"

# Uncomment to enable Jetstream
# [jetstream]
# host = "jetstream1.us-east.bsky.network"
# collections = ["app.bsky.feed.post"]

# Uncomment to enable relay firehose (full com.atproto.sync.subscribeRepos)
# [firehose]
# relay = "wss://bsky.network"

# Uncomment to run as a feed generator
# [feed_generator]
# did = "did:web:feeds.example.com"

# Uncomment to run as a labeler
# [labeler]
# did = "did:web:labels.example.com"
# signing_key_path = "keys/labeler.pem"

# Uncomment to enable rate limiting
# [rate_limit]
# requests_per_second = 10.0
# burst = 50
# enabled = true
"#;

const TMPL_MAIN_RS: &str = r#"use atrg_core::AtrgApp;

mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    AtrgApp::new()
        .with_auth_routes(atrg_auth::routes::auth_router())
        .with_cleanup_task(atrg_auth::routes::spawn_cleanup_task)
        .mount(routes::api())
        .run()
        .await
}
"#;

const TMPL_ROUTES_RS: &str = r#"use axum::{Router, routing::get, Json};
use atrg_core::AppState;
use serde_json::json;

pub fn api() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/api/me", get(me))
}

async fn index() -> Json<serde_json::Value> {
    Json(json!({ "name": "{{name}}", "status": "ok" }))
}

async fn me() -> Json<serde_json::Value> {
    // TODO: Wire up AuthUser extractor in Phase 2
    Json(json!({ "authenticated": false }))
}
"#;

const TMPL_GITIGNORE: &str = "/target
*.db
*.db-journal
*.db-wal
*.db-shm
.env
";

const TMPL_README: &str = "# {{name}}

An AT Protocol API server built with [at-rust-go](https://github.com/tellmeY18/at-rust-go).

## Getting started

    atrg dev

## API Endpoints

- `GET /` - Health check
- `GET /api/me` - Current user info (requires auth)
";

// ---------------------------------------------------------------------------
// Command implementation
// ---------------------------------------------------------------------------

/// Run the `atrg new` command.
pub fn run(name: &str, template: &str, path: Option<&str>, force: bool) -> anyhow::Result<()> {
    if template != "basic" {
        bail!(
            "Template '{}' is not yet available. Use 'basic' (default).",
            template
        );
    }

    let target = Path::new(path.unwrap_or(name));

    // Check target directory
    if target.exists() {
        let is_empty = target
            .read_dir()
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);

        if !is_empty && !force {
            bail!(
                "Directory '{}' already exists and is not empty. Use --force to overwrite.",
                target.display()
            );
        }
    }

    // Generate a cryptographically random 32-byte secret key
    let mut key_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key_bytes);
    let secret_key = hex::encode(key_bytes);

    // Helper: substitute placeholders and write a file
    let write_file = |rel_path: &str, content: &str| -> anyhow::Result<()> {
        let full_path = target.join(rel_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        let rendered = content
            .replace("{{name}}", name)
            .replace("{{secret_key}}", &secret_key);
        std::fs::write(&full_path, rendered)
            .with_context(|| format!("Failed to write: {}", full_path.display()))?;
        Ok(())
    };

    // Write all scaffold files
    write_file("Cargo.toml", TMPL_CARGO_TOML)?;
    write_file("rust-toolchain.toml", TMPL_RUST_TOOLCHAIN)?;
    write_file("atrg.toml", TMPL_ATRG_TOML)?;
    write_file("src/main.rs", TMPL_MAIN_RS)?;
    write_file("src/routes.rs", TMPL_ROUTES_RS)?;
    write_file("migrations/.gitkeep", "")?;
    write_file(".gitignore", TMPL_GITIGNORE)?;
    write_file("README.md", TMPL_README)?;

    println!();
    println!("  \u{2713} Created {}", name);
    println!();
    println!("  Next steps:");
    println!("    cd {}", path.unwrap_or(name));
    println!("    atrg dev");
    println!();

    Ok(())
}
