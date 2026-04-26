# Getting Started with atrg

## Prerequisites

- Rust stable toolchain (install via [rustup](https://rustup.rs/))
- SQLite (comes with most operating systems)

## Install the CLI

```bash
cargo install atrg-cli
```

## Create a Project

```bash
atrg new my-app
cd my-app
```

This creates a ready-to-run AT Protocol API project with:
- Pre-configured `Cargo.toml` with all atrg dependencies
- `atrg.toml` configuration file
- `src/main.rs` entry point (5 lines)
- `src/routes.rs` with example JSON handlers
- `migrations/` directory for your SQL schemas

## Run the Dev Server

```bash
atrg dev
```

Your API server is now running at `http://localhost:3000/`.

## Verify It Works

```bash
# Health check
curl http://localhost:3000/
# → {"name":"my-app","status":"ok"}

# Readiness probe
curl http://localhost:3000/readyz
# → {"ok":true,"database":"connected",...}

# 404 returns JSON
curl http://localhost:3000/nonexistent
# → {"error":"not_found","message":"Not found"}
```

## Configuration

Edit `atrg.toml` to configure your application:

```toml
[app]
name = "my-app"
host = "127.0.0.1"
port = 3000
secret_key = "change-this-in-production"
cors_origins = ["http://localhost:5173"]

[auth]
client_id = "http://localhost:3000/client-metadata.json"
redirect_uri = "http://localhost:3000/auth/callback"

[database]
url = "sqlite://atrg.db"
```

> **Tip:** Additional optional sections are available: `[jetstream]`, `[firehose]`, `[feed_generator]`, `[labeler]`, `[rate_limit]`. See the [full configuration reference](https://github.com/tellmeY18/at-rust-go/blob/main/README.md#configuration) for details.

## Next Steps

- [OAuth Authentication](oauth.md)
- [Jetstream Event Streaming](jetstream.md)
- [XRPC Procedures](xrpc.md)
- [Code Generation](codegen.md)
- [Testing](testing.md)