# Changelog

All notable changes to at-rust-go are documented in this file.

## [0.2.0] — 2026-05-12

### Added

#### AppState Extensions
- `Extensions` type-map in `AppState` — apps attach custom typed state via `AtrgApp::with_extension::<T>()` and retrieve via `state.extension::<T>()`. Eliminates `once_cell` global state workarounds.

#### Migration Namespace Isolation
- `run_isolated_migrations(pool, dir, tracking_table)` — framework and app migrations use separate tracking tables (`_atrg_migrations` vs `_app_migrations`), preventing conflicts in multi-crate workspaces.
- `AtrgApp::run()` now uses `run_isolated_migrations` instead of the deprecated `run_user_migrations`.

#### API Key Authentication (`atrg-auth`)
- Native API key support: `create_api_key`, `find_by_key`, `list_api_keys`, `revoke_api_key`.
- `RequireAuth` extractor recognizes API key tokens (prefix-based, e.g. `atrg_*`) alongside OAuth sessions and JWTs.
- `AuthSource::ApiKey` variant added.

#### Role-Based Access Control (`atrg-auth`)
- `has_role`, `grant_role`, `revoke_role` with optional per-resource scoping.
- `is_banned`, `ban_did`, `lift_ban` with TTL-based auto-expiry.
- `bootstrap_admins` for provisioning admin DIDs on startup via config/env var.
- SQL DDL constants for both SQLite and PostgreSQL.

#### Blob Storage (`atrg-blob`) — NEW CRATE
- `BlobStore` trait with `put`, `get`, `exists`, `delete`.
- `S3BlobStore` backend (via `rust-s3`).
- `FileBlobStore` backend for development/testing.
- `compute_cid()` — SHA-256 content addressing with `sha256-` prefix.

#### Email / OTP (`atrg-email`) — NEW CRATE
- SMTP email delivery via `lettre` with starttls/tls/none modes.
- `send_otp` / `verify_otp` — two-step OTP verification flow with DB storage.
- Dev mode: logs OTPs to stdout when SMTP is not configured.
- `validate_domain` helper for email domain allowlists.

#### Event Router (`atrg-stream`)
- `EventRouterBuilder` — typed event dispatch by collection and operation.
- `CommitEvent` struct with flattened fields for handler convenience.
- `.on_create()`, `.on_update()`, `.on_delete()`, `.on()` registration methods.
- Produces a closure compatible with `AtrgApp::on_event()`.

#### Jetstream Cursor Persistence (`atrg-stream`)
- `cursor::save_cursor` / `cursor::load_cursor` — persist last processed `time_us` across restarts.
- `spawn_consumer_with_cursor` — enhanced consumer that auto-saves cursor every 100 events.
- `StreamConfig::cursor` field: `"live"` (default), `"auto"` (resume), or explicit timestamp.

#### Cross-Origin Auth Handoff
- `[auth] post_login_redirect` config — after OAuth callback, redirects to frontend URL with `?token=&did=&handle=` params for SPA integration.
- `ATRG_AUTH__POST_LOGIN_REDIRECT` env var override.

#### Admin Bootstrap
- `[app] admin_dids` config + `ATRG_APP__ADMIN_DIDS` env var — auto-provisions admin roles on startup.

#### App-Specific Config
- `config::load_app_config::<T>(section)` — deserialize custom `[section]` blocks from `atrg.toml`.

#### Multi-Binary Template
- `atrg new --template multi-binary` — scaffolds a workspace with write server + read aggregator + shared types crate.

#### Rate Limiting Integration
- Rate limiting middleware automatically applied in `AtrgApp::run()` when `[rate_limit]` is configured, with background cleanup of stale buckets.

#### Postgres E2E Test Suite
- 28 integration tests against real PostgreSQL covering migrations, sessions, API keys, RBAC, bans, cursors, OTP, and full-stack scenarios.
- `scripts/pg-e2e.sh` — ephemeral Postgres runner for local testing.
- `.github/workflows/postgres-e2e.yml` — CI workflow with Postgres service container.

