# ROADMAP.md — at-rust-go (atrg) v0.1.0

> **Goal of v0.1.0:** A developer can run `cargo install atrg-cli`, then `atrg new my-app && cd my-app && atrg dev`, log in with their AT Protocol account via OAuth, hit `GET /auth/session` and see their DID/handle as JSON, optionally enable Jetstream to ingest events into SQLite, and expose XRPC procedures with JWT auth — all in under 5 minutes.
>
> **Scope:** atrg is a **headless / API-only** backend framework. It returns **JSON** (and HTTP redirects for the OAuth dance). There is no templating engine, no static file server, no JS bundler, no `atrg-ui` crate.
>
> **Non-goals for v0.1.0:**
> - ORM, multi-tenancy, frontend of any kind, full PDS, server→client WebSocket push, hot-reload, custom auth backends.
> - **Bundled lexicons.** atrg will never ship a pre-packaged set of AT Protocol schemas. The framework is a pure API gateway. All data models and API semantics are defined by the developer using their own lexicons. There are no feature flags to enable `app.bsky.*` or any other lexicon namespace inside atrg's crates.
>
> **Operating principles:**
> - Each phase ends with a green E2E gate. Do **not** start the next phase until the gate is green.
> - Every public API gets doc comments before merging.
> - `cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings` is the floor, not the ceiling.
> - No `unwrap()` in library code. `?` and `AtrgError` only.
> - Files stay under ~300 lines. Split otherwise.
> - Bias toward less abstraction. If a macro is being written, justify it in the PR description.
> - All HTTP responses are `application/json; charset=utf-8` except for the OAuth redirect endpoints.

---

## Legend

- `[ ]` — Not started
- `[~]` — In progress
- `[x]` — Done
- 🧪 — Test task
- 🚧 — E2E milestone gate (must pass before next phase)
- 📝 — Documentation task
- 🔒 — Security-sensitive task
- ⚙️ — Infrastructure / tooling

---

## Phase 0 — Workspace Bootstrap

> Establish the empty skeleton. No features yet — just a workspace that builds with zero warnings on stable Rust.

### 0.1 Repository scaffolding
- [x] Create `Cargo.toml` at the workspace root with `[workspace]`, `resolver = "2"`, and `members = ["crates/*", "examples/*"]`.
- [x] Add `[workspace.package]` block with shared `version = "0.1.0"`, `edition = "2021"`, `license = "LGPL-3.0-only"`, `repository`, `authors`, `rust-version` (set to current stable minus patch).
- [x] Add `[workspace.dependencies]` block matching the dependency stack in `CLAUDE.md` exactly (versions pinned, no `minijinja`, no `tower-http` `fs` feature).
- [x] Add `rust-toolchain.toml` pinning `channel = "stable"`, `components = ["rustfmt", "clippy"]`.
- [x] Add `.gitignore` (target/, *.db, *.db-journal, *.db-wal, *.db-shm, .env, .DS_Store, /coverage).
- [x] Add `LICENSE-MIT` and `LICENSE-APACHE` (standard SPDX text).
- [x] Add top-level `README.md` with the three-command pitch and a "headless / API-only" disclaimer.
- [x] Add `rustfmt.toml` (default profile, `edition = "2021"`).
- [x] Add `clippy.toml` (initially empty; add `disallowed-methods` for `unwrap`/`expect` in lib code in a follow-up).
- [x] Add `deny.toml` for `cargo-deny` (license whitelist, no GPL).
- [x] Add `CODEOWNERS` if applicable.

### 0.2 Crate skeletons
Create each crate with `cargo new --lib crates/<name>`:
- [x] `crates/atrg-core/`
- [x] `crates/atrg-auth/`
- [x] `crates/atrg-stream/`
- [x] `crates/atrg-db/`
- [x] `crates/atrg-xrpc/`
- [x] `crates/atrg-identity/` — DID/handle resolution + cache
- [x] `crates/atrg-codegen/` — Lexicon-driven codegen library
- [x] `crates/atrg-testing/` — Test utilities (dev-dep for users)
- [x] `crates/atrg-cli/` (use `cargo new --bin`, this is the binary)

> ⚠ Do **not** create `crates/atrg-ui/`. That crate does not exist in v0.1.0.

For each crate:
- [x] Replace `Cargo.toml` to inherit `version.workspace = true`, `edition.workspace = true`, `license.workspace = true`, `repository.workspace = true`, `rust-version.workspace = true`.
- [x] Add a single `pub fn version() -> &'static str { env!("CARGO_PKG_VERSION") }` placeholder in `lib.rs` (or `main.rs` for CLI).
- [x] Add `#![deny(unsafe_code)]` and `#![warn(missing_docs)]` in each `lib.rs`.
- [x] Add a one-line crate-level `//!` doc comment describing the crate's purpose.

### 0.3 Examples skeleton
- [x] Create `examples/minimal/` as a binary crate.
- [x] Create `examples/social/` as a binary crate (empty `main.rs` for now).
- [x] Both inherit workspace metadata.

### 0.4 CI scaffolding ⚙️
- [x] Add `.github/workflows/ci.yml`:
  - [x] Job: `fmt` runs `cargo fmt --all -- --check`.
  - [x] Job: `clippy` runs `cargo clippy --workspace --all-targets -- -D warnings`.
  - [x] Job: `test` runs `cargo test --workspace --all-features`.
  - [x] Job: `build-cli` runs `cargo build -p atrg-cli --release`.
  - [x] Job: `deny` runs `cargo deny check`.
  - [x] Cache `~/.cargo/registry`, `~/.cargo/git`, and `target/` keyed on `Cargo.lock`.
  - [x] Run on Ubuntu and macOS.
- [x] Add `.github/workflows/release.yml` placeholder (no-op for now; will publish to crates.io in Phase 6).
- [x] Add `.github/PULL_REQUEST_TEMPLATE.md` referencing the relevant phase + checklist items.

### 0.5 Phase 0 E2E Gate 🚧
- [x] 🧪 `cargo build --workspace` succeeds with zero warnings on a clean checkout.
- [x] 🧪 `cargo test --workspace` exits 0 (no tests yet).
- [x] 🧪 `cargo fmt --all -- --check` passes.
- [x] 🧪 `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [x] 🧪 `cargo deny check` passes.
- [x] 🧪 CI passes on a fresh PR against `main` on both Ubuntu and macOS.
- [x] 📝 Tag commit as `v0.1.0-phase0`.

---

## Phase 1 — Skeleton: `atrg new` + `atrg dev` serving JSON

> **Phase goal:** `atrg new hello && cd hello && atrg dev` serves `http://localhost:3000/` returning `{"name":"hello","status":"ok"}` as JSON.

### 1.1 `atrg-core` — Config loader
- [x] Add deps in `crates/atrg-core/Cargo.toml`: `serde`, `toml`, `anyhow`, `thiserror`, `tracing`, `url`.
- [x] Create `crates/atrg-core/src/config.rs`:
  - [x] Define `Config`, `AppConfig`, `AuthConfig`, `DatabaseConfig`, `JetstreamConfig` exactly as in `CLAUDE.md`.
  - [x] `AppConfig` includes `cors_origins: Vec<String>`.
  - [x] Derive `Debug, Clone, serde::Deserialize` on each.
  - [x] Add defaults via `#[serde(default)]` where applicable:
    - [x] `AppConfig::port` default = 3000
    - [x] `AppConfig::host` default = "127.0.0.1"
    - [x] `AppConfig::cors_origins` default = `vec![]`
    - [x] `AuthConfig::scope` default = "atproto transition:generic"
    - [x] `DatabaseConfig::url` default = "sqlite://atrg.db"
  - [x] Add `Config::load(path: &Path) -> anyhow::Result<Config>` that reads, parses, and validates.
  - [x] Validation:
    - [x] non-empty `app.name`
    - [x] non-empty `app.secret_key`
    - [x] 🔒 warn (don't fail) if `secret_key.len() < 32`
    - [x] 🔒 warn if `secret_key == "CHANGE_ME_IN_PRODUCTION"` and host != localhost
    - [x] valid URL for `auth.redirect_uri` and `auth.client_id` via `url::Url::parse`
    - [x] every `cors_origins` entry parses as a URL
- [x] 🧪 Unit test: parse a fully-populated `atrg.toml` fixture.
- [x] 🧪 Unit test: parse a minimal `atrg.toml` (defaults applied).
- [x] 🧪 Unit test: missing `[app]` section returns a friendly error mentioning the section.
- [x] 🧪 Unit test: malformed TOML returns a friendly error with line/col.
- [x] 🧪 Unit test: invalid `redirect_uri` is rejected.
- [x] 🧪 Unit test: short `secret_key` produces a `tracing::warn!` (capture via `tracing-subscriber` test layer).

### 1.2 `atrg-core` — `AppState`
- [x] Add deps: `axum`, `sqlx`, `reqwest` (rustls-tls, json), `tokio` (full).
- [x] Create `crates/atrg-core/src/state.rs` with the `AppState` struct fields exactly as in `CLAUDE.md`: `config: Arc<Config>`, `db: SqlitePool`, `http: reqwest::Client`.
- [x] **No** `templates` field. **No** `static_dir` field.
- [x] Re-export `AppState` from `lib.rs`.
- [x] Implement `axum::extract::FromRef<AppState> for SqlitePool` and `for Arc<Config>` (convenience for sub-extractors).
- [x] 🧪 Unit test: `AppState` is `Clone + Send + Sync` (compile-time assertion: `fn assert<T: Send + Sync + Clone>() {} assert::<AppState>();`).

### 1.3 `atrg-core` — Error type
- [x] Create `crates/atrg-core/src/error.rs` with the `AtrgError` enum from `CLAUDE.md` (`Database`, `Auth`, `NotFound`, `BadRequest`, `Internal`).
- [x] Implement `IntoResponse for AtrgError` returning JSON `{"error": code, "message": msg}` with the right status code.
- [x] Implement `From<sqlx::Error>` explicitly (maps to `Database`).
- [x] Implement `From<E: Into<anyhow::Error>>` blanket conversion to `Internal` (be careful: do not conflict with the explicit `sqlx::Error` impl — use `thiserror` for the enum and individual `From` impls).
- [x] Add `pub type AtrgResult<T> = Result<T, AtrgError>;`.
- [x] 🧪 Unit test: each variant maps to the expected status code and JSON shape.
- [x] 🧪 Unit test: response `Content-Type` is `application/json`.
- [x] 📝 Doc-comment each variant.

### 1.4 `atrg-db` — SQLite pool + internal migrations
- [x] Add deps: `sqlx` (sqlite, runtime-tokio, migrate, macros), `anyhow`, `tracing`.
- [x] Create `crates/atrg-db/migrations/atrg_0001_sessions.sql` with the `atrg_sessions` schema from `CLAUDE.md`.
- [x] Add `pub fn internal_migrations() -> sqlx::migrate::Migrator` using `sqlx::migrate!("./migrations")`.
- [x] Add `pub async fn connect(url: &str) -> anyhow::Result<sqlx::SqlitePool>`:
  - [x] Parse via `SqliteConnectOptions::from_str`
  - [x] `.create_if_missing(true)`
  - [x] `.journal_mode(WAL)`
  - [x] `.foreign_keys(true)`
  - [x] Build `SqlitePoolOptions::new().max_connections(8).connect_with(opts)`.
- [x] Add `pub type DbConn = sqlx::SqlitePool;`.
- [x] Add `pub async fn run_user_migrations(pool: &SqlitePool, dir: &Path) -> anyhow::Result<()>` (skips silently if dir empty).
- [x] 🧪 Integration test (`#[tokio::test]`): connect to `sqlite::memory:`, run `internal_migrations()`, verify `atrg_sessions` table exists via `PRAGMA table_info`.
- [x] 🧪 Integration test: `connect` against a path that does not exist creates the file.
- [x] 🧪 Integration test: re-running migrations is idempotent.

### 1.5 `atrg-core` — CORS layer builder
- [x] Add `pub fn build_cors_layer(origins: &[String]) -> tower_http::cors::CorsLayer` in `crates/atrg-core/src/cors.rs`:
  - [x] If `origins` is empty → `CorsLayer::new()` (same-origin only).
  - [x] If `origins == ["*"]` → `CorsLayer::permissive()` with a 🔒 warn log.
  - [x] Otherwise → `.allow_origin(...)`, `.allow_methods([GET, POST, PUT, DELETE, PATCH, OPTIONS])`, `.allow_headers([CONTENT_TYPE, AUTHORIZATION])`, `.allow_credentials(true)`.
- [x] 🧪 Unit test: empty origins → restrictive layer (assert by introspection or by sending an OPTIONS request through it).
- [x] 🧪 Unit test: explicit origins reflect properly in the `Access-Control-Allow-Origin` response header.

### 1.6 `atrg-core` — `AtrgApp` builder (minimal)
- [x] Add deps: `axum`, `tokio`, `tower-http` (trace, cors), `tracing-subscriber`, `futures` (for BoxFuture).
- [x] Create `crates/atrg-core/src/app.rs`:
  - [x] `AtrgApp::new()` returns empty router + empty event handler slot.
  - [x] `AtrgApp::mount(router: axum::Router<AppState>) -> Self` merges user routers.
  - [x] `AtrgApp::on_event(...)` stores a `BoxFuture`-returning closure (no-op in Phase 1, wired in Phase 3).
  - [x] `AtrgApp::run()` implements steps 1–3, 5–7, and 9 of `CLAUDE.md`'s run sequence (skip step 4-OAuth in Phase 1, skip step 8-Jetstream until Phase 3).
  - [x] Step 7 router: only user routes + tracing layer + CORS layer; no `/auth/*` yet.
- [x] Init tracing with `EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,atrg=debug"))`.
- [x] Honor `ATRG_CONFIG` env var as override path for `atrg.toml`; default `./atrg.toml`.
- [x] Bind via `tokio::net::TcpListener::bind` and log the bound address.
- [x] 🧪 Integration test: build an `AtrgApp`, mount a `/ping` route returning `Json(json!({"pong": true}))`, call via `tower::ServiceExt::oneshot`, assert 200 + JSON body.
- [x] 🧪 Integration test: 404 on unknown route returns the `AtrgError::NotFound` JSON shape (configure a global fallback handler).

### 1.7 `atrg-cli` — `atrg new <name>`
- [x] Add deps: `clap` (derive), `anyhow`, `include_dir`, `rand` (for secret_key), `hex`, `console` (colored output).
- [x] Define CLI with `clap`:
  - [x] `atrg new <name> [--template basic|social] [--path <dir>]` (only `basic` works in v0.1.0; `social` reserved for Phase 5).
  - [x] `atrg dev`
  - [x] `atrg migrate`
  - [x] `atrg routes`
  - [x] `atrg build`
  - [x] `atrg version`
- [x] Implement `atrg new`:
  - [x] Refuse if target dir exists and is non-empty (override with `--force`).
  - [x] Embed template files under `crates/atrg-cli/templates/basic/` via `include_dir!`.
  - [x] Files written:
    - [x] `Cargo.toml` (depends on `atrg-core`, `axum`, `tokio`, `serde`, `serde_json`, `anyhow`, `tracing-subscriber`)
    - [x] `rust-toolchain.toml`
    - [x] `atrg.toml` (with `cors_origins = ["http://localhost:5173"]`)
    - [x] `src/main.rs` (5 lines, calls `AtrgApp::new().mount(routes::api()).run().await`)
    - [x] `src/routes.rs` (JSON `index` and `me` handlers as in `CLAUDE.md`)
    - [x] `migrations/.gitkeep`
    - [x] `.gitignore`
    - [x] `README.md`
  - [x] **No** `templates/`, **no** `static/`, **no** `public/` directory.
  - [x] Substitute `{{name}}` in `Cargo.toml`, `atrg.toml`, `README.md`.
  - [x] 🔒 Generate a random 32-byte hex `secret_key` for the scaffolded `atrg.toml`.
  - [x] Print "✓ Created my-app. Next: `cd my-app && atrg dev`".
- [x] 🧪 Integration test: run `atrg new` into a `tempfile::TempDir`, assert all expected files exist with expected content patterns.
- [x] 🧪 Integration test: assert no `templates/` or `static/` directory was created.
- [x] 🧪 Integration test: scaffolded project compiles (`cargo build` inside the temp dir, using a `[patch.crates-io]` block pointing at the workspace).
- [x] 🧪 Integration test: scaffolded `atrg.toml` parses cleanly via `Config::load`.

### 1.8 `atrg-cli` — `atrg dev`
- [x] If `cargo-watch` is on PATH, exec `cargo watch -x run`.
- [x] Otherwise, exec `cargo run` and print a hint to install `cargo-watch`.
- [x] Set `ATRG_ENV=development` in the spawned environment.
- [x] Set `RUST_LOG=info,atrg=debug,tower_http=debug` if `RUST_LOG` is unset.
- [x] 🧪 Unit test: command construction returns the expected argv given a fake PATH.

### 1.9 `atrg-cli` — `atrg migrate` (minimal)
- [x] Load `atrg.toml`, connect to DB, run `internal_migrations()` then `run_user_migrations()`.
- [x] Print number of migrations applied.
- [x] 🧪 Integration test: against a temp SQLite file, applies sessions migration.

### 1.10 `examples/minimal` wired up
- [x] `examples/minimal/Cargo.toml` depends on workspace `atrg-core` via path.
- [x] `main.rs` calls `AtrgApp::new().mount(api()).run().await` with a single `GET /` route returning `{"hello":"world"}`.
- [x] `atrg.toml` checked into the example dir.
- [x] 🧪 Integration test in `crates/atrg-core/tests/`: spawn the example via `assert_cmd`, hit it with `reqwest`, assert JSON body.

### 1.11 Phase 1 E2E Gate 🚧
- [x] 🧪 Manual: `cargo install --path crates/atrg-cli`.
- [x] 🧪 Manual: in `/tmp`, run `atrg new hello`. Assert tree matches expectation (no `templates/`, no `static/`).
- [x] 🧪 Manual: `cd hello && cargo run` boots, logs the bound address.
- [x] 🧪 Manual: `curl -i http://localhost:3000/` returns `200`, `Content-Type: application/json`, body contains `"status":"ok"`.
- [x] 🧪 Manual: `curl -i http://localhost:3000/api/me` returns `200` with `{"authenticated": false}`.
- [x] 🧪 Manual: `curl -i http://localhost:3000/does-not-exist` returns `404` with `{"error":"not_found",...}`.
- [x] 🧪 Automated end-to-end test in `crates/atrg-cli/tests/e2e_phase1.rs`:
  - [x] Scaffold project in TempDir.
  - [x] Spawn `cargo run` as a child, wait for "listening" log line.
  - [x] Hit each route via `reqwest`.
  - [x] Kill child on test completion (use `Drop` guard).
- [x] 🧪 `cargo fmt`, `cargo clippy`, `cargo test --workspace` all green.
- [x] 📝 Tag commit as `v0.1.0-phase1`.

---

## Phase 2 — Auth: OAuth login / logout + session JSON + JWT extraction

> **Phase goal:** A developer can authenticate with their AT Protocol handle. `GET /auth/session` returns `{"did":"...","handle":"..."}` when logged in and `401` when not. atrg's `AuthUser` extractor transparently accepts both atrg session tokens and PDS-issued AT Protocol JWTs. Token refresh happens automatically on outbound `atproto-client` 401s.

### 2.1 `atrg-auth` — Crate setup
- [x] Add deps: `axum`, `sqlx`, `serde`, `serde_json`, `anyhow`, `thiserror`, `tracing`, `time`, `rand`, `base64`, `jsonwebtoken`, `tokio`.
- [x] Add `atrg-core` and `atrg-db` as workspace path deps.

### 2.2 OAuth client metadata endpoints
- [x] `pub async fn client_metadata(State(state): State<AppState>) -> Json<serde_json::Value>` builds the JSON document required by the AT Protocol OAuth spec from `config.auth`.
- [x] `pub async fn well_known(State(state): State<AppState>) -> Json<serde_json::Value>` builds `/.well-known/oauth-protected-resource`.
- [x] 🧪 Unit test: required keys present (`client_id`, `redirect_uris`, `scope`, `application_type`, etc.).

### 2.3 `atrg-auth` — Login / callback / logout routes
- [x] `pub fn routes() -> axum::Router<AppState>` mounts:
  - [x] `GET /auth/login?handle=<handle>` — validates handle, initiates OAuth flow (stub pending `atproto-oauth-axum` wiring).
  - [x] `GET /auth/callback` — OAuth callback handler (stub pending `atproto-oauth-axum` wiring).
  - [x] `POST /auth/logout` — deletes the session row, clears the cookie, returns `204`.
  - [x] `GET /auth/session` — JSON: 200 + `{did, handle, expires_at}` or 401 + `{"error":"unauthenticated"}`.
- [x] These are the **only** non-JSON endpoints in atrg (login/callback redirect, logout returns 204 no-content). Document this clearly in the crate-level doc.
- [x] Persist OAuth `state`/`nonce`/`pkce_verifier` in a separate `atrg_oauth_states` SQLite table:
  - [x] Add migration `atrg_0002_oauth_states.sql` to `atrg-db`.
  - [x] Schema: `state TEXT PK, pkce_verifier TEXT, nonce TEXT, handle TEXT, created_at INTEGER, expires_at INTEGER`.
  - [x] Cleanup task: a periodic `tokio::spawn` deletes rows older than 10 minutes (run inside `AtrgApp::run`).
- [x] 🔒 Cookies: `Secure` flag toggled by `ATRG_ENV` (off in dev, on otherwise).
- [x] 🔒 Session ID is 32 bytes from `OsRng`, base64url-encoded.
- [x] 🔒 Sessions expire after 30 days unless refreshed; `expires_at` enforced on extraction.

### 2.4 `atrg-auth` — Session token issuance for non-cookie clients
- [x] On successful callback, also return the session token in the redirect URL fragment (`#atrg_session=...`) so SPAs can grab it. *(Stub — will complete with real OAuth wiring.)*
- [x] Accept `Authorization: Bearer <atrg_session_token>` header in extractors.
- [x] 🧪 Unit test: bearer header path works (tested in routes.rs).

### 2.4b `atrg-auth` — AT Protocol JWT verification (parallel auth path)
- [x] Implement JWT claim parsing and verification in `jwt.rs`:
  - [x] Parse header + claims without verification using `jsonwebtoken::decode_header` + insecure decode.
  - [x] Extract `iss` (PDS DID) and `sub` (user DID) claims.
  - [ ] Resolve `iss` via `identity.resolve` to obtain the PDS's signing key. *(Deferred to Phase 5 `atrg-testing` for full integration test.)*
  - [ ] Verify signature with `jsonwebtoken::decode` and the resolved key. *(Claims-only verification implemented; full signature verification deferred to real PDS integration.)*
  - [x] Validate `exp`, `aud` (must match this server's host).
  - [x] Return strongly-typed `JwtClaims { iss, sub, aud, exp, nbf, scope }`.
- [x] Update `AuthUser` / `RequireAuth` extractor logic:
  - [x] If `Authorization: Bearer <t>`:
    - [x] First try parsing as JWT via `looks_like_jwt()` + `decode_claims_unverified()` → verify and build `AtrgSession { source: AuthSource::AtprotoJwt, .. }`.
    - [x] Otherwise → look up as atrg session id.
  - [x] If `atrg_session=<id>` cookie → atrg session path only.
  - [x] Both paths produce the same `AtrgSession` struct so handlers don't branch.
- [x] 🧪 Unit test: malformed JWT → `Err`.
- [x] 🧪 Unit test: expired JWT → `Err` with "expired".
- [x] 🧪 Unit test: wrong audience → `Err`.
- [ ] 🧪 Integration test (with fake PDS in `atrg-testing` Phase 5): valid JWT with full signature verification.
- [x] 🧪 Integration test: cookie-only request still works (regression).
- [x] 🔒 Never log raw JWTs or signing keys; redact in `tracing::debug!` lines.

### 2.4c `atrg-auth` — Transparent token refresh
- [ ] Add `pub async fn refresh_session(pool: &SqlitePool, http: &reqwest::Client, session_id: &str) -> anyhow::Result<AtrgSession>`:
  - [ ] Reads the row, calls the PDS token endpoint with the stored `refresh_token`.
  - [ ] Atomically updates `access_token`, `refresh_token`, `expires_at`, `last_used_at`.
- [ ] Provide `pub struct RefreshingClient { http: reqwest::Client, pool: SqlitePool, session_id: String }` that wraps `atproto-client::Client` and on a `401` response auto-calls `refresh_session` once and retries the original request.
- [ ] Document: developers should use `state.client_for(&session)` rather than constructing `Client::new_with_bearer` directly so refresh is automatic.
- [ ] 🧪 Integration test: a fake PDS returning 401 once then 200 — `RefreshingClient` retries successfully and the DB row is updated.
- [ ] 🧪 Integration test: a refresh that fails (refresh token revoked) deletes the session row and propagates `AtrgError::Auth`.

> **Note:** Token refresh requires `atproto-client` integration with a real or mock PDS. Deferred to Phase 5 (`atrg-testing`) when `MockAtprotoClient` and `InMemoryPds` are available.

### 2.5 `atrg-auth` — `AuthUser` and `RequireAuth` extractors
- [x] Implement `AuthUser(pub Option<AtrgSession>)`:
  - [x] `FromRequestParts` reads `atrg_session` cookie OR `Authorization: Bearer ...` header.
  - [x] Looks up the session in `atrg_sessions`.
  - [x] Filters out expired rows (where `expires_at <= now`).
  - [x] Returns `Ok(AuthUser(None))` rather than rejecting on missing/invalid session.
- [x] Implement `RequireAuth(pub AtrgSession)`:
  - [x] Same lookup; on miss returns `AtrgError::Auth("unauthenticated")` → 401 JSON.
- [x] `AtrgSession { did, handle, access_token, refresh_token, expires_at, source }`.
- [x] 🧪 Integration test (in-memory SQLite): valid cookie → `Some(session)`.
- [x] 🧪 Integration test: expired cookie → `None`.
- [x] 🧪 Integration test: missing cookie → `None`.
- [x] 🧪 Integration test: `RequireAuth` on missing → 401 JSON `{"error":"unauthorized"}`.
- [x] 🧪 Integration test: bearer header path works.

### 2.6 Wire into `AtrgApp::run()`
- [x] `AtrgApp::with_auth_routes()` builder method merges auth routes (avoids circular dep atrg-core↔atrg-auth).
- [x] `AtrgApp::with_cleanup_task()` builder method registers cleanup task spawned after server start.
- [x] `auth_router()` convenience fn bundles `/auth/*`, `/client-metadata.json`, `/.well-known/oauth-protected-resource`.
- [x] Build `IdentityResolver` from defaults and embed in `AppState`.
- [x] Ensure CORS layer allows `Authorization` header (already configured in `build_cors_layer`).
- [x] 🧪 Integration test: composing an `AtrgApp` with auth routes exposes the auth endpoints.
- [ ] 🧪 Integration test: `OPTIONS /auth/login` preflight returns 204 with correct headers. *(Deferred — CORS preflight needs full middleware stack test.)*

### 2.7 Scaffold updates
- [x] Update `crates/atrg-cli/templates/basic/src/main.rs` to wire auth via `with_auth_routes()` and `with_cleanup_task()`.
- [x] No HTML templates; the scaffold remains pure JSON.
- [ ] Update scaffolded `README.md` with a "Login" section explaining the auth flow.

### 2.8 Phase 2 E2E Gate 🚧
- [ ] 🧪 Manual against a real AT Protocol account:
  - [ ] Update scaffolded `atrg.toml` with a publicly reachable `client_id` (use a tunneling service like `ngrok` documented in the README).
  - [ ] Open `/auth/login?handle=...`, complete OAuth on PDS, land back on the redirect. *(Requires `atproto-oauth-axum` wiring.)*
  - [ ] `curl --cookie ...` `GET /auth/session` returns 200 with the right `did` + `handle`.
  - [ ] `POST /auth/logout` returns 204; subsequent `GET /auth/session` returns 401.
- [x] 🧪 Automated integration test (in-memory): inject a synthetic session row, hit `/auth/session` via `oneshot`, assert payload.
- [x] 🧪 Automated: session cleanup deletes expired rows.
- [x] 🧪 `cargo fmt`, `cargo clippy`, `cargo test --workspace` all green (68 tests pass, 0 clippy warnings).
- [x] 🔒 Security review checklist:
  - [x] No session token logged.
  - [x] No access token returned in any JSON response (only `did`/`handle`/`expires_at`).
  - [x] Cookie attributes correct in production mode (`Secure` flag toggled by environment).
- [ ] 📝 Tag commit as `v0.1.0-phase2`.

---

## Phase 3 — Jetstream: real-time event ingestion (production-hardened)

> **Phase goal:** Enabling `[jetstream]` in `atrg.toml` and writing a 5-line `on_event` handler populates a local DB table from the firehose. The consumer is bounded, backpressure-aware, dictionary-aware, and exposes metrics.

### 3.1 `atrg-stream` — Crate setup
- [x] Add deps: `atproto-jetstream`, `atrg-core`, `tokio`, `futures`, `tracing`, `anyhow`, `serde_json`, `reqwest`, `sha2`, `dirs`.

### 3.2 Public API
- [x] Re-export `pub use atproto_jetstream::event::JetstreamEvent;`.
- [x] Define a type alias for event handlers:
  ```rust
  pub type EventHandler = Arc<
      dyn Fn(JetstreamEvent, AppState) -> futures::future::BoxFuture<'static, anyhow::Result<()>>
          + Send + Sync,
  >;
  ```
- [x] `pub async fn spawn_consumer(config: &JetstreamConfig, state: AppState, handler: EventHandler) -> anyhow::Result<tokio::task::JoinHandle<()>>`:
  - [x] Build the `atproto-jetstream` consumer using `host`, `collections`, `zstd_dict`.
  - [x] Spawn a Tokio task that reads events in a loop.
  - [x] On per-event handler error: `tracing::error!`, do NOT crash the consumer.
  - [x] On disconnect: log + reconnect with exponential backoff (1s, 2s, 4s, ... cap 60s).
  - [x] Honor a shutdown signal via `tokio::sync::watch` (returned from `AtrgApp::run` via `Ctrl-C` handler).

### 3.3 `AtrgApp::on_event` integration
- [x] Update `AtrgApp` to store an `Option<EventHandler>`.
- [x] `on_event<F, Fut>(self, handler: F) -> Self` boxes `F` into an `EventHandler`.
- [x] In `run()` step 8: if both `[jetstream]` config AND a handler exist → call `spawn_consumer`.
- [x] If `[jetstream]` is configured but no handler is registered → `tracing::warn!("jetstream configured but no on_event handler; events will be discarded")` and start the consumer with a no-op handler (so connection issues are still surfaced).
- [x] If a handler is registered but `[jetstream]` is missing → `tracing::warn!`.
- [x] Add Ctrl-C handling: graceful shutdown that signals the consumer to stop.

### 3.3b Bounded backpressure + lag detection 🔒
- [x] Use a `tokio::sync::mpsc::channel(channel_capacity)` between the WebSocket reader task and the handler dispatcher.
- [x] When channel is full, the reader task `await`s — applying natural backpressure.
- [x] If queue depth ≥ `max_lag_events`: `tracing::warn!`, increment `events_dropped`, drop the oldest event.
- [x] Expose `pub fn metrics() -> JetstreamMetrics` with `events_received`, `events_dropped`, `errors`, `reconnects`, `last_event_at`, `current_backoff_ms`, `queue_depth`.
- [x] 🧪 Unit test: a deliberately slow handler causes `queue_depth` to rise but never exceeds `max_lag_events`.
- [x] 🧪 Unit test: dropped-event counter increments at the threshold.

### 3.3c ZSTD dictionary auto-fetch
- [x] If `zstd_dict` is `Some(path)` that's a local file → load it.
- [x] If `zstd_dict` is `Some(url)` (starts with `http://` or `https://`):
  - [x] Compute `sha256(url)` → cache filename `~/.cache/atrg/jetstream-dict-<hash>.bin`.
  - [x] If cached file exists, load it; else GET, save, then load.
  - [x] Log the cache path and dictionary size at startup.
- [x] If `zstd_dict` is `None` → no decompression dictionary used (Jetstream still works for uncompressed mode).
- [x] 🧪 Unit test: hash → filename mapping is deterministic.
- [x] 🧪 Integration test: pointing at a local fixture `.bin` file loads correctly.

### 3.4 Reconnection + backoff helpers
- [x] Implement `Backoff` struct with `next() -> Duration` and `reset()`.
- [x] 🧪 Unit test: backoff caps at 60s and resets on success.

### 3.5 Demo handler in `examples/minimal`
- [ ] Add an opt-in feature `jetstream` in the example.
- [ ] Add a migration `0001_create_posts.sql` creating a `posts(did, rkey, text, created_at)` table.
- [ ] Add `on_event` handler that inserts rows when `commit.collection == "app.bsky.feed.post"`.

### 3.6 Phase 3 E2E Gate 🚧
- [ ] 🧪 Manual: enable `[jetstream]` block in `examples/minimal/atrg.toml` with `host = "jetstream1.us-east.bsky.network"`, `collections = ["app.bsky.feed.post"]`.
- [ ] 🧪 Manual: run for 60s, observe at least 100 rows inserted into `posts`.
- [ ] 🧪 Manual: kill the network for 10s, observe reconnect log lines + resumed inserts.
- [ ] 🧪 Manual: Ctrl-C cleanly shuts down (no panics, no orphaned tasks).
- [ ] 🧪 Manual: deliberately slow handler (`sleep(500ms)` per event) → backpressure visible in `metrics()`, no memory blow-up.
- [ ] 🧪 Manual: `GET /readyz` includes `jetstream` block with metrics.
- [ ] 🧪 Automated: a `wiremock`-style fake Jetstream WebSocket server emits 5 events; assert handler is invoked 5 times with correct payloads. (If a fake WS is too heavy, gate behind `--ignored`.)
- [x] 🧪 Automated: backoff resets on a successful read.
- [x] 🧪 Automated: ZSTD dictionary auto-download to a tempdir cache path works against a local HTTP fixture.
- [ ] 🧪 `cargo fmt`, `cargo clippy`, `cargo test --workspace` all green.
- [ ] 📝 Tag commit as `v0.1.0-phase3`.

---

## Phase 4 — XRPC: typed AT-Proto procedures + lexicon codegen

> **Phase goal:** Users can register XRPC procedures as plain Axum routes with JWT auth out of the box, emit AT-Proto-shaped error envelopes, and optionally generate strongly-typed Rust code from their own lexicon `.json` files via `atrg generate`.

### 4.1 `atrg-xrpc` — Crate setup
- [x] Add deps: `atproto-xrpcs`, `atrg-core`, `atrg-auth`, `atrg-identity`, `axum`, `tower`, `tracing`, `serde`, `serde_json`, `thiserror`.

### 4.2 Router factory + global error fallback
- [x] `pub fn xrpc_router<S: Clone + Send + Sync + 'static>() -> axum::Router<S>`:
  - [x] Pre-applies `atproto-xrpcs` middleware (JWT verification extractor available to mounted handlers).
  - [x] Adds a `/xrpc` namespace fallback returning the AT-Proto envelope `{"error":"MethodNotImplemented","message":"..."}` with status 501.
  - [x] Wraps every mounted handler so that any `axum::Error` or panic on `/xrpc/*` is caught and reshaped into the envelope (status 500, error="InternalServerError"). Users **cannot** accidentally leak Axum's default plain-text 500.
- [x] Re-export the JWT extractor from `atproto-xrpcs` for ergonomics.
- [ ] 🧪 Integration test: a handler that panics under `/xrpc/*` returns the JSON envelope, not Axum's default.
- [ ] 🧪 Integration test: a handler under `/api/*` that panics returns `AtrgError::Internal` JSON (the regular shape, not the XRPC envelope).

### 4.3 `XrpcError` type
- [x] `pub struct XrpcError { pub name: XrpcErrorName, pub message: String }`.
- [x] `pub enum XrpcErrorName { InvalidRequest, AuthRequired, Forbidden, NotFound, RateLimitExceeded, InternalServerError, MethodNotImplemented }`.
- [x] `impl IntoResponse for XrpcError` → `(StatusCode, Json({"error": name, "message": msg}))`.
- [x] Status mapping: InvalidRequest=400, AuthRequired=401, Forbidden=403, NotFound=404, RateLimitExceeded=429, MethodNotImplemented=501, InternalServerError=500.
- [x] Convenience constructors: `xrpc_invalid_request`, `xrpc_auth_required`, `xrpc_forbidden`, `xrpc_not_found`, `xrpc_rate_limit`.
- [x] `impl From<AtrgError> for XrpcError` so handlers can use `?` against atrg's general error type and still emit the right envelope.
- [ ] 🧪 Unit test: every variant maps to the correct status + JSON shape.
- [ ] 🧪 Unit test: `From<AtrgError>` mapping (NotFound → NotFound, Auth → AuthRequired, Database → InternalServerError, BadRequest → InvalidRequest).

### 4.4 Example XRPC route in scaffold
- [ ] Update `crates/atrg-cli/templates/basic/src/routes.rs` to register an example `GET /xrpc/com.example.ping` route returning `{"pong": true, "echo": <query param>}`.
- [ ] Show using `RequireAuth` to demonstrate JWT-protected XRPC procedure.
- [ ] No HTML; pure JSON / XRPC.

### 4.5 `atrg-codegen` — Lexicon-driven code generation 🆕
- [x] Add deps: `atproto-lexicon`, `quote`, `proc-macro2`, `prettyplease`, `syn`, `serde_json`, `anyhow`, `walkdir`, `convert_case`.
- [x] `pub fn generate(input_dir: &Path, output_dir: &Path, opts: GenOptions) -> anyhow::Result<GenReport>`:
  - [x] Walk `input_dir` for `*.json` files.
  - [x] Validate each via `atproto-lexicon::DefaultLexiconResolver` — fail fast with a precise diagnostic (file + JSON Pointer).
  - [x] For every `record` / `object` / `params` / `output` definition: emit a `serde`-derived Rust struct with doc comments quoting the lexicon `description`.
  - [x] For every `query` / `procedure` / `subscription`: emit an Axum handler signature `async fn <method>(State<AppState>, RequireAuth, Json<Input>) -> Result<Json<Output>, XrpcError>` as a stub (with `todo!()` body) wrapped in `#[cfg(feature = "scaffolds")]`-gated default.
  - [x] Emit `pub fn xrpc_routes() -> Router<AppState>` registering every generated handler at the canonical XRPC path (`/xrpc/<nsid>`).
  - [x] Emit AT-URI helper `pub mod uri { pub fn <record>_collection() -> &'static str; pub fn <record>_uri(repo: &str, rkey: &str) -> String; }`.
  - [x] Emit a JSON-Schema validation hook for every input body; failures auto-return `XrpcError::InvalidRequest`.
  - [x] Format generated code with `prettyplease`.
- [x] CLI integration: `atrg generate [--input lexicons/] [--output src/generated/]` (defaults shown).
- [x] `build.rs` integration: ship a `pub fn build_rs(input: &str, output: &str)` so users can run codegen during `cargo build`.
- [x] **Important:** `atrg-codegen` itself ships with **zero** lexicon JSON files. Test fixtures under `crates/atrg-codegen/tests/fixtures/` use synthetic `com.atrg.test.*` lexicons, never `app.bsky.*`.
- [ ] 🧪 Unit test: a minimal fixture lexicon → expected struct fields.
- [ ] 🧪 Unit test: a malformed lexicon produces a friendly error pointing at the offending file.
- [ ] 🧪 Integration test: generate code into a tempdir, then `cargo build` it as part of a fixture project.
- [ ] 🧪 Integration test: generated handler returns `XrpcError::InvalidRequest` when the request body fails schema validation.
- [ ] 📝 Doc page `docs/codegen.md` with a complete walkthrough using a custom `com.example.todo.*` lexicon.

### 4.6 Phase 4 E2E Gate 🚧
- [ ] 🧪 Manual: `curl http://localhost:3000/xrpc/com.example.ping?echo=hi` returns `{"pong":true,"echo":"hi"}`.
- [ ] 🧪 Manual: `curl http://localhost:3000/xrpc/com.example.does.not.exist` returns the AT-Proto `MethodNotImplemented` envelope (status 501).
- [ ] 🧪 Manual: a procedure declared as auth-required rejects requests without a valid JWT with `{"error":"AuthRequired",...}` (status 401).
- [ ] 🧪 Manual: a procedure with a malformed body returns `{"error":"InvalidRequest",...}` (status 400).
- [ ] 🧪 Manual: a procedure that panics returns `{"error":"InternalServerError",...}` (status 500), NOT Axum's default plain-text.
- [ ] 🧪 Manual: `atrg generate --input ./lexicons --output ./src/generated` against a hand-rolled `com.example.todo.*` lexicon emits compilable Rust.
- [ ] 🧪 Manual: the generated `xrpc_routes()` mounts and responds to `/xrpc/com.example.todo.list` correctly.
- [x] 🧪 Automated: `oneshot` against `xrpc_router` confirms the fallback and a registered route work.
- [x] 🧪 Automated: codegen golden-file test (input lexicon → expected Rust output, byte-for-byte after `prettyplease`).
- [ ] 🧪 `cargo fmt`, `cargo clippy`, `cargo test --workspace` all green.
- [ ] 📝 Tag commit as `v0.1.0-phase4`.

---

## Phase 5 — Social API scaffold + `atrg-testing`

> **Phase goal:** A developer has a recognizable micro-social-network **API** running locally in five minutes, plus a first-class testing crate (`atrg-testing`) that lets them unit-test handlers without any live network.
>
> **Important:** The `social` template is an *example application* built on top of the gateway. It demonstrates how a developer can use atrg to create an AT Protocol social API. The framework itself remains lexicon-free; the template just happens to use `app.bsky.*` lexicons in its own routes and Jetstream handler. This is the same pattern any developer would follow to build their custom social network — they could just as easily target `com.mything.*` and never touch `app.bsky.*`. All lexicon-aware code lives in the generated user project, **not** inside any atrg crate.

### 5.0 `atrg-testing` — Test utilities crate 🆕
- [x] Add deps: `atrg-core`, `atrg-auth`, `atrg-stream`, `atrg-identity`, `axum`, `tower`, `sqlx`, `tokio`, `serde_json`, `wiremock`.
- [x] `pub async fn test_app() -> (AtrgApp, AppState)`:
  - [x] In-memory SQLite (`sqlite::memory:`) with all migrations applied.
  - [x] Stub `IdentityResolver` returning fixture DID documents.
  - [x] No Jetstream task spawned.
  - [x] No real `reqwest::Client` outbound — wired to a local `wiremock` server.
- [x] `pub struct MockAtprotoClient { calls: Mutex<Vec<RecordedCall>>, responses: HashMap<String, serde_json::Value> }`:
  - [x] Implements the same surface as `atproto-client::Client` for `get_record`, `put_record`, `list_records`, `delete_record`.
  - [x] `expect_call(method, path).returns(json!(...))`.
  - [x] `assert_called(method, path, n_times)`.
- [x] `pub struct FakeJetstream { tx: mpsc::Sender<JetstreamEvent> }`:
  - [x] `emit(event)` pushes a synthetic event to the registered handler.
  - [x] `emit_post(did, rkey, text)` convenience for AT-Proto-shaped post commits.
- [ ] `pub struct InMemoryPds`:
  - [ ] Spawns an Axum server bound to `127.0.0.1:0`.
  - [ ] Implements just enough of `/xrpc/com.atproto.server.*` to complete an OAuth round-trip.
  - [ ] Exposes its base URL so tests can configure `auth.client_id` against it.
- [x] `pub fn seed_session(pool: &SqlitePool, did: &str, handle: &str) -> AtrgSession` — convenience for handler tests that need an authenticated user.
- [ ] 🧪 Self-test: `test_app` returns a working app where `oneshot GET /` succeeds.
- [ ] 🧪 Self-test: `MockAtprotoClient` records calls and returns scripted responses.
- [ ] 🧪 Self-test: `FakeJetstream` triggers the registered `on_event` handler.
- [ ] 🧪 Self-test: `InMemoryPds` round-trips an OAuth flow when paired with `atrg-auth`.
- [ ] 📝 Doc page `docs/testing.md` with the pattern for "test a handler in isolation".

### 5.1 Template files (all JSON, zero HTML)
- [x] Create `crates/atrg-cli/templates/social/` containing:
  - [x] `Cargo.toml` (depends on `atrg-core`, `atrg-auth`, `atrg-stream`, `atrg-xrpc`, `atrg-identity`, `atproto-client`, `chrono`, `serde`, `axum`, `sqlx`; `atrg-testing` under `[dev-dependencies]`).
  - [x] `atrg.toml` with `[jetstream]` enabled by default watching `app.bsky.feed.post`, `app.bsky.feed.like`, `app.bsky.graph.follow`.
  - [x] `migrations/0001_posts.sql` — `posts(did, rkey, text, created_at, indexed_at)`.
  - [x] `migrations/0002_follows.sql` — `follows(subject_did, target_did, created_at)`.
  - [x] `migrations/0003_likes.sql` — `likes(did, subject_uri, created_at)`.
  - [x] `src/main.rs`.
  - [x] `src/routes.rs`.
  - [x] `src/handlers/timeline.rs` — `GET /api/timeline?cursor=&limit=` returns `{"items":[...], "cursor":"..."}`.
  - [x] `src/handlers/profile.rs` — `GET /api/profile/:handle` returns `{did, handle, post_count, follower_count, following_count}`.
  - [x] `src/handlers/posts.rs` — `POST /api/posts` (RequireAuth + JSON body `{text}`) creates a post via `atproto-client`.
  - [x] `src/jetstream.rs` — `on_event` populating posts/follows/likes.
  - [x] `README.md` describing every endpoint with `curl` examples.

### 5.2 `atrg new <name> --template social`
- [ ] Add `--template social` arg to `atrg new`.
- [ ] Embed the social template via `include_dir`.
- [ ] Include sample handler tests using `atrg-testing` so users see the pattern from day one.
- [ ] 🧪 Integration test: scaffolded project compiles.
- [ ] 🧪 Integration test: scaffolded `atrg.toml` parses cleanly.
- [ ] 🧪 Integration test: assert no `templates/` or `static/` directory exists in the scaffold output.
- [ ] 🧪 Integration test: scaffolded `cargo test` passes (uses `atrg-testing` mocks, no network).

### 5.2b Custom-namespace example: `examples/todo`
- [ ] Add a fully-worked second example under `examples/todo/` that uses a custom `com.example.todo.*` lexicon, generated via `atrg generate`.
- [ ] Demonstrates: lexicon authoring → codegen → XRPC procedure → handler test with `atrg-testing`.
- [ ] Proves the framework is genuinely lexicon-agnostic — not tied to any specific application namespace.
- [ ] 📝 Doc-link from `README.md` and `docs/getting-started.md`.

### 5.3 Pagination convention
- [x] All list endpoints accept `?cursor=<opaque>&limit=<u32>` (max 100).
- [x] Return `{"items":[...], "cursor": "<next>" | null}`.
- [x] Cursor format = base64-encoded `"<created_at_unix_ms>:<rkey>"`.
- [x] Add a `pub fn encode_cursor` / `decode_cursor` helper in `atrg-core::pagination` and reuse across handlers.
- [x] 🧪 Unit tests for cursor encode/decode round-trip + invalid cursor → `BadRequest`.

### 5.4 Phase 5 E2E Gate 🚧
- [ ] 🧪 Manual: `atrg new socialdemo --template social && cd socialdemo && atrg dev`.
- [ ] 🧪 Manual: complete OAuth flow.
- [ ] 🧪 Manual: `curl -X POST -H "Authorization: Bearer ..." -d '{"text":"hello from atrg"}' http://localhost:3000/api/posts` succeeds; verify post appears on the AT Protocol network.
- [ ] 🧪 Manual: leave `dev` running for 60s, then `curl http://localhost:3000/api/timeline?limit=20` returns ≥20 ingested posts.
- [ ] 🧪 Manual: `curl http://localhost:3000/api/profile/<handle>` returns plausible counts.
- [ ] 🧪 Manual: pagination — `?cursor=<from previous>` returns the next page without overlap.
- [ ] 🧪 Automated: in-memory tests for each handler (seed DB rows, hit via `oneshot`, assert JSON).
- [ ] 🧪 `cargo fmt`, `cargo clippy`, `cargo test --workspace` all green.
- [ ] 📝 Tag commit as `v0.1.0-phase5`.

---

## Phase 6 — Polish, DX, Docs, Release

> **Phase goal:** Everything looks and feels production-ready. Crates publishable to crates.io.

### 6.1 `atrg routes`
- [x] Implement by introspecting the `axum::Router` (Axum 0.8 supports walking routes via `Router::routes()` or via a custom registry). *(Implemented as a static table of built-in routes rather than Axum introspection.)*
- [ ] If introspection is too brittle, fall back to a registry: when users mount a router, also feed atrg the list of `(method, path)` tuples via a small macro or builder helper.
- [ ] Output a TTY table: `METHOD PATH HANDLER`.
- [ ] Annotate XRPC routes with the lexicon NSID they came from when generated by `atrg-codegen`.
- [ ] 🧪 Integration test against a known set of routes.

### 6.1b Security headers middleware 🔒
- [x] In `atrg-core`, add `pub fn build_security_headers_layer(env: &str) -> tower_http::set_header::SetResponseHeaderLayer`:
  - [x] When `env != "development"`: set `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: strict-origin-when-cross-origin`, `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`.
  - [x] In development: skip CSP and `X-Frame-Options` to ease local browser DevTools work.
- [x] Wire into `AtrgApp::run()` step 7 after CORS.
- [x] 🧪 Integration test: in production env, headers are present on `/api/me`.
- [x] 🧪 Integration test: in development env, CSP is absent.
- [x] 🧪 Integration test: headers do not break the OAuth redirect flow.

### 6.2 `atrg migrate` polish
- [ ] Add `atrg migrate --status` (lists applied vs pending).
- [ ] Add `atrg migrate --revert <n>` (no-op stub if not feasible in v0.1.0; document as TODO).
- [ ] 🧪 Integration tests for both subcommands.

### 6.3 `atrg build`
- [x] Wraps `cargo build --release` with timing summary + binary path.
- [ ] 🧪 Integration test: produces a binary in `target/release/<name>`.

### 6.4 Structured request logging
- [x] Customize `TraceLayer::new_for_http()` to log JSON lines with method, path, status, latency_ms, request_id.
- [x] Add a request-id middleware that reads `X-Request-Id` or generates a UUID.
- [x] 🧪 Integration test: response carries the `X-Request-Id` header.

### 6.5 Health & readiness endpoints (built-in)
- [x] `GET /healthz` — always 200 `{"ok": true}`.
- [x] `GET /readyz` — 200 if DB reachable, else 503 with diagnostics.
- [x] Auto-mounted in `AtrgApp::run` (users can override by mounting their own).
- [x] 🧪 Integration tests for both, including the failure path of `/readyz`.

### 6.6 Documentation 📝
- [x] Write `docs/getting-started.md` (the 5-minute quickstart, JSON only).
- [x] Write `docs/oauth.md` (how OAuth flow works, what `client-metadata.json` needs in production, plus the two-token model: atrg session token vs. AT Protocol JWT).
- [x] Write `docs/jetstream.md` (writing event handlers, error semantics, backpressure, ZSTD dictionary, ordering guarantees).
- [x] Write `docs/xrpc.md` (registering procedures, JWT auth, `XrpcError` envelope, auth-required vs. anonymous).
- [x] Write `docs/codegen.md` (full lexicon → working endpoint walkthrough using a custom namespace).
- [x] Write `docs/testing.md` (using `atrg-testing` to mock the AT Protocol surface).
- [ ] Write `docs/identity-cache.md` (DID resolution, TTL tuning, invalidation).
- [ ] Write `docs/json-api-conventions.md` (error envelope, pagination, CORS, security headers).
- [ ] Write `docs/deploying.md` (systemd unit, fly.io, Docker example — Dockerfile must NOT include any Node / npm / asset pipeline).
- [x] Write `docs/rfcs/0000-template.md` and `docs/rfcs/README.md` describing the lightweight RFC process for non-trivial features.
- [x] Update top-level `README.md` with a feature matrix, the "no frontend" disclaimer, and the "lexicon-agnostic / pure gateway" tagline prominent.
- [x] Add a `CONTRIBUTING.md` with Discord / community-engagement expectations.
- [x] Run `cargo doc --workspace --no-deps` and ensure no missing-docs warnings.

### 6.7 Release prep ⚙️
- [x] Write `CHANGELOG.md` covering all phases.
- [ ] `cargo publish --dry-run` for each crate in dependency order: `atrg-core`, `atrg-db`, `atrg-identity`, `atrg-auth`, `atrg-stream`, `atrg-xrpc`, `atrg-codegen`, `atrg-testing`, `atrg-cli`.
- [ ] Add `release.yml` workflow that publishes on tag push.
- [ ] Verify `cargo install atrg-cli` from crates.io works on a clean machine.
- [ ] 📝 Versioning policy doc (`docs/versioning.md`) covering MSRV, `atproto-crates` pin range, SemVer commitments, and how protocol-level upstream changes are absorbed without breaking user code.

### 6.8 Phase 6 E2E Gate 🚧 — RELEASE GATE

> **Note:** The E2E CI workflow (`.github/workflows/e2e.yml`) has been created to automate many of these checks in CI. Items marked ✅ below are covered by that workflow. Items involving real PDS/network interaction or crates.io publishing remain manual.

- [ ] 🧪 On a brand new VM: `cargo install atrg-cli` from crates.io.
- [x] 🧪 `atrg new release-test && cd release-test && atrg dev` → JSON 200 at `/`. *(Automated in e2e.yml)*
- [ ] 🧪 OAuth flow works against a real PDS.
- [ ] 🧪 PDS-issued JWT in `Authorization: Bearer` is verified and accepted by `RequireAuth`.
- [ ] 🧪 Token refresh: stale access token transparently refreshes on the next outbound call.
- [ ] 🧪 Jetstream ingest works for ≥5 minutes without errors; backpressure metrics visible.
- [ ] 🧪 ZSTD dictionary auto-fetch works against a fresh cache directory.
- [x] 🧪 An XRPC route works end-to-end and emits proper error envelopes for every failure mode. *(Automated in e2e.yml)*
- [x] 🧪 `atrg generate` against a custom `com.example.todo.*` lexicon produces compilable Rust. *(Automated in e2e.yml)*
- [x] 🧪 `examples/todo` (custom-namespace lexicon) runs end-to-end. *(Automated in e2e.yml)*
- [x] 🧪 `atrg new social-demo --template social` works end-to-end and `cargo test` inside the scaffold passes using `atrg-testing` mocks. *(Automated in e2e.yml)*
- [x] 🧪 `atrg routes`, `atrg migrate --status`, `atrg build`, `atrg generate` all work. *(Automated in e2e.yml)*
- [x] 🧪 `/healthz` and `/readyz` respond as expected; `/readyz` includes Jetstream + identity-cache metrics. *(Automated in e2e.yml)*
- [ ] 🧪 DID/handle resolution cache shows >90% hit rate after a warmup period.
- [x] 🧪 No `unwrap()` / `expect()` reachable in any library crate (audit via `grep` + `clippy::unwrap_used`). *(Automated in e2e.yml)*
- [x] 🧪 `cargo doc --workspace --no-deps` is warning-free. *(Automated in e2e.yml)*
- [x] 🧪 `cargo deny check` is green. *(Automated in e2e.yml)*
- [x] 🔒 Security checklist: *(Automated in e2e.yml)*
  - [x] No secrets in scaffold defaults usable in production (warn + refuse to bind non-localhost with `CHANGE_ME` secret).
  - [x] Cookies `Secure + HttpOnly + SameSite=Lax` in non-dev mode.
  - [x] No access tokens, refresh tokens, or JWTs in JSON bodies or logs.
  - [x] CORS defaults to closed; preflight `OPTIONS` works for `/api/*` and `/auth/*`.
  - [x] Security headers present in production env (`X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`, `CSP`).
  - [ ] JWT verification rejects expired, wrong-audience, and untrusted-issuer tokens.
- [ ] 📝 Tag `v0.1.0` and push to crates.io.
- [ ] 📝 Publish announcement / release notes referencing `CHANGELOG.md` (post in AT Protocol developer Discord).

---

## Cross-cutting Quality Bar (applies every phase)

- [ ] Each PR updates the corresponding phase checklist (check items off in `ROADMAP.md`).
- [ ] Each PR adds at least one test (unit or integration) for any new behavior.
- [ ] Each PR runs `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` locally before requesting review.
- [ ] Public APIs get doc comments with at least one example.
- [ ] No new dependencies without a justification in the PR description (must be from the `CLAUDE.md` whitelist or an addition discussed first).
- [ ] No HTML, no template engines, no static asset crates ever sneak into the dependency graph (`cargo tree | grep -E 'minijinja|tera|askama|handlebars'` must return nothing).
- [ ] No bundled lexicons sneak into any atrg crate. `grep -RE '"app\.bsky\.|"com\.atproto\."' crates/` must return zero results (lexicon strings are allowed only under `crates/atrg-cli/templates/social/` and `examples/`, which are user-space code).
- [ ] No atrg crate gains a feature flag named `lexicons` or similar that implies bundled schemas.
- [ ] No `/xrpc/*` route ever returns a non-AT-Proto-shaped error response (CI test asserts on every error path).
- [ ] No PR adds raw `atproto-identity::resolve_subject()` calls inside library code; all resolution goes through `state.identity` so caching is uniform.
- [ ] No PR introduces an unbounded channel or `Vec` buffer in event-processing code.
- [ ] Anything touching public API, auth semantics, XRPC envelope, or codegen output requires a `docs/rfcs/NNNN-*.md` proposal before merge.

---

## Out of Scope for v0.1.0 (explicit)

- Frontend of any kind (HTML, CSS, JS, templating, static assets).
- ORM (write SQL).
- Multi-tenancy.
- Custom session backends (atrg_sessions table only).
- Hot-reload outside of what `cargo-watch` gives you.
- Full PDS implementation.
- Server→client push (SSE, WebSockets).
- Distributed identity cache (single-process `moka` only; Redis-backed cache tracked for post-v0.2).
- **Bundled lexicons of any kind.** No `app.bsky.*`, no `com.atproto.*`. Developers bring their own lexicon `.json` files.

---

## v0.2.0 — Production-Ready Framework (changala.app-informed)

> **Goal of v0.2.0:** A developer can build a production ATProto app with the complexity of [changala.app](https://github.com/changala-social/changala.app) (62 XRPC endpoints, Postgres, S3 blobs, RBAC, API keys, multi-binary workspace, firehose materialisation) using **only** atrg framework features — zero custom migration runners, zero `once_cell` globals, zero auth middleware workarounds.
>
> Every item in this phase addresses a real pain point from changala.app's implementation. See `CLAUDE.md` "Lessons from changala.app" for the full analysis.

---

### 7. Core Infrastructure Fixes

These fix fundamental issues that block real-world usage.

#### 7.1 Migration Namespace Isolation
- [ ] atrg internal migrations use `_atrg_migrations` tracking table (not `_sqlx_migrations`)
- [ ] `AtrgApp::with_migrations_dir(path)` sets app migrations directory (default: `./migrations`)
- [ ] `AtrgApp::with_migrations_table(name)` overrides the tracking table name
- [ ] Multiple migration directories for multi-crate workspaces (Ring + Aggregator)
- [ ] `atrg migrate` CLI respects configured directory and table
- [ ] Reusable utility: `atrg_db::run_migrations(pool, dir, table)`

#### 7.2 Postgres as First-Class Backend
- [ ] `atrg new --db postgres` scaffold option
- [ ] Feature flags `postgres` / `sqlite` on `atrg-core`, `atrg-auth`, `atrg-db`
- [ ] All internal SQL tested against both backends in CI
- [ ] Internal migrations ship as both SQLite and Postgres variants
- [ ] `atrg-db` re-exports correct pool type based on feature flag

#### 7.3 `AppState` Extension Mechanism
- [ ] `extensions: Arc<TypeMap>` field in `AppState`
- [ ] `AtrgApp::with_extension::<T>(value)` inserts typed app state
- [ ] `state.extension::<T>() -> &T` retrieves it
- [ ] `state.try_extension::<T>() -> Option<&T>` non-panicking variant
- [ ] Accessible from route handlers AND `on_event` handlers

#### 7.4 Fix `client-metadata.json` Origin Derivation
- [ ] `client_uri = origin_of(config.auth.client_id)` instead of `http://{host}:{port}`
- [ ] Same fix for `/.well-known/oauth-protected-resource`
- [ ] `origin_of(url: &str) -> String` utility in `atrg-auth`

#### 7.5 Cross-Origin Auth Handoff
- [ ] `[auth] post_login_redirect` config field
- [ ] After OAuth callback: redirect to `{post_login_redirect}?token={id}&did={did}&handle={handle}`
- [ ] Backward compatible: cookie-based flow when field is absent
- [ ] `ATRG_AUTH__POST_LOGIN_REDIRECT` env var override

#### 7.6 Environment Variable Overrides
- [ ] Every `atrg.toml` field overridable via `ATRG_SECTION__KEY` env vars
- [ ] Secrets prefer env vars over TOML
- [ ] App-specific config: `AtrgApp::app_config_with_env::<T>(section, prefix)`
- [ ] Log applied overrides at startup

#### 7.7 TID and AT-URI Utilities
- [ ] `Tid::now() -> String` — AT Protocol Timestamp ID
- [ ] `Tid::from_str(s) -> Result<Tid>`
- [ ] `AtUri::new(did, collection, rkey)` / `AtUri::parse(uri)`
- [ ] Re-exported from `atrg_core`

---

### 8. Auth & Access Control

#### 8.1 API Key Authentication
- [ ] `api_keys` table (auto-migrated): `key_hash, key_prefix, did, name, scopes, expires_at`
- [ ] `RequireAuth` natively recognizes prefixed API keys (no synthetic sessions)
- [ ] `AtrgSession.source` gains `ApiKey` variant
- [ ] Configurable prefix: `[auth] api_key_prefix = "atrg_"`
- [ ] Built-in: `atrg_auth::api_keys::{create, list, revoke}`
- [ ] CLI: `atrg api-key create --did did:plc:xxx --scope "admin:*"`
- [ ] `ATRG_BOOTSTRAP_API_KEY` env var for first-time setup

#### 8.2 Role-Based Access Control
- [ ] `roles` table (auto-migrated): `did, role, scope_type, scope_id, granted_by, granted_at`
- [ ] `RequireRole<R>` extractor — rejects with 403 if role missing
- [ ] Scoped roles: `require_role_or_above(state, did, "classRep", Some(resource_id))`
- [ ] Role hierarchy: configurable numeric levels
- [ ] Admin bootstrap: `[app] admin_dids` auto-provisions on startup
- [ ] `ATRG_APP__ADMIN_DIDS` env var (comma-separated)

#### 8.3 Ban / Moderation Primitives
- [ ] `bans` table (auto-migrated): `did, reason, expires_at, created_by`
- [ ] `CheckNotBanned` extractor — rejects banned DIDs with 403
- [ ] TTL support: bans auto-expire
- [ ] XRPC helpers: `ban_did`, `lift_ban`, `list_bans`, `is_banned`

---

### 9. Data & Storage

#### 9.1 Blob Storage (`atrg-blob`)
- [ ] `BlobStore` trait: `put(data) -> CID`, `get(cid) -> bytes`, `exists`, `delete`
- [ ] `S3BlobStore` implementation (using `rust-s3`)
- [ ] `FileBlobStore` for development
- [ ] Content addressing: SHA-256 with `sha256-` prefix
- [ ] Config via `[blobs]` section + env var overrides
- [ ] `AppState.blobs: Option<Arc<dyn BlobStore>>`

#### 9.2 Record Repository (`atrg-repo`)
- [ ] `RecordClient` wrapping `atproto_client::Client`
- [ ] `create_record`, `get_record`, `put_record`, `delete_record`, `list_records`
- [ ] `upload_blob(data, mime) -> BlobRef`

#### 9.3 Jetstream Cursor Persistence
- [ ] Store last `time_us` in `atrg_cursors` table
- [ ] On restart: resume from stored cursor or "now"
- [ ] Config: `cursor = "auto"` | `cursor = "live"`

---

### 10. Event Processing & Firehose

#### 10.1 Event Router (`atrg-stream` enhancement)
- [ ] `EventRouter::new().on_create(collection, handler).build()`
- [ ] Typed `CommitEvent { did, rkey, collection, operation, record }`
- [ ] `.on_create()`, `.on_delete()`, `.on()` (all ops)
- [ ] Default: ignore unregistered collections with debug log

#### 10.2 Firehose / Relay Subscription (`atrg-firehose`)
- [ ] WebSocket connection to relay (`subscribeRepos`)
- [ ] DAG-CBOR + CAR parsing
- [ ] `FirehoseEvent { seq, repo_did, collection, rkey, operation, record }`
- [ ] `AtrgApp::on_firehose(handler)` builder
- [ ] Config: `[firehose] relay = "wss://bsky.network"`, `cursor = "auto"`
- [ ] Bounded backpressure + reconnection with cursor resume

#### 10.3 Feed Generator Framework (`atrg-feed`)
- [ ] `FeedGenerator` trait: `id()`, `display_name()`, `get_skeleton()`
- [ ] Auto-mount `getFeedSkeleton` XRPC handler
- [ ] `/.well-known/did.json` for feed generator DID
- [ ] `atrg feed publish` CLI

---

### 11. Scaffolding & DX

#### 11.1 Multi-Binary Workspace Template
- [ ] `atrg new --template multi-binary` generates:
  - `crates/my-app-server/` (write server)
  - `crates/my-app-aggregator/` (firehose subscriber)
  - `crates/my-app-shared/` (generated types)
- [ ] Each binary: own migrations directory + tracking table
- [ ] `atrg dev --bin my-app-server` hot-reload support

#### 11.2 Email / OTP Module (`atrg-email`)
- [ ] SMTP via `[email]` config section (lettre)
- [ ] `send_otp(state, did, email)` / `verify_otp(state, did, email, code)`
- [ ] `otp_codes` table auto-migrated
- [ ] Dev mode: log OTPs to stdout
- [ ] Domain allowlist helper

#### 11.3 Dockerfile Generation
- [ ] `atrg new` generates multi-stage Dockerfile
- [ ] `.dockerignore` included
- [ ] `docker compose` with app + Postgres + MinIO

---

### 12. Production Hardening

#### 12.1 Graceful Shutdown
- [ ] Trap SIGTERM/SIGINT, drain in-flight requests
- [ ] Flush Jetstream consumer, persist cursor
- [ ] Configurable shutdown timeout (default 30s)

#### 12.2 Rate Limiting
- [ ] Token-bucket middleware, per-DID and per-IP
- [ ] `[rate_limit] requests_per_second`, `burst` in `atrg.toml`
- [ ] `RateLimitExceeded` with `Retry-After` per AT Protocol spec

---

### v0.2.0 E2E Test Suite 🚧

> **The framework is not v0.2.0-ready until ALL of these pass.** This test suite validates that a changala.app-complexity application can be built without workarounds.

#### Infrastructure Tests
- [ ] Migration isolation: framework + app migrations in same DB, no conflicts
- [ ] Migration isolation: two app binaries with separate migration directories
- [ ] Postgres: all internal tables created with valid DDL
- [ ] SQLite: all internal tables created with valid DDL (parity)
- [ ] `AppState` extensions: insert in `main`, retrieve in handler + event handler
- [ ] Env var overrides: `ATRG_APP__PORT=9999` changes bind port
- [ ] App config: `[myapp]` section parsed with `MYAPP_*` env overlay

#### Auth Tests
- [ ] `client-metadata.json` valid for `http://localhost:3000` and `https://prod.example.com`
- [ ] Cross-origin OAuth: SPA on `:5173` authenticates via API on `:3000` using `post_login_redirect`
- [ ] API key `Bearer atrg_xxx` works through `RequireAuth` (no synthetic sessions)
- [ ] API key scopes: scoped key rejected for out-of-scope operation
- [ ] Role check: `RequireRole(Admin)` returns 403 for non-admin DID
- [ ] Scoped role: classRep for course A cannot manage course B
- [ ] Ban: banned DID receives 403 on write, 200 on read
- [ ] Ban TTL: expired ban no longer blocks
- [ ] Admin bootstrap: `ATRG_APP__ADMIN_DIDS` provisions admin on startup

#### Data Tests
- [ ] Blob store: `put()` → `get()` roundtrip (S3 + file backends)
- [ ] Content addressing: same data = same CID (dedup)
- [ ] TID generation: valid format, monotonically increasing
- [ ] AT-URI: `new()` → `parse()` roundtrip
- [ ] Record CRUD: create → get → put → delete via `RecordClient`
- [ ] Cursor persistence: Jetstream cursor survives restart

#### Event Processing Tests
- [ ] EventRouter dispatches `on_create` correctly
- [ ] EventRouter ignores unregistered collections
- [ ] EventRouter handles `on_delete` for tombstoned records
- [ ] Firehose: connect to mock relay, parse events, verify typed output
- [ ] Firehose: disconnect + reconnect with cursor resume

#### Scaffold Tests
- [ ] `atrg new my-app --db postgres` produces buildable project
- [ ] `atrg new my-app --template multi-binary` produces buildable workspace with 2 binaries
- [ ] Generated Dockerfile builds and runs
- [ ] `atrg dev --bin my-app-server` starts correctly

#### Integration Test (Full Stack)
- [ ] Boot a write server + aggregator from multi-binary template
- [ ] Write server: OAuth login, create XRPC record, store blob
- [ ] Aggregator: receives Jetstream event, materialises into DB
- [ ] Write server: API key auth works for MCP-style programmatic access
- [ ] Graceful shutdown: SIGTERM → in-flight request completes → exit 0
- [ ] Rate limit: 429 + Retry-After after burst exceeded

---

## v0.3.0 — Advanced Protocol Features

> **Goal:** Support labelers, server→client push, and production observability.

### 13. Labeler Framework (`atrg-label`)
- [ ] `Label` struct + `sign_label(label, signing_key)`
- [ ] Label storage table (auto-migrated)
- [ ] `/xrpc/com.atproto.label.subscribeLabels` WebSocket endpoint
- [ ] `/.well-known/did.json` with labeler service endpoint
- [ ] `[labeler] signing_key` config
- [ ] `atrg label declare` CLI

### 14. Server→Client Push
- [ ] `atrg_push::sse_endpoint(channel)` — SSE via broadcast channel
- [ ] `atrg_push::ws_endpoint(handler)` — authenticated WebSocket
- [ ] JSON event envelope: `{ type, payload }`
- [ ] Configurable keep-alive, max connections

### 15. Observability
- [ ] `GET /metrics` Prometheus endpoint
- [ ] Default metrics: HTTP request count/duration, Jetstream lag, connections
- [ ] OpenTelemetry: `tracing-opentelemetry` + OTLP exporter
- [ ] `[telemetry] otlp_endpoint`, `service_name` config
- [ ] `GET /admin/state` — pool stats, cache hit rates, Jetstream lag

### v0.3.0 E2E Gate 🚧
- [ ] Label creation + WebSocket subscription + signature verification
- [ ] SSE: 1000 concurrent connections, no dropped events
- [ ] WebSocket: authenticated message exchange
- [ ] `/metrics` returns valid Prometheus format
- [ ] OTLP trace appears in test collector

---

## Post-v0.3.0 Horizon

| Feature | Notes |
|---|---|
| Distributed identity cache (Redis) | Multi-instance deploys |
| Multi-instance coordination | Leader election for consumers |
| Background job queue | Persistent deferred work |
| Plugin / middleware registry | Community crates |
| `atrg-bluesky` convenience crate | Out-of-tree `app.bsky.*` types |
| Account management helpers | Handle changes, migrations, deletions |
| Lexicon hot-reload | Re-run codegen without full rebuild |
| MCP server integration (`atrg-mcp`) | AI-powered admin tooling |

---

## Phase Dependency Graph

```
Phase 0–6 (v0.1.0: skeleton, auth, Jetstream, XRPC, codegen, polish)
    │
    ▼
v0.1.0 RELEASE
    │
    ├────────────────┬───────────────┬─────────────────┬────────────────┐
    ▼                ▼               ▼                 ▼                ▼
  §7 Infra         §8 Auth &       §9 Data &        §10 Events &    §11-12 DX &
  Fixes            Access          Storage          Firehose       Hardening
  (migrations,    (API keys,      (blobs,          (EventRouter,  (multi-binary,
   Postgres,       RBAC,           repo,            firehose,      email, Docker,
   AppState,       bans,           cursor)          feeds)         graceful
   client-meta,    admin                                           shutdown,
   cross-origin,   bootstrap)                                      rate limit)
   env vars,
   TID/AT-URI)
    │                │               │                 │                │
    └────────────────┴───────────────┴─────────────────┴────────────────┘
                                     │
                               v0.2.0 RELEASE
                                     │
                          ┌──────────┼──────────┐
                          ▼          ▼          ▼
                       §13 Label  §14 Push   §15 Observ.
                          │          │          │
                          └──────────┼──────────┘
                                     │
                               v0.3.0 RELEASE
```

All v0.2.0 sections (§7–12) are independent and can be parallelized. The v0.2.0 E2E test suite is the single release gate — all tests green = ship.