#### Website
- Production-quality landing page at `site/index.html` (dark theme, feature grid, code examples).
- `scripts/build-docs.sh` — CI-time markdown-to-HTML doc page generator from `docs/*.md`.

### Fixed
- `client-metadata.json` now derives `client_uri` from `client_id` (fixes `http://0.0.0.0:3000` rejection by PDS).
- `well_known` endpoint uses same `origin_of()` derivation.
- Migration runner uses `sqlx::raw_sql()` instead of `sqlx::query()` for multi-statement SQL (fixes Postgres `cannot insert multiple commands` error).

### Changed
- `run_user_migrations` deprecated in favor of `run_isolated_migrations`.
- `AppState` now has an `extensions: Arc<Extensions>` field (all downstream test code updated).
- `AppConfig` gains `admin_dids: Vec<String>` field (defaults to empty).
- `StreamConfig` gains `cursor: Option<String>` field.

### Deprecated
- `atrg_db::run_user_migrations` — use `run_isolated_migrations` with a custom tracking table name.

## [0.1.1] — 2026-04-26

### Fixed
- Corrected license to LGPL-3.0-only across all Cargo.toml and documentation (was incorrectly MIT OR Apache-2.0).
- Fixed `unwrap()` in doc example in `atrg-repo`.

### Added
- README.md for all 13 crates (displayed on crates.io).
- `homepage` field in workspace Cargo.toml.
- `readme` field in all crate Cargo.toml files.

## [0.1.0] — 2026-04-26

### Added

#### Core Framework
- `atrg-core`: AppState, Config (atrg.toml), AtrgApp builder, AtrgError, CORS, security headers, request IDs, health/readiness endpoints, pagination helpers.
- `atrg-db`: SQLite connection pool via sqlx, internal + user migration runner.
- `atrg-identity`: DID/handle resolution with moka TTL-backed cache.
- `atrg-cli`: `atrg new`, `atrg dev`, `atrg migrate`, `atrg routes`, `atrg build`, `atrg generate` commands.

#### Authentication
- `atrg-auth`: OAuth login/callback/logout routes, session management, AT Protocol JWT verification, `AuthUser`/`RequireAuth` extractors, transparent token refresh.

#### Real-time Events
- `atrg-stream`: Jetstream WebSocket consumer with bounded backpressure, lag detection, ZSTD dictionary support, automatic reconnection.
- `atrg-firehose`: Full relay firehose (`com.atproto.sync.subscribeRepos`) consumer with CAR v1 decoding, CBOR→JSON conversion, cursor tracking.

#### XRPC & Codegen
- `atrg-xrpc`: XRPC router factory, AT Protocol error envelope, helper constructors.
- `atrg-codegen`: Lexicon JSON → Rust types, validators, and XRPC route stubs.

#### Record Operations
- `atrg-repo`: Typed record CRUD (`get/list/create/put/delete`), blob uploads, AT-URI parsing/validation, TID generation.

#### Feed & Label Frameworks
- `atrg-feed`: Feed generator builder, `describeFeedGenerator` + `getFeedSkeleton` XRPC routes, multi-feed support.
- `atrg-label`: Label service (create/negate/query), SQLite-backed label store, placeholder signing, `queryLabels` XRPC route.

#### Production Hardening
- Graceful shutdown (SIGTERM/SIGINT handling, DB pool drain with timeout).
- Per-IP token-bucket rate limiting middleware.
- Environment variable overrides for all atrg.toml fields (`ATRG_APP__PORT`, etc.).

#### Testing
- `atrg-testing`: `test_state()` with in-memory SQLite, `seed_session()`, `MockAtprotoClient`, `FakeJetstream`.

#### CI/CD
- GitHub Actions: fmt, clippy, test, coverage (per-crate thresholds), E2E scaffold tests, codegen E2E, documentation build.
- GitHub Pages workflow for API docs, llms.txt, llms-full.txt